#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
pub use macos::{
    MacOSDisplayId as PlatformDisplayId, MacOSDisplayObserver as PlatformDisplayObserver,
    MacOSError as PlatformError,
};
#[cfg(target_os = "windows")]
pub use windows::{
    WindowsDisplayId as PlatformDisplayId, WindowsDisplayObserver as PlatformDisplayObserver,
    WindowsError as PlatformError,
};

/// A unique identifier for a display.
/// It is used to track displays across different platforms.
///
/// # Platform-specific
/// - **Windows**: The id is a value of [`HMONITOR`][HMONITOR].
/// - **macOS**: The id is a value of [`CGDirectDisplayID`][CGDirectDisplayID].
///
/// [HMONITOR]: https://learn.microsoft.com/en-us/windows/win32/gdi/hmonitor-and-the-device-context
/// [CGDirectDisplayID]: https://developer.apple.com/documentation/coregraphics/cgdirectdisplayid?language=objc
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct DisplayId(PlatformDisplayId);

unsafe impl Send for DisplayId {}
unsafe impl Sync for DisplayId {}

impl From<PlatformDisplayId> for DisplayId {
    fn from(value: PlatformDisplayId) -> Self {
        Self(value)
    }
}

impl DisplayId {
    /// Returns platform representation of the display id.
    ///
    /// # Safety
    /// The display ID returned by this function might become invalid after the display is removed.
    pub fn platform_id(&self) -> &PlatformDisplayId {
        &self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Resolution {
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone)]
pub enum DisplayEvent {
    Added(DisplayId),
    Removed(DisplayId),
    ResolutionChanged {
        id: DisplayId,
        before: Resolution,
        after: Resolution,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Initialization failed.")]
    InitializationError(PlatformError),
    #[error("A platform-specific error has occurred.")]
    PlatformError(PlatformError),
}

pub type DisplayEventCallback = Box<dyn FnMut(DisplayEvent) + Send + 'static>;

pub struct DisplayObserver {
    inner: PlatformDisplayObserver,
}

impl DisplayObserver {
    /// Create the display observer instance.
    ///
    /// # Platform-specific
    /// - **macOS**: This will always return `Ok`.
    pub fn new() -> Result<Self, Error> {
        #[cfg(target_os = "windows")]
        {
            Ok(Self {
                inner: windows::WindowsDisplayObserver::new()?,
            })
        }
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                inner: macos::MacOSDisplayObserver::new(),
            })
        }
    }

    pub fn into_platform_display_observer(self) -> PlatformDisplayObserver {
        self.inner
    }

    pub fn set_callback<F>(&self, callback: F)
    where
        F: FnMut(DisplayEvent) + Send + 'static,
    {
        self.inner.set_callback(Box::new(callback));
    }

    /// Run the event loop.
    /// Since macOS ui thread must be on main, this function must be called on main thread.
    /// If you call this on non-main thread, this will panic.
    ///
    /// # Platform-specific
    /// - **macOS**: This will always return `Ok`.
    pub fn run(&self) -> Result<(), Error> {
        #[cfg(target_os = "windows")]
        {
            self.inner.run()?;
            Ok(())
        }
        #[cfg(target_os = "macos")]
        {
            self.inner.run();
            Ok(())
        }
    }
}
