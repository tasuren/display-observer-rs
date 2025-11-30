use dpi::{LogicalPosition, LogicalSize};

#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
use macos::{
    MacOSDisplayId as PlatformDisplayId, MacOSDisplayObserver as PlatformDisplayObserver,
    MacOSError as PlatformError, get_macos_displays as get_platform_displays,
};
#[cfg(target_os = "windows")]
use windows::{
    WindowsDisplayId as PlatformDisplayId, WindowsDisplayObserver as PlatformDisplayObserver,
    WindowsError as PlatformError, get_windows_displays as get_platform_displays,
};

/// The error type for this crate.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// An error occurred during initialization.
    #[error("Initialization failed.")]
    InitializationError(PlatformError),
    /// An error occurred in the platform-specific implementation.
    #[error("A platform-specific error has occurred.")]
    PlatformError(PlatformError),
}

impl From<PlatformError> for Error {
    fn from(value: PlatformError) -> Self {
        Self::PlatformError(value)
    }
}

/// Get all available displays.
///
/// # Returns
/// A list of [`Display`]s.
///
/// # Errors
/// Returns [`Error`] if the platform-specific implementation fails.
pub fn get_displays() -> Result<Vec<Display>, Error> {
    Ok(get_platform_displays()?)
}

/// A unique identifier for a display.
/// It is used to track displays across different platforms.
///
/// # Platform-specific
/// - **Windows**: The id is [device path][device path].
/// - **macOS**: The id is a value of [`CGDirectDisplayID`][CGDirectDisplayID].
///
/// [device path]: https://learn.microsoft.com/en-us/dotnet/standard/io/file-path-formats#dos-device-paths
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
    /// Returns the Windows-specific display id.
    #[cfg(target_os = "windows")]
    pub fn windows_id(&self) -> &PlatformDisplayId {
        &self.0
    }

    /// Returns the macOS-specific display id.
    #[cfg(target_os = "macos")]
    pub fn macos_id(&self) -> &PlatformDisplayId {
        &self.0
    }
}

/// A display.
///
/// This struct provides a cross-platform interface to interact with displays.
/// You can get the display's id, origin, size, and check if it's mirrored.
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    /// The unique identifier of the display.
    pub id: DisplayId,
    /// The origin of the display.
    pub origin: LogicalPosition<i32>,
    /// The size of the display.
    pub size: LogicalSize<u32>,
    /// The scale factor of the display.
    pub scale_factor: f64,
    /// Whether the display is the primary monitor.
    pub is_primary: bool,
    /// Whether the display is mirrored.
    pub is_mirrored: bool,
}

/// An event that occurs when the display configuration changes.
#[derive(Debug, Clone)]
pub enum Event {
    /// A display was added.
    Added(Display),
    /// A display was removed.
    Removed(DisplayId),
    /// The size of a display changed.
    SizeChanged {
        display: Display,
        before: LogicalSize<u32>,
        after: LogicalSize<u32>,
    },
    /// The origin of a display changed.
    OriginChanged {
        display: Display,
        before: LogicalPosition<i32>,
        after: LogicalPosition<i32>,
    },
    /// A display was mirrored.
    Mirrored(Display),
    /// A display was unmirrored.
    UnMirrored(Display),
}

/// A callback function that is called when a display event occurs.
pub type DisplayEventCallback = Box<dyn FnMut(Event) + Send + 'static>;

/// A display observer that monitors changes to the display configuration.
pub struct DisplayObserver {
    inner: PlatformDisplayObserver,
}

impl From<PlatformDisplayObserver> for DisplayObserver {
    fn from(inner: PlatformDisplayObserver) -> Self {
        Self { inner }
    }
}

impl From<DisplayObserver> for PlatformDisplayObserver {
    fn from(value: DisplayObserver) -> Self {
        value.inner
    }
}

impl DisplayObserver {
    /// Create the display observer instance.
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            inner: PlatformDisplayObserver::new()?,
        })
    }

    #[cfg(target_os = "windows")]
    pub fn windows_display_observer(&self) -> &PlatformDisplayObserver {
        &self.inner
    }

    #[cfg(target_os = "macos")]
    pub fn macos_display_observer(&self) -> &PlatformDisplayObserver {
        &self.inner
    }

    /// Sets the callback function to be invoked when a display event occurs.
    pub fn set_callback<F>(&self, callback: F)
    where
        F: FnMut(Event) + Send + 'static,
    {
        self.inner.set_callback(Box::new(callback));
    }

    /// Removes the currently set callback function. After calling this, no display events will be dispatched.
    pub fn remove_callback(&self) {
        self.inner.remove_callback();
    }

    /// Run the event loop.
    /// Since macOS ui thread must be on main, this function must be called on main thread.
    /// If you call this on non-main thread, this will panic.
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
