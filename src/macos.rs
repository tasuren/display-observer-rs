use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{Arc, Mutex},
};

use objc2_core_foundation::{CGPoint, CGSize};
use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayBounds, CGDisplayChangeSummaryFlags, CGDisplayIsMain,
    CGDisplayMirrorsDisplay, CGDisplayRegisterReconfigurationCallback,
    CGDisplayRemoveReconfigurationCallback, CGDisplayScreenSize, CGError, CGGetActiveDisplayList,
    kCGNullDirectDisplay,
};

use crate::{Display, DisplayEventCallback, Event, MayBeDisplayAvailable, Origin, Size};

pub type MacOSDisplayId = CGDirectDisplayID;

#[derive(Debug, thiserror::Error)]
pub enum MacOSError {
    #[error("Failed to load `NSApplication`.")]
    NSApplicationLoadError,
    #[error("`CGError` has occurred: {0:?}")]
    CGError(CGError),
}

impl From<MacOSError> for crate::Error {
    fn from(value: MacOSError) -> Self {
        Self::PlatformError(value)
    }
}

trait CGErrorToResult {
    fn into_result<T>(self, value: T) -> Result<T, MacOSError>;
}

impl CGErrorToResult for CGError {
    fn into_result<T>(self, value: T) -> Result<T, MacOSError> {
        if self == CGError::Success {
            Ok(value)
        } else {
            Err(MacOSError::CGError(self))
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacOSDisplay {
    pub(crate) id: MacOSDisplayId,
}

impl MacOSDisplay {
    pub fn new(id: MacOSDisplayId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> MacOSDisplayId {
        self.id
    }

    pub fn origin(&self) -> Origin {
        CGDisplayBounds(self.id).origin.into()
    }

    pub fn size(&self) -> Size {
        CGDisplayBounds(self.id).size.into()
    }

    pub fn is_primary(&self) -> bool {
        CGDisplayIsMain(self.id)
    }

    pub fn is_mirrored(&self) -> bool {
        CGDisplayMirrorsDisplay(self.id) != kCGNullDirectDisplay
    }
}

pub fn get_displays() -> Result<Vec<MacOSDisplay>, MacOSError> {
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
    for display_id in active_displays {
        displays.push(MacOSDisplay::new(display_id));
    }

    Ok(displays)
}

#[derive(Default)]
struct EventTracker {
    cached_size: HashMap<MacOSDisplayId, Size>,
}

impl EventTracker {
    fn new() -> Result<Self, MacOSError> {
        let displays = get_displays()?;
        let mut cached_size = HashMap::new();

        for display in displays {
            cached_size.insert(display.id(), display.size());
        }

        Ok(Self { cached_size })
    }

    fn add(&mut self, id: MacOSDisplayId) {
        let resolution = CGDisplayScreenSize(id).into();
        self.cached_size.insert(id, resolution);
    }

    fn remove(&mut self, id: MacOSDisplayId) {
        self.cached_size.remove(&id);
    }

    fn track_resolution_changed(&mut self) -> Result<Option<Event>, MacOSError> {
        let displays = get_displays()?;
        let mut new_cached_size = HashMap::new();

        for display in displays {
            new_cached_size.insert(display.id(), display.size());
        }

        let before = std::mem::replace(&mut self.cached_size, new_cached_size);

        for (id, before) in before.iter() {
            if let Some(after) = self.cached_size.get(id)
                && before != after
            {
                return Ok(Some(Event::SizeChanged {
                    before: *before,
                    after: *after,
                }));
            }
        }

        Ok(None)
    }
}

struct UserInfo {
    callback: Option<DisplayEventCallback>,
    tracker: EventTracker,
}

pub struct MacOSDisplayObserver {
    user_info: Arc<Mutex<UserInfo>>,
}

impl MacOSDisplayObserver {
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

    pub fn set_callback(&self, callback: DisplayEventCallback) {
        let mut user_info = self.user_info.lock().unwrap();
        user_info.callback = Some(callback);
    }

    pub fn remove_callback(&self) {
        let mut user_info = self.user_info.lock().unwrap();
        user_info.callback = None;
    }

    /// Run the [`NSApplication`][NSApplication] and start handling events.
    ///
    /// # Panics
    /// It will panic on non-main thread.
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
            let _ = CGDisplayRemoveReconfigurationCallback(Some(display_callback), user_info)
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
        let event = if flags.contains(CGDisplayChangeSummaryFlags::AddFlag) {
            user_info.tracker.add(id);
            Event::Added
        } else if flags.contains(CGDisplayChangeSummaryFlags::RemoveFlag) {
            user_info.tracker.remove(id);
            Event::Removed { id: id.into() }
        } else if flags.contains(CGDisplayChangeSummaryFlags::MirrorFlag) {
            Event::Mirrored
        } else if flags.contains(CGDisplayChangeSummaryFlags::UnMirrorFlag) {
            Event::UnMirrored
        } else if flags.contains(CGDisplayChangeSummaryFlags::SetModeFlag) {
            if let Ok(Some(event)) = user_info.tracker.track_resolution_changed() {
                event
            } else {
                return;
            }
        } else {
            return;
        };

        let event = if matches!(event, Event::Removed { .. }) {
            MayBeDisplayAvailable::NotAvailable { event }
        } else {
            MayBeDisplayAvailable::Available {
                display: Display::new(MacOSDisplay::new(id)),
                event,
            }
        };

        (user_info.callback.as_mut().unwrap())(event);
    }
}
