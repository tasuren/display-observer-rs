use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{Arc, Mutex},
};

use objc2_core_foundation::{CGPoint, CGSize};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGDisplayChangeSummaryFlags, CGDisplayIsMain,
    CGDisplayMirrorsDisplay, CGDisplayRegisterReconfigurationCallback,
    CGDisplayRemoveReconfigurationCallback, CGError, CGGetActiveDisplayList, kCGNullDirectDisplay,
};
use smallvec::SmallVec;

use crate::{Display, DisplayEventCallback, Event, MayBeDisplayAvailable, Origin, Size};

/// The type alias for macOS display ID, which is [`CGDirectDisplayID`][CGDirectDisplayID].
///
/// [CGDirectDisplayID]: https://developer.apple.com/documentation/coregraphics/cgdirectdisplayid?language=objc
pub type MacOSDisplayId = CGDirectDisplayID;

/// The error type for macOS-specific operations, which is [`CGError`][CGError].
///
/// [CGError]: https://developer.apple.com/documentation/coregraphics/cgerror?language=objc
pub type MacOSError = CGError;

trait CGErrorToResult {
    fn into_result<T>(self, value: T) -> Result<T, MacOSError>;
}

impl CGErrorToResult for CGError {
    fn into_result<T>(self, value: T) -> Result<T, MacOSError> {
        if self == CGError::Success {
            Ok(value)
        } else {
            Err(self)
        }
    }
}

impl From<CGSize> for Size {
    fn from(value: CGSize) -> Self {
        Self {
            width: value.width as _,
            height: value.height as _,
        }
    }
}

impl From<CGPoint> for Origin {
    fn from(value: CGPoint) -> Self {
        Self {
            x: value.x as _,
            y: value.y as _,
        }
    }
}

/// A macOS-specific display.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacOSDisplay {
    pub(crate) id: MacOSDisplayId,
}

impl MacOSDisplay {
    /// Create a new `MacOSDisplay` from a [`CGDirectDisplayID`][CGDirectDisplayID].
    ///
    /// [CGDirectDisplayID]: https://developer.apple.com/documentation/coregraphics/cgdirectdisplayid?language=objc
    pub fn new(id: MacOSDisplayId) -> Self {
        Self { id }
    }

    /// Get the [`CGDirectDisplayID`][CGDirectDisplayID] of the display.
    ///
    /// [CGDirectDisplayID]: https://developer.apple.com/documentation/coregraphics/cgdirectdisplayid?language=objc
    pub fn id(&self) -> MacOSDisplayId {
        self.id
    }

    /// Get the origin (top-left corner) of the display in screen coordinates.
    pub fn origin(&self) -> Origin {
        CGDisplayBounds(self.id).origin.into()
    }

    /// Get the current resolution (width and height) of the display.
    pub fn size(&self) -> Size {
        CGDisplayBounds(self.id).size.into()
    }

    /// Check if this display is the primary (main) display.
    pub fn is_primary(&self) -> bool {
        CGDisplayIsMain(self.id)
    }

    /// Check if this display is currently mirrored.
    ///
    /// If a display is mirrored, it means its content is identical to another display.
    pub fn is_mirrored(&self) -> bool {
        CGDisplayMirrorsDisplay(self.id) != kCGNullDirectDisplay
    }

    /// Get the [`CGDirectDisplayID`][CGDirectDisplayID] of the primary display if this display is mirrored.
    ///
    /// Returns `None` if the display is not mirrored or is the primary display itself.
    ///
    /// [CGDirectDisplayID]: https://developer.apple.com/documentation/coregraphics/cgdirectdisplayid?language=objc
    pub fn get_primary_id(&self) -> Option<MacOSDisplayId> {
        let primary_id = CGDisplayMirrorsDisplay(self.id);

        if primary_id == kCGNullDirectDisplay {
            None
        } else {
            Some(primary_id)
        }
    }
}

/// Get a list of all currently active macOS displays.
///
/// # Returns
/// A `Result` containing a `Vec` of [`Display`] objects on success, or a [`MacOSError`] on failure.
///
/// # Errors
/// This function can return a [`MacOSError`] if there's an issue with Core Graphics.
pub fn get_displays() -> Result<Vec<Display>, MacOSError> {
    const MAX_DISPLAYS: u32 = 20;
    let mut active_displays = [0; MAX_DISPLAYS as _];
    let mut display_count = 0;

    unsafe {
        CGGetActiveDisplayList(
            MAX_DISPLAYS,
            &raw mut active_displays as *mut _,
            &mut display_count,
        )
        .into_result(())?;
    }

    let mut displays = Vec::new();
    for &display_id in active_displays.iter().take(display_count as usize) {
        displays.push(MacOSDisplay::new(display_id).into());
    }

    Ok(displays)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DisplayState {
    size: Size,
    origin: Origin,
}

#[derive(Default)]
struct EventTracker {
    cached_state: HashMap<MacOSDisplayId, DisplayState>,
}

impl EventTracker {
    fn new() -> Result<Self, MacOSError> {
        Ok(Self {
            cached_state: Self::collect_new_cached_state()?,
        })
    }

    fn collect_new_cached_state() -> Result<HashMap<MacOSDisplayId, DisplayState>, MacOSError> {
        let displays = get_displays()?;
        let mut cached_state = HashMap::new();

        for display in displays.into_iter().map(Into::<MacOSDisplay>::into) {
            cached_state.insert(
                display.id(),
                DisplayState {
                    size: display.size(),
                    origin: display.origin(),
                },
            );
        }

        Ok(cached_state)
    }

    fn add(&mut self, id: MacOSDisplayId) {
        let display = MacOSDisplay::new(id);

        self.cached_state.insert(
            id,
            DisplayState {
                size: display.size(),
                origin: display.origin(),
            },
        );
    }

    fn remove(&mut self, id: MacOSDisplayId) {
        self.cached_state.remove(&id);
    }

    fn track_changes(&mut self) -> Result<SmallVec<[Event; 4]>, MacOSError> {
        let before = std::mem::replace(&mut self.cached_state, Self::collect_new_cached_state()?);
        let mut events = SmallVec::new();

        for (id, before_state) in before.iter() {
            if let Some(after_state) = self.cached_state.get(id) {
                if before_state.size != after_state.size {
                    events.push(Event::SizeChanged {
                        before: before_state.size,
                        after: after_state.size,
                    });
                }

                if before_state.origin != after_state.origin {
                    events.push(Event::OriginChanged {
                        before: before_state.origin,
                        after: after_state.origin,
                    });
                }
            }
        }

        Ok(events)
    }
}

struct UserInfo {
    callback: Option<DisplayEventCallback>,
    tracker: EventTracker,
}

/// A macOS-specific display observer that monitors changes to the display configuration.
///
/// This observer uses `CGDisplayRegisterReconfigurationCallback` to receive notifications
/// about display changes. It also caches display information to track changes
/// like resolution and origin, which are not directly provided by the callback.
pub struct MacOSDisplayObserver {
    user_info: Arc<Mutex<UserInfo>>,
}

impl MacOSDisplayObserver {
    /// Creates a new `MacOSDisplayObserver`.
    ///
    /// This function sets up the necessary Core Graphics callbacks to begin observing
    /// display configuration changes.
    ///
    /// # Errors
    /// Returns a [`MacOSError`] if there is an issue registering the callback
    /// or collecting initial display information.
    pub fn new() -> Result<Self, MacOSError> {
        let user_info = Arc::new(Mutex::new(UserInfo {
            callback: None,
            tracker: EventTracker::new()?,
        }));

        unsafe {
            let user_info = Arc::as_ptr(&user_info) as *mut c_void;
            CGDisplayRegisterReconfigurationCallback(Some(display_callback), user_info)
                .into_result(())?;
        }

        Ok(Self { user_info })
    }

    /// Sets the callback function to be invoked when a display event occurs.
    ///
    /// The provided callback will receive a `MayBeDisplayAvailable` enum,
    /// indicating the nature of the display change and if the display is still available.
    pub fn set_callback(&self, callback: DisplayEventCallback) {
        let mut user_info = self.user_info.lock().unwrap();
        user_info.callback = Some(callback);
    }

    /// Removes the currently set callback function.
    /// After calling this, no display events will be dispatched.
    pub fn remove_callback(&self) {
        let mut user_info = self.user_info.lock().unwrap();
        user_info.callback = None;
    }

    /// Runs the [`NSApplication`][NSApplication] event loop to start handling display events.
    ///
    /// This function will block the current thread and dispatch events.
    ///
    /// # Panics
    /// This function must be called on the main thread, otherwise it will panic.
    ///
    /// [NSApplication]: https://developer.apple.com/documentation/appkit/nsapplication
    pub fn run(&self) {
        let mtm =
            objc2::MainThreadMarker::new().expect("This function must be called on main thread");
        objc2_app_kit::NSApplication::sharedApplication(mtm).run();
    }
}

impl Drop for MacOSDisplayObserver {
    fn drop(&mut self) {
        // TODO: Should I warn if it returns error.
        unsafe {
            let user_info = Arc::as_ptr(&self.user_info) as *mut c_void;
            _ = CGDisplayRemoveReconfigurationCallback(Some(display_callback), user_info)
                .into_result(());
        }
    }
}

unsafe extern "C-unwind" fn display_callback(
    id: CGDirectDisplayID,
    flags: CGDisplayChangeSummaryFlags,
    user_info: *mut c_void,
) {
    if user_info.is_null() {
        return;
    }

    // We only care about the "after" events, so ignore BeginConfiguration.
    if flags.contains(CGDisplayChangeSummaryFlags::BeginConfigurationFlag) {
        return;
    }

    // We don't own the Arc here, just borrowing the pointer.
    // The `MacOSDisplayObserver` keeps the Arc alive.
    // SAFETY: `user_info` is the pointer to the `Arc<Mutex<CallbackState>>` created in new().
    let user_info = unsafe { &*(user_info as *const Mutex<UserInfo>) };
    let Ok(mut user_info) = user_info.lock() else {
        return;
    };

    if user_info.callback.is_some() {
        let mut events: SmallVec<[MayBeDisplayAvailable; 4]> = SmallVec::new();
        let display_available = |event| MayBeDisplayAvailable::Available {
            display: MacOSDisplay::new(id).into(),
            event,
        };

        if flags.contains(CGDisplayChangeSummaryFlags::AddFlag) {
            user_info.tracker.add(id);
            events.push(display_available(Event::Added));
        } else if flags.contains(CGDisplayChangeSummaryFlags::RemoveFlag) {
            user_info.tracker.remove(id);
            events.push(MayBeDisplayAvailable::NotAvailable {
                event: Event::Removed { id: id.into() },
            });
        } else if flags.contains(CGDisplayChangeSummaryFlags::MirrorFlag) {
            events.push(display_available(Event::Mirrored));
        } else if flags.contains(CGDisplayChangeSummaryFlags::UnMirrorFlag) {
            events.push(display_available(Event::UnMirrored));
        } else if flags.contains(CGDisplayChangeSummaryFlags::SetModeFlag)
            || flags.contains(CGDisplayChangeSummaryFlags::MovedFlag)
        {
            if let Ok(tracked_events) = user_info.tracker.track_changes() {
                for event in tracked_events {
                    events.push(display_available(event));
                }
            }
        }

        if events.is_empty() {
            return;
        }

        let callback = user_info.callback.as_mut().unwrap();
        for available in events {
            (callback)(available);
        }
    }
}
