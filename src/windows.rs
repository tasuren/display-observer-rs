use std::{
    collections::HashMap,
    ffi::{OsString, c_void},
    os::windows::ffi::OsStringExt,
    sync::{Arc, Mutex},
};

use smallvec::SmallVec;
use windows::{
    Win32::{
        Devices::Display::*, Foundation::*, Graphics::Gdi::*, System::LibraryLoader::*,
        UI::WindowsAndMessaging::*,
    },
    core::{BOOL, w},
};

use crate::{DisplayEvent, DisplayEventCallback, Size};

pub type WindowsError = windows::core::Error;

impl From<WindowsError> for crate::Error {
    fn from(value: WindowsError) -> Self {
        Self::PlatformError(value)
    }
}

#[derive(Debug, Clone)]
pub struct WindowsDisplayId {
    name: OsString,
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
    pub fn new(name: OsString) -> Self {
        Self { name }
    }

    pub fn from_handle(handle: HMONITOR) -> Result<Self, WindowsError> {
        let mut monitor_info = MONITORINFOEXW::default();
        monitor_info.monitorInfo.cbSize = std::mem::size_of::<MONITORINFOEXW>() as _;

        unsafe { GetMonitorInfoW(handle, &raw mut monitor_info as _).ok()? };
        let name = OsString::from_wide(&monitor_info.szDevice);

        Ok(Self { name })
    }

    /// Get device identification string. This is also called device path.
    /// e.g. `\\?\DISPLAY1..."`
    ///
    /// See [Microsoft's documentation][docs] for more details.
    ///
    /// [docs]: https://learn.microsoft.com/en-us/dotnet/standard/io/file-path-formats#dos-device-paths
    pub fn name(&self) -> &OsString {
        &self.name
    }
}

impl From<RECT> for Size {
    fn from(value: RECT) -> Self {
        Self {
            width: value.right as _,
            height: value.bottom as _,
        }
    }
}

unsafe extern "system" fn monitor_enum_proc(
    h_monitor: HMONITOR,
    _hdc: HDC,
    rect: *mut RECT,
    user_data: LPARAM,
) -> BOOL {
    let monitors_ptr = user_data.0 as *mut HashMap<WindowsDisplayId, Size>;
    if monitors_ptr.is_null() || rect.is_null() {
        return false.into();
    }

    let monitors = unsafe { &mut *monitors_ptr };
    let rect = unsafe { *rect };

    if let Ok(id) = WindowsDisplayId::from_handle(h_monitor) {
        monitors.insert(id, Size::from(rect));
    }

    true.into()
}

fn get_monitors() -> Result<HashMap<WindowsDisplayId, Size>, WindowsError> {
    let mut monitors = HashMap::new();

    unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(monitor_enum_proc),
            LPARAM(&raw mut monitors as isize),
        )
        .ok()?;
    };

    Ok(monitors)
}

struct EventTracker(HashMap<WindowsDisplayId, Size>);

impl EventTracker {
    fn new() -> Result<Self, WindowsError> {
        Ok(Self(get_monitors()?))
    }

    fn track_events(&mut self) -> Result<SmallVec<[DisplayEvent; 10]>, WindowsError> {
        let before = std::mem::replace(&mut self.0, get_monitors()?);
        let mut events = SmallVec::new();

        for (id, before) in before.iter() {
            if let Some(after) = self.0.get(id)
                && before != after
            {
                events.push(DisplayEvent::SizeChanged {
                    id: id.clone().into(),
                    before: *before,
                    after: *after,
                });
            };

            if !self.0.contains_key(id) {
                events.push(DisplayEvent::Removed {
                    id: id.clone().into(),
                });
            }
        }

        for (id, resolution) in self.0.iter() {
            if !before.contains_key(id) {
                events.push(DisplayEvent::Added {
                    id: id.clone().into(),
                    resolution: *resolution,
                });
            }
        }

        Ok(events)
    }
}

struct ObserverContext {
    callback: Option<DisplayEventCallback>,
    tracker: EventTracker,
}

pub struct WindowsDisplayObserver {
    hwnd: HWND,
    h_notify: HDEVNOTIFY,
    ctx: Arc<Mutex<ObserverContext>>,
}

impl WindowsDisplayObserver {
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

    pub fn set_callback(&self, callback: DisplayEventCallback) {
        let mut state = self.ctx.lock().unwrap();
        state.callback = Some(callback);
    }

    pub fn remove_callback(&self) {
        let mut state = self.ctx.lock().unwrap();
        state.callback = None;
    }

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
            use windows::Win32::UI::WindowsAndMessaging::{
                DestroyWindow, UnregisterDeviceNotification,
            };
            if !self.h_notify.is_invalid() {
                let _ = UnregisterDeviceNotification(self.h_notify);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

#[inline]
fn process_window_message(
    msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
    ctx: &mut ObserverContext,
) -> Result<Option<SmallVec<[DisplayEvent; 10]>>, WindowsError> {
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
