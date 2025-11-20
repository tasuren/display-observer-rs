use std::{
    collections::HashMap,
    ffi::c_void,
    sync::{Arc, Mutex},
};

use objc2_core_graphics::{
    CGDirectDisplayID, CGDisplayChangeSummaryFlags, CGDisplayRegisterReconfigurationCallback,
    CGDisplayRemoveReconfigurationCallback, CGError,
};

use crate::{DisplayEvent, DisplayEventCallback, DisplayId, Resolution};

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

pub fn get_display_resolutino(display_id: MacOSDisplayId) -> Resolution {
    let width = objc2_core_graphics::CGDisplayPixelsWide(display_id) as u32;
    let height = objc2_core_graphics::CGDisplayPixelsHigh(display_id) as u32;

    Resolution { width, height }
}

struct UserInfo {
    callback: Option<DisplayEventCallback>,
    previous_resolution: HashMap<CGDirectDisplayID, Resolution>,
}

pub struct MacOSDisplayObserver {
    user_info: Arc<Mutex<UserInfo>>,
}

impl MacOSDisplayObserver {
    pub fn new() -> Self {
        let state = Arc::new(Mutex::new(UserInfo {
            callback: None,
            previous_resolution: HashMap::new(),
        }));

        Self { user_info: state }
    }

    pub fn set_callback(&self, callback: DisplayEventCallback) -> Result<(), MacOSError> {
        let mut user_info = self.user_info.lock().unwrap();

        if user_info.callback.is_none() {
            // Register the callback.
            unsafe {
                let user_info = Arc::as_ptr(&self.user_info) as *mut c_void;
                CGDisplayRegisterReconfigurationCallback(Some(display_callback), user_info)
                    .into_result(())?;
            }
        }

        user_info.callback = Some(callback);

        Ok(())
    }

    pub fn remove_callback(&self) -> Result<(), MacOSError> {
        let mut user_info = self.user_info.lock().unwrap();

        unsafe {
            let user_info = Arc::as_ptr(&self.user_info) as *mut c_void;
            CGDisplayRemoveReconfigurationCallback(Some(display_callback), user_info)
                .into_result(())?;
        }

        user_info.callback = None;

        Ok(())
    }

    /// Run the [`NSApplication`][NSApplication] and start handling events.
    ///
    /// # Panics
    /// It will panic on non-main thread.
    ///
    /// [NSApplication]: https://developer.apple.com/documentation/appkit/nsapplication
    pub fn run(&self) {
        let mtm =
            objc2::MainThreadMarker::new().expect("This function must be called on main thread.");
        objc2_app_kit::NSApplication::sharedApplication(mtm).run();
    }
}

impl Drop for MacOSDisplayObserver {
    fn drop(&mut self) {
        // TODO: Should I warn if it returns error.
        let _ = self.remove_callback();
    }
}

unsafe extern "C-unwind" fn display_callback(
    display: CGDirectDisplayID,
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
        let id = DisplayId(display);

        let event = if flags.contains(CGDisplayChangeSummaryFlags::AddFlag) {
            user_info
                .previous_resolution
                .insert(display, get_display_resolutino(display));

            DisplayEvent::Added(id)
        } else if flags.contains(CGDisplayChangeSummaryFlags::RemoveFlag) {
            user_info.previous_resolution.remove(&display);

            DisplayEvent::Removed(id)
        } else if flags.contains(CGDisplayChangeSummaryFlags::SetModeFlag)
            || flags.contains(CGDisplayChangeSummaryFlags::DesktopShapeChangedFlag)
        {
            DisplayEvent::ResolutionChanged {
                id,
                resolution: get_display_resolutino(display),
            }
        } else {
            return;
        };

        (user_info.callback.as_mut().unwrap())(event);
    }
}
