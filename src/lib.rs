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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DisplayId(pub PlatformDisplayId);

impl DisplayId {
    pub fn as_u64(&self) -> u64 {
        self.0 as _
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
        resolution: Resolution,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum Error {
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
                inner: windows::MonitorInner::new()?,
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

    pub fn set_callback<F>(&self, callback: F) -> Result<(), Error>
    where
        F: FnMut(DisplayEvent) + Send + 'static,
    {
        self.inner.set_callback(Box::new(callback))?;

        Ok(())
    }

    /// Run the event loop.
    ///
    /// # Platform-specific
    /// - **macOS**: This function must be called on main thread.
    ///     And this will always return `Ok`.
    pub fn run(&self) -> Result<(), Error> {
        #[cfg(target_os = "windows")]
        return self.inner.run();

        #[cfg(target_os = "macos")]
        {
            self.inner.run();
            Ok(())
        }
    }
}
