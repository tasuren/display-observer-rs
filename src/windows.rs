// TODO: AI generated codes. We should refactor this.

use crate::{DisplayEvent, DisplayEventCallback, DisplayId};
use std::ffi::c_void;
use std::sync::{Arc, Mutex};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::Graphics::Gdi::{GetDC, GetDeviceCaps, HORZRES, VERTRES};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DBT_DEVICEARRIVAL, DBT_DEVICEREMOVECOMPLETE,
    DBT_DEVTYP_DEVICEINTERFACE, DEVICE_NOTIFY_WINDOW_HANDLE, DefWindowProcW, DispatchMessageW,
    GWLP_USERDATA, GetMessageW, HDEVNOTIFY, MSG, RegisterClassW, RegisterDeviceNotificationW,
    TranslateMessage, WINDOW_EX_STYLE, WM_DEVICECHANGE, WM_DISPLAYCHANGE, WNDCLASSW,
    WS_OVERLAPPEDWINDOW,
};
use windows::core::{PCWSTR, w};

struct CallbackState {
    callback: Option<DisplayEventCallback>,
}

pub struct MonitorInner {
    hwnd: HWND,
    hnotify: HDEVNOTIFY,
    state: Arc<Mutex<CallbackState>>,
}

impl MonitorInner {
    pub fn new() -> Result<Self, anyhow::Error> {
        let instance = unsafe { GetModuleHandleW(None)? };
        let class_name = w!("DisplayMonitorClass");

        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(wnd_proc),
            hInstance: instance.into(),
            lpszClassName: class_name,
            ..Default::default()
        };

        unsafe {
            RegisterClassW(&wc);
        }

        let state = Arc::new(Mutex::new(CallbackState { callback: None }));
        let state_ptr = Arc::as_ptr(&state) as *mut c_void;

        let hwnd = unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE::default(),
                class_name,
                w!("DisplayMonitorWindow"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                0,
                0,
                None,
                None,
                instance,
                Some(state_ptr),
            )
        };

        if hwnd.0 == std::ptr::null_mut() {
            return Err(anyhow::anyhow!("Failed to create window"));
        }

        // Register for device notifications
        // We need to register for GUID_DEVINTERFACE_MONITOR
        // GUID_DEVINTERFACE_MONITOR: {E6F07B5F-EE97-4a90-B076-33F57BF4EAA7}
        let interface_class_guid =
            windows::core::GUID::from_u128(0xE6F07B5F_EE97_4a90_B076_33F57BF4EAA7);

        let mut filter = windows::Win32::UI::WindowsAndMessaging::DEV_BROADCAST_DEVICEINTERFACE_W {
            dbcc_size: std::mem::size_of::<
                windows::Win32::UI::WindowsAndMessaging::DEV_BROADCAST_DEVICEINTERFACE_W,
            >() as u32,
            dbcc_devicetype: DBT_DEVTYP_DEVICEINTERFACE,
            dbcc_classguid: interface_class_guid,
            ..Default::default()
        };

        let hnotify = unsafe {
            RegisterDeviceNotificationW(
                hwnd,
                &mut filter as *mut _ as *const c_void,
                DEVICE_NOTIFY_WINDOW_HANDLE,
            )
        }?;

        // Store the state pointer in the window user data so WndProc can access it
        // Note: We passed it in CreateWindowExW, but we also set it here to be sure or if we missed WM_CREATE handling
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::SetWindowLongPtrW;
            SetWindowLongPtrW(hwnd, GWLP_USERDATA, state_ptr as isize);
        }

        Ok(Self {
            hwnd,
            hnotify,
            state,
        })
    }

    pub fn set_callback(&mut self, callback: DisplayEventCallback) {
        let mut state = self.state.lock().unwrap();
        state.callback = Some(callback);
    }

    pub fn run(&self) -> Result<(), anyhow::Error> {
        unsafe {
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).into() {
                TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        Ok(())
    }
}

impl Drop for MonitorInner {
    fn drop(&mut self) {
        unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{
                DestroyWindow, UnregisterDeviceNotification,
            };
            if !self.hnotify.is_invalid() {
                let _ = UnregisterDeviceNotification(self.hnotify);
            }
            let _ = DestroyWindow(self.hwnd);
        }
    }
}

unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW;

    let user_data = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
    if user_data == 0 {
        return DefWindowProcW(hwnd, msg, wparam, lparam);
    }

    let state_ptr = user_data as *const Mutex<CallbackState>;
    let state = &*state_ptr;

    match msg {
        WM_DISPLAYCHANGE => {
            if let Ok(mut guard) = state.lock() {
                if let Some(cb) = &mut guard.callback {
                    // Resolution changed
                    let width = (lparam.0 & 0xFFFF) as u32;
                    let height = ((lparam.0 >> 16) & 0xFFFF) as u32;
                    // For DisplayId on Windows, it's complex to map exactly to a specific monitor from just this message
                    // without enumerating. For now, we'll use a dummy ID or try to find the primary.
                    // The user asked for "tracking connection/disconnection and resolution".
                    // WM_DISPLAYCHANGE is global.
                    // We'll use 0 as a generic ID for "primary/desktop" or try to improve this later.
                    cb(DisplayEvent::ResolutionChanged(DisplayId(0), width, height));
                }
            }
        }
        WM_DEVICECHANGE => {
            if let Ok(mut guard) = state.lock() {
                if let Some(cb) = &mut guard.callback {
                    match wparam.0 as u32 {
                        DBT_DEVICEARRIVAL => {
                            // A device arrived. We should check if it's a monitor.
                            // lparam points to DEV_BROADCAST_HDR.
                            // We filtered for monitors, so it likely is.
                            // We can parse lparam to get the ID.
                            cb(DisplayEvent::Connected(DisplayId(0))); // Placeholder ID
                        }
                        DBT_DEVICEREMOVECOMPLETE => {
                            cb(DisplayEvent::Disconnected(DisplayId(0))); // Placeholder ID
                        }
                        _ => {}
                    }
                }
            }
        }
        _ => {}
    }

    DefWindowProcW(hwnd, msg, wparam, lparam)
}
