#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "windows")]
pub mod windows;

#[cfg(target_os = "macos")]
use macos::{
    MacOSDisplay as PlatformDisplay, MacOSDisplayId as PlatformDisplayId,
    MacOSDisplayObserver as PlatformDisplayObserver, MacOSError as PlatformError,
    get_displays as get_platform_displays,
};
#[cfg(target_os = "windows")]
use windows::{
    WindowsDisplay as PlatformDisplay, WindowsDisplayId as PlatformDisplayId,
    WindowsDisplayObserver as PlatformDisplayObserver, WindowsError as PlatformError,
    get_displays as get_platform_displays,
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
    /// Returns platform representation of the display id.
    ///
    /// # Safety
    /// The display ID returned by this function might become invalid after the display is removed.
    pub fn platform_id(&self) -> &PlatformDisplayId {
        &self.0
    }
}

/// A point on the screen.
/// The origin is the top-left corner of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    /// The x coordinate.
    pub x: u32,
    /// The y coordinate.
    pub y: u32,
}

/// The size of the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    /// The width of the screen.
    pub width: u32,
    /// The height of the screen.
    pub height: u32,
}

/// A display.
///
/// This struct provides a cross-platform interface to interact with displays.
/// You can get the display's id, origin, size, and check if it's mirrored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Display(PlatformDisplay);

impl From<PlatformDisplay> for Display {
    fn from(value: PlatformDisplay) -> Self {
        Self(value)
    }
}

impl From<Display> for PlatformDisplay {
    fn from(value: Display) -> Self {
        value.0
    }
}

impl Display {
    /// Get the Windows specific display implementation.
    #[cfg(target_os = "windows")]
    pub fn windows_display(&self) -> &windows::WindowsDisplay {
        &self.0
    }

    /// Get the macOS specific display implementation.
    #[cfg(target_os = "macos")]
    pub fn macos_display(&self) -> &macos::MacOSDisplay {
        &self.0
    }

    /// Get the unique identifier of the display.
    pub fn id(&self) -> DisplayId {
        self.0.id().into()
    }

    /// Get the origin of the display.
    pub fn origin(&self) -> Result<Origin, Error> {
        #[cfg(target_os = "windows")]
        {
            Ok(self.0.origin()?)
        }
        #[cfg(target_os = "macos")]
        {
            Ok(self.0.origin())
        }
    }

    /// Get the size of the display.
    pub fn size(&self) -> Result<Size, Error> {
        #[cfg(target_os = "windows")]
        {
            Ok(self.0.size()?)
        }
        #[cfg(target_os = "macos")]
        {
            Ok(self.0.size())
        }
    }

    /// Check if the display is mirrored.
    pub fn is_mirrored(&self) -> Result<bool, Error> {
        #[cfg(target_os = "windows")]
        {
            Ok(self.0.is_mirrored()?)
        }
        #[cfg(target_os = "macos")]
        {
            Ok(self.0.is_mirrored())
        }
    }

    /// Check if this display is the primary monitor.
    pub fn is_primary(&self) -> Result<bool, Error> {
        #[cfg(target_os = "windows")]
        {
            Ok(self.0.is_primary()?)
        }
        #[cfg(target_os = "macos")]
        {
            Ok(self.0.is_primary())
        }
    }
}

/// An event that occurs when the display configuration changes.
#[derive(Debug, Clone)]
pub enum Event {
    /// A display was added.
    Added,
    /// A display was removed.
    Removed { id: DisplayId },
    /// The size of a display changed.
    SizeChanged { before: Size, after: Size },
    /// The origin of a display changed.
    OriginChanged { before: Origin, after: Origin },
    /// A display was mirrored.
    Mirrored,
    /// A display was unmirrored.
    UnMirrored,
}

/// A wrapper around [`Event`] that provides the display if it is still available.
#[derive(Clone)]
pub enum MayBeDisplayAvailable {
    /// The display is available.
    Available { display: Display, event: Event },
    /// The display is not available (e.g. it was removed).
    NotAvailable { event: Event },
}

/// A callback function that is called when a display event occurs.
pub type DisplayEventCallback = Box<dyn FnMut(MayBeDisplayAvailable) + Send + 'static>;

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
    ///
    /// # Platform-specific
    /// - **macOS**: This will always return `Ok`.
    pub fn new() -> Result<Self, Error> {
        Ok(Self {
            inner: PlatformDisplayObserver::new()?,
        })
    }

    pub fn into_platform_display_observer(self) -> PlatformDisplayObserver {
        self.inner
    }

    pub fn set_callback<F>(&self, callback: F)
    where
        F: FnMut(MayBeDisplayAvailable) + Send + 'static,
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
