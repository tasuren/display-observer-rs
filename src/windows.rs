use std::{
    collections::HashMap,
    ffi::{OsStr, OsString, c_void},
    os::windows::ffi::OsStringExt,
    sync::{Arc, Mutex},
};

use dpi::{LogicalPosition, LogicalSize};
use smallvec::SmallVec;
use windows::{
    Win32::{
        Devices::Display::*,
        Foundation::*,
        Graphics::Gdi::*,
        System::LibraryLoader::*,
        UI::{HiDpi::*, WindowsAndMessaging::*},
    },
    core::{BOOL, w},
};

use crate::{Display, DisplayEventCallback, Event};

/// The error type for Windows-specific operations.
/// This is a type alias for [`windows::core::Error`][windows::core::Error].
///
/// [windows::core::Error]: https://docs.rs/windows/latest/windows/core/struct.Error.html
pub type WindowsError = windows::core::Error;

/// Sets the current process as DPI aware (Per Monitor).
///
/// This function calls `SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE)`.
/// It is recommended to call this function at the very beginning of the application
/// to ensure that the display information (especially `scale_factor`) is correctly reported.
///
/// **Important**: This setting cannot be changed once set for a process.
/// If you are integrating this crate with a GUI framework (e.g., Winit, Tauri, or others),
/// it is likely that the framework already handles DPI awareness. Calling this function
/// in such a scenario might conflict with the framework's own DPI management,
/// potentially leading to unexpected behavior or crashes. In most cases, it's best to
/// defer DPI awareness management to your chosen GUI framework or manage it at the
/// application level very early in the process lifecycle.
///
/// # Errors
/// Returns a [`WindowsError`] if `SetProcessDpiAwareness` fails.
pub fn set_process_per_monitor_dpi_aware() -> Result<(), WindowsError> {
    unsafe { SetProcessDpiAwareness(PROCESS_PER_MONITOR_DPI_AWARE) }
}

/// A Windows-specific unique identifier for a display.
///
/// This ID is based on the [device path][device path] of the display.
///
/// [device path]: https://learn.microsoft.com/en-us/dotnet/standard/io/file-path-formats#dos-device-paths
#[derive(Debug, Clone)]
pub struct WindowsDisplayId {
    name: Arc<OsString>,
}

impl std::hash::Hash for WindowsDisplayId {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.name.hash(state)
    }
}

impl PartialEq for WindowsDisplayId {
    fn eq(&self, other: &Self) -> bool {
        self.name.eq(&other.name)
    }
}

impl Eq for WindowsDisplayId {}

impl WindowsDisplayId {
    /// Creates a new `WindowsDisplayId` from a device name string.
    pub fn new(name: OsString) -> Self {
        Self {
            name: Arc::new(name),
        }
    }

    /// Creates a `WindowsDisplayId` from a Windows `HMONITOR` handle.
    ///
    /// # Errors
    /// Returns a [`WindowsError`] if `GetMonitorInfoW` fails.
    pub fn from_handle(handle: HMONITOR) -> Result<Self, WindowsError> {
        let mut monitor_info = MONITORINFOEXW::default();
        monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as _;

        unsafe { GetMonitorInfoW(handle, &raw mut monitor_info as _).ok()? };

        let name_slice = &monitor_info.szDevice;
        let len = name_slice
            .iter()
            .position(|&c| c == 0)
            .unwrap_or(name_slice.len());
        let name = OsString::from_wide(&monitor_info.szDevice[..len]);

        Ok(Self {
            name: Arc::new(name),
        })
    }

    /// Get device identification string. This is also called device path.
    /// e.g. `\\?\DISPLAY1..."
    ///
    /// See [Microsoft's documentation][docs] for more details.
    ///
    /// [docs]: https://learn.microsoft.com/en-us/dotnet/standard/io/file-path-formats#dos-device-paths
    pub fn device_name(&self) -> &OsStr {
        &self.name
    }
}

fn is_display_mirrored(device_name: &OsStr) -> Result<bool, WindowsError> {
    let mut path_count = 0;
    let mut mode_count = 0;

    unsafe {
        GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
            .ok()?;
    }

    let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
    let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];

    unsafe {
        QueryDisplayConfig(
            QDC_ONLY_ACTIVE_PATHS,
            &mut path_count,
            paths.as_mut_ptr(),
            &mut mode_count,
            modes.as_mut_ptr(),
            None,
        )
        .ok()?;
    }

    let mut match_count = 0;
    for path in paths.iter().take(path_count as usize) {
        let mut source_name = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();

        source_name.header.r#type = DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME;
        source_name.header.size = std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32;
        source_name.header.adapterId = path.sourceInfo.adapterId;
        source_name.header.id = path.sourceInfo.id;

        if unsafe { DisplayConfigGetDeviceInfo(&mut source_name.header as *mut _) }
            == ERROR_SUCCESS.0 as i32
        {
            let name_slice = &source_name.viewGdiDeviceName;
            let len = name_slice
                .iter()
                .position(|&c| c == 0)
                .unwrap_or(name_slice.len());
            let name = OsString::from_wide(&name_slice[..len]);

            if name == device_name {
                match_count += 1;
            }
        }
    }

    Ok(match_count > 1)
}

fn get_scale_factor(hdc: HDC, h_monitor: HMONITOR) -> f64 {
    // NOTE: https://learn.microsoft.com/ja-jp/windows/win32/learnwin32/dpi-and-device-independent-pixels#converting-physical-pixels-to-dips
    const USER_DEFAULT_SCREEN_DPI: u32 = 96;

    let mut dpi_x = 0;
    let mut dpi_y = 0;
    let result = unsafe { GetDpiForMonitor(h_monitor, MDT_EFFECTIVE_DPI, &mut dpi_x, &mut dpi_y) };

    if result.is_err() {
        dpi_x = if unsafe { IsProcessDPIAware().as_bool() } {
            unsafe { GetDeviceCaps(Some(hdc), LOGPIXELSX) as _ }
        } else {
            USER_DEFAULT_SCREEN_DPI
        };
    };

    dpi_x as f64 / USER_DEFAULT_SCREEN_DPI as f64
}

struct EnumDisplayMonitorsUserData {
    displays: Vec<Display>,
    result: Result<(), WindowsError>,
}

unsafe extern "system" fn monitor_enum_proc(
    h_monitor: HMONITOR,
    hdc: HDC,
    _rect: *mut RECT,
    user_data: LPARAM,
) -> BOOL {
    let monitors_ptr = user_data.0 as *mut EnumDisplayMonitorsUserData;
    if monitors_ptr.is_null() {
        return false.into();
    }

    let user_data = unsafe { &mut *monitors_ptr };

    // Get full monitor info
    let mut monitor_info = MONITORINFOEXW::default();
    monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as _;

    if let Err(e) = unsafe { GetMonitorInfoW(h_monitor, &raw mut monitor_info as _) }.ok() {
        user_data.result = Err(e);
        return true.into(); // Skip this monitor but continue enumeration
    }

    let name_slice = &monitor_info.szDevice;
    let len = name_slice
        .iter()
        .position(|&c| c == 0)
        .unwrap_or(name_slice.len());
    let device_name = OsString::from_wide(&monitor_info.szDevice[..len]);
    let id = WindowsDisplayId::new(device_name);

    let origin = LogicalPosition::new(
        monitor_info.monitorInfo.rcMonitor.left,
        monitor_info.monitorInfo.rcMonitor.top,
    );
    let size = LogicalSize::new(
        (monitor_info.monitorInfo.rcMonitor.right - monitor_info.monitorInfo.rcMonitor.left) as u32,
        (monitor_info.monitorInfo.rcMonitor.bottom - monitor_info.monitorInfo.rcMonitor.top) as u32,
    );
    let is_primary = (monitor_info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY) != 0;

    let is_mirrored = match is_display_mirrored(id.device_name()) {
        Ok(value) => value,
        Err(e) => {
            user_data.result = Err(e);
            return false.into();
        }
    };
    let scale_factor = get_scale_factor(hdc, h_monitor);

    user_data.displays.push(Display {
        id: id.into(),
        origin,
        size,
        scale_factor,
        is_primary,
        is_mirrored,
    });

    true.into()
}

/// Get a list of all currently active Windows displays.
pub fn get_windows_displays() -> Result<Vec<Display>, WindowsError> {
    let mut user_data: EnumDisplayMonitorsUserData = EnumDisplayMonitorsUserData {
        displays: Vec::new(),
        result: Ok(()),
    };

    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_proc),
            LPARAM(&raw mut user_data as isize),
        )
        .ok()?;
    };

    user_data.result.map(|_| user_data.displays)
}

struct EventTracker {
    cached_displays: HashMap<WindowsDisplayId, Display>,
}

impl EventTracker {
    fn new() -> Result<Self, WindowsError> {
        let mut tracker = Self {
            cached_displays: HashMap::new(),
        };
        tracker.cached_displays = tracker.collect_new_cached_state()?;

        Ok(tracker)
    }

    fn collect_new_cached_state(&self) -> Result<HashMap<WindowsDisplayId, Display>, WindowsError> {
        let displays = get_windows_displays()?;
        let mut cached_state = HashMap::new();

        for display in displays {
            let win_id = display.id.windows_id();
            cached_state.insert(win_id.clone(), display);
        }

        Ok(cached_state)
    }

    fn track_events(&mut self) -> Result<SmallVec<[Event; 10]>, WindowsError> {
        let new_cached_state = self.collect_new_cached_state()?;
        let before = std::mem::replace(&mut self.cached_displays, new_cached_state);
        let mut events = SmallVec::new();

        for (id, before_display) in before.iter() {
            if let Some(after_display) = self.cached_displays.get(id) {
                if before_display.size != after_display.size {
                    events.push(Event::SizeChanged {
                        display: (*after_display).clone(),
                        before: before_display.size,
                        after: after_display.size,
                    });
                };

                if before_display.origin != after_display.origin {
                    events.push(Event::OriginChanged {
                        display: (*after_display).clone(),
                        before: before_display.origin,
                        after: after_display.origin,
                    });
                }

                if before_display.is_mirrored != after_display.is_mirrored {
                    let event = if after_display.is_mirrored {
                        Event::Mirrored((*after_display).clone())
                    } else {
                        Event::UnMirrored((*after_display).clone())
                    };

                    events.push(event);
                }
            } else {
                events.push(Event::Removed(id.clone().into()));
            }
        }

        for (id, after_display) in &self.cached_displays {
            if !before.contains_key(id) {
                events.push(Event::Added((*after_display).clone()));
            }
        }

        Ok(events)
    }
}

struct ObserverContext {
    callback: Option<DisplayEventCallback>,
    tracker: EventTracker,
}

/// A Windows-specific display observer that monitors changes to the display configuration.
///
/// This observer creates a hidden window to receive `WM_DISPLAYCHANGE` messages
/// and uses device notification APIs to track display events.
pub struct WindowsDisplayObserver {
    hwnd: HWND,
    h_notify: HDEVNOTIFY,
    ctx: Arc<Mutex<ObserverContext>>,
}

impl WindowsDisplayObserver {
    /// Creates a new `WindowsDisplayObserver`.
    ///
    /// This function sets up a hidden window and registers for device notifications
    /// to begin observing display configuration changes.
    ///
    /// # Errors
    /// Returns a [`WindowsError`] if there is an issue creating the window,
    /// registering for notifications, or collecting initial display information.
    pub fn new() -> Result<Self, WindowsError> {
        let h_instance = unsafe { GetModuleHandleW(None)? };
        let window_class_name = w!("DisplayMonitorClass");
        let window_class = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: h_instance.into(),
            lpszClassName: window_class_name,
            ..Default::default()
        };

        unsafe {
            RegisterClassW(&window_class);
        }

        let ctx = Arc::new(Mutex::new(ObserverContext {
            callback: None,
            tracker: EventTracker::new()?,
        }));
        let state_ptr = Arc::as_ptr(&ctx) as *mut c_void;

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                window_class_name,
                w!("DisplayMonitorWindow"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                0,
                0,
                None,
                None,
                Some(h_instance.into()),
                Some(state_ptr),
            )?
        };

        let mut filter = DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: std::mem::size_of::<DEV_BROADCAST_DEVICEINTERFACE_W>() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE.0,
            dbcc_classguid: GUID_DEVINTERFACE_MONITOR,
            ..Default::default()
        };

        let h_notify = unsafe {
            RegisterDeviceNotificationW(
                hwnd.into(),
                &mut filter as *mut _ as *const c_void,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )?
        };

        // Store the state pointer in the window user data so WndProc can access it.
        // NOTE: We passed it in CreateWindowExW, but we also set it here to be sure or if we missed WM_CREATE handling.
        unsafe {
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        }

        Ok(Self {
            hwnd,
            h_notify,
            ctx,
        })
    }

    /// Sets the callback function to be invoked when a display event occurs.
    ///
    /// The provided callback will receive a [`Event`] enum,
    /// indicating the nature of the display change.
    pub fn set_callback(&self, callback: DisplayEventCallback) {
        let mut state = self.ctx.lock().unwrap();
        state.callback = Some(callback);
    }

    /// Removes the currently set callback function.
    /// After calling this, no display events will be dispatched.
    pub fn remove_callback(&self) {
        let mut state = self.ctx.lock().unwrap();
        state.callback = None;
    }

    /// Runs the Windows message loop to start handling display events.
    ///
    /// This function will block the current thread and dispatch messages.
    ///
    /// # Errors
    /// Returns a [`WindowsError`] if `GetMessageW` fails.
    pub fn run(&self) -> Result<(), WindowsError> {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }

        Ok(())
    }
}

impl Drop for WindowsDisplayObserver {
    fn drop(&mut self) {
        unsafe {
            if !self.h_notify.is_invalid() {
                _ = UnregisterDeviceNotification(self.h_notify);
            }
            _ = DestroyWindow(self.hwnd);
        }
    }
}

#[inline]
fn process_window_message(
    msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
    ctx: &mut ObserverContext,
) -> Result<Option<SmallVec<[Event; 10]>>, WindowsError> {
    Ok(match msg {
        WM_DISPLAYCHANGE => Some(ctx.tracker.track_events()?),
        _ => None,
    })
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    let default_window_proc = || unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) };

    let ctx = unsafe {
        let user_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
        let user_data_ptr = user_data as *const Mutex<ObserverContext>;

        if user_data_ptr.is_null() {
            return default_window_proc();
        }

        &*(user_data_ptr)
    };

    if let Ok(mut ctx) = ctx.lock()
        && let Ok(Some(events)) = process_window_message(msg, wparam, lparam, &mut ctx)
        && let Some(callback) = ctx.callback.as_mut()
    {
        for event in events {
            (callback)(event);
        }
    }

    default_window_proc()
}
