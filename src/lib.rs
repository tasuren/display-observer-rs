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

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Initialization failed.")]
    InitializationError(PlatformError),
    #[error("A platform-specific error has occurred.")]
    PlatformError(PlatformError),
}

impl From<PlatformError> for Error {
    fn from(value: PlatformError) -> Self {
        Self::PlatformError(value)
    }
}

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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Origin {
    pub x: u32,
    pub y: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

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
    #[cfg(target_os = "windows")]
    pub fn windows_display(&self) -> &windows::WindowsDisplay {
        &self.0
    }

    #[cfg(target_os = "macos")]
    pub fn macos_display(&self) -> &macos::MacOSDisplay {
        &self.0
    }

    pub fn id(&self) -> DisplayId {
        self.0.id().into()
    }

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

    pub fn is_mirrored(&self) -> bool {
        #[cfg(target_os = "windows")]
        {
            self.0.is_mirrored()
        }
        #[cfg(target_os = "macos")]
        {
            self.0.is_mirrored()
        }
    }
}

#[derive(Debug, Clone)]
pub enum Event {
    Added,
    Removed { id: DisplayId },
    SizeChanged { before: Size, after: Size },
    Mirrored,
    UnMirrored,
}

#[derive(Clone)]
pub enum MayBeDisplayAvailable {
    Available { display: Display, event: Event },
    NotAvailable { event: Event },
}

pub type DisplayEventCallback = Box<dyn FnMut(MayBeDisplayAvailable) + Send + 'static>;

pub struct DisplayObserver {
    inner: PlatformDisplayObserver,
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
