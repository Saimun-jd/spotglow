use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

use windows::core::{BOOL, PWSTR};
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::Win32::System::StationsAndDesktops::{
    OpenDesktopW, OpenWindowStationW, SetProcessWindowStation, SetThreadDesktop, DESKTOP_CONTROL_FLAGS,
};
use windows::Win32::System::Threading::{
    OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::Accessibility::{SetWinEventHook, UnhookWinEvent, HWINEVENTHOOK};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, EnumWindows, GetClassNameW, GetForegroundWindow, GetWindowRect, GetWindowTextW,
    GetWindowThreadProcessId, IsIconic, IsWindow, IsWindowVisible, IsZoomed,
    PeekMessageW, PostThreadMessageW, TranslateMessage, EVENT_OBJECT_DESTROY,
    EVENT_OBJECT_LOCATIONCHANGE, EVENT_SYSTEM_FOREGROUND, EVENT_SYSTEM_MINIMIZEEND,
    EVENT_SYSTEM_MINIMIZESTART, MSG, OBJID_WINDOW, PM_REMOVE, WINEVENT_OUTOFCONTEXT,
    WINEVENT_SKIPOWNPROCESS, WM_QUIT, WM_USER,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct WindowRect {
    pub left: i32,
    pub top: i32,
    pub right: i32,
    pub bottom: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct SpotifyWindowState {
    pub hwnd: isize,
    pub rect: WindowRect,
    pub visible: bool,
    pub minimized: bool,
    pub maximized: bool,
    pub dpi: u32,
    pub is_foreground: bool,
}

pub type WindowEventCallback = Arc<dyn Fn(SpotifyWindowState) + Send + Sync>;

// Global state shared with the WinEvent callback
static mut ACTIVE_SPOTIFY_HWND: isize = 0;
static mut TRACKER_THREAD_ID: u32 = 0;

/// Attaches the calling thread to the interactive "Default" desktop on "WinSta0".
pub fn attach_to_default_desktop() {
    unsafe {
        let winsta_name = windows::core::w!("WinSta0");
        if let Ok(hwinsta) = OpenWindowStationW(winsta_name, false, 0x037F) {
            let _ = SetProcessWindowStation(hwinsta);
        }
        let default_name = windows::core::w!("Default");
        let access = 0x01FF; // DESKTOP_ALL
        if let Ok(hdesk) = OpenDesktopW(default_name, DESKTOP_CONTROL_FLAGS(0), false, access) {
            let _ = SetThreadDesktop(hdesk);
        }
    }
}

pub struct WindowTracker {
    on_event: Option<WindowEventCallback>,
    is_running: Arc<AtomicBool>,
}

impl WindowTracker {
    pub fn new(on_event: Option<WindowEventCallback>) -> Self {
        Self {
            on_event,
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Spawns the window tracking thread with high-frequency check and WinEvent hooks.
    pub fn start(&self) {
        if self.is_running.swap(true, Ordering::SeqCst) {
            warn!("[WindowTracker] Tracker is already running.");
            return;
        }

        let on_event = self.on_event.clone();
        let is_running = self.is_running.clone();

        std::thread::spawn(move || {
            info!("[WindowTracker] Starting Spotify window tracking thread...");

            // Ensure attached to interactive "Default" desktop before creating message queues or hooks
            attach_to_default_desktop();

            unsafe {
                TRACKER_THREAD_ID = windows::Win32::System::Threading::GetCurrentThreadId();
            }

            // Install WinEvent hook for window events
            let hook = unsafe {
                SetWinEventHook(
                    EVENT_SYSTEM_FOREGROUND,
                    EVENT_OBJECT_LOCATIONCHANGE,
                    None,
                    Some(win_event_proc),
                    0,
                    0,
                    WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
                )
            };

            if hook.is_invalid() {
                warn!("[WindowTracker] Failed to install SetWinEventHook.");
            } else {
                info!("[WindowTracker] WinEvent hook installed successfully.");
            }

            let mut last_state = SpotifyWindowState::default();
            let mut current_hwnd: Option<HWND> = None;

            // Initial check to find Spotify if already running
            if let Some(hwnd) = find_spotify_window() {
                current_hwnd = Some(hwnd);
                unsafe { ACTIVE_SPOTIFY_HWND = hwnd.0 as isize; }
                if let Some(state) = get_window_state(hwnd) {
                    info!(
                        "[WindowTracker] Spotify found: HWND=0x{:x} | Rect: {}x{} at ({},{}) | Min: {} | Max: {} | Vis: {} | DPI: {}",
                        state.hwnd, state.rect.width, state.rect.height, state.rect.left, state.rect.top,
                        state.minimized, state.maximized, state.visible, state.dpi
                    );
                    if let Some(cb) = &on_event {
                        cb(state.clone());
                    }
                    last_state = state;
                }
            } else {
                info!("[WindowTracker] Spotify not currently running. Tracking...");
            }

            // 60 FPS non-blocking polling loop + Win32 message pump
            while is_running.load(Ordering::SeqCst) {
                // 1. Process any pending Win32 messages (WinEvent wakeups, quits)
                let mut msg = MSG::default();
                while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE).as_bool() } {
                    if msg.message == WM_QUIT {
                        break;
                    }
                    unsafe {
                        let _ = TranslateMessage(&msg);
                        DispatchMessageW(&msg);
                    }
                }

                // 2. Verify current HWND or re-search for Spotify
                let spotify_valid = current_hwnd.map_or(false, |h| unsafe { IsWindow(Some(h)).as_bool() });

                if !spotify_valid {
                    if let Some(hwnd) = find_spotify_window() {
                        current_hwnd = Some(hwnd);
                        unsafe { ACTIVE_SPOTIFY_HWND = hwnd.0 as isize; }
                        info!("[WindowTracker] Spotify attached/re-detected: HWND=0x{:x}", hwnd.0 as isize);
                    } else if current_hwnd.is_some() {
                        info!("[WindowTracker] Spotify closed or exited.");
                        current_hwnd = None;
                        unsafe { ACTIVE_SPOTIFY_HWND = 0; }
                        let empty_state = SpotifyWindowState::default();
                        if let Some(cb) = &on_event {
                            cb(empty_state.clone());
                        }
                        last_state = empty_state;
                    }
                }

                // 3. Query Spotify window state
                if let Some(hwnd) = current_hwnd {
                    if let Some(state) = get_window_state(hwnd) {
                        let changed = state != last_state;

                        if changed {
                            info!(
                                "[WindowTracker] Spotify window updated: HWND=0x{:x} | Rect: {}x{} at ({},{}) | Min: {} | Max: {} | Vis: {}",
                                state.hwnd, state.rect.width, state.rect.height, state.rect.left, state.rect.top,
                                state.minimized, state.maximized, state.visible
                            );

                            if let Some(cb) = &on_event {
                                cb(state.clone());
                            }

                            last_state = state;
                        }
                    }
                }

                // 4. Sleep ~16ms (60 Hz refresh rate for smooth real-time tracking)
                std::thread::sleep(Duration::from_millis(16));
            }

            // Cleanup
            unsafe {
                if !hook.is_invalid() {
                    let _ = UnhookWinEvent(hook);
                }
            }

            info!("[WindowTracker] Window tracking thread terminated.");
        });
    }

    /// Stops the window tracking thread cleanly.
    pub fn stop(&self) {
        if self.is_running.swap(false, Ordering::SeqCst) {
            unsafe {
                if TRACKER_THREAD_ID != 0 {
                    let _ = PostThreadMessageW(
                        TRACKER_THREAD_ID,
                        WM_QUIT,
                        windows::Win32::Foundation::WPARAM(0),
                        windows::Win32::Foundation::LPARAM(0),
                    );
                }
            }
        }
    }
}

impl Drop for WindowTracker {
    fn drop(&mut self) {
        self.stop();
    }
}

/// WinEvent callback function invoked by Windows.
unsafe extern "system" fn win_event_proc(
    _h_win_event_hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    _id_child: i32,
    _id_event_thread: u32,
    _dwms_event_time: u32,
) {
    if id_object != OBJID_WINDOW.0 {
        return;
    }

    let is_spotify = hwnd.0 as isize == ACTIVE_SPOTIFY_HWND;
    let is_relevant_event = event == EVENT_OBJECT_LOCATIONCHANGE
        || event == EVENT_SYSTEM_MINIMIZESTART
        || event == EVENT_SYSTEM_MINIMIZEEND
        || event == EVENT_OBJECT_DESTROY
        || event == EVENT_SYSTEM_FOREGROUND;

    if (is_spotify || event == EVENT_SYSTEM_FOREGROUND) && is_relevant_event {
        if TRACKER_THREAD_ID != 0 {
            let _ = PostThreadMessageW(
                TRACKER_THREAD_ID,
                WM_USER + 1,
                windows::Win32::Foundation::WPARAM(0),
                windows::Win32::Foundation::LPARAM(0),
            );
        }
    }
}

/// Enumerates top-level windows to locate the main Spotify desktop client window.
pub fn find_spotify_window() -> Option<HWND> {
    attach_to_default_desktop();

    struct SearchContext {
        found: Option<HWND>,
    }

    let mut context = SearchContext { found: None };

    unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let ctx = &mut *(lparam.0 as *mut SearchContext);

        // 1. Must be visible or iconic (minimized)
        if !IsWindowVisible(hwnd).as_bool() && !IsIconic(hwnd).as_bool() {
            return BOOL(1);
        }

        // 2. Class name must start with "Chrome_WidgetWin_"
        let mut class_buf = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut class_buf);
        if len == 0 {
            return BOOL(1);
        }
        let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
        if !class_name.starts_with("Chrome_WidgetWin_") {
            return BOOL(1);
        }

        // 3. Process executable name must be "Spotify.exe"
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return BOOL(1);
        }

        if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut img_buf = [0u16; 1024];
            let mut img_size = img_buf.len() as u32;

            if QueryFullProcessImageNameW(
                process,
                PROCESS_NAME_WIN32,
                PWSTR(img_buf.as_mut_ptr()),
                &mut img_size,
            )
            .is_ok()
            {
                let img_name = String::from_utf16_lossy(&img_buf[..img_size as usize]);
                if img_name.to_lowercase().ends_with("spotify.exe") {
                    let mut title_buf = [0u16; 512];
                    let title_len = GetWindowTextW(hwnd, &mut title_buf);
                    let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);

                    // Spotify desktop main window has a title (or when iconic)
                    if !title.is_empty() || IsIconic(hwnd).as_bool() {
                        let _ = CloseHandle(process);
                        ctx.found = Some(hwnd);
                        return BOOL(0); // Stop enumeration
                    }
                }
            }
            let _ = CloseHandle(process);
        }

        BOOL(1)
    }

    let _ = unsafe {
        EnumWindows(
            Some(enum_proc),
            LPARAM(&mut context as *mut SearchContext as isize),
        )
    };

    context.found
}

/// Retrieves the DWM extended frame bounds and window metrics for a given HWND.
pub fn get_window_state(hwnd: HWND) -> Option<SpotifyWindowState> {
    unsafe {
        if !IsWindow(Some(hwnd)).as_bool() {
            return None;
        }

        let visible = IsWindowVisible(hwnd).as_bool();
        let minimized = IsIconic(hwnd).as_bool();
        let maximized = IsZoomed(hwnd).as_bool();

        let mut rect = RECT::default();
        let hr = DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut rect as *mut _ as *mut _,
            std::mem::size_of::<RECT>() as u32,
        );

        let window_rect = if hr.is_ok() {
            WindowRect {
                left: rect.left,
                top: rect.top,
                right: rect.right,
                bottom: rect.bottom,
                width: rect.right - rect.left,
                height: rect.bottom - rect.top,
            }
        } else {
            let mut wr = RECT::default();
            if GetWindowRect(hwnd, &mut wr).is_ok() {
                WindowRect {
                    left: wr.left,
                    top: wr.top,
                    right: wr.right,
                    bottom: wr.bottom,
                    width: wr.right - wr.left,
                    height: wr.bottom - wr.top,
                }
            } else {
                WindowRect::default()
            }
        };

        let dpi = GetDpiForWindow(hwnd);
        let dpi = if dpi == 0 { 96 } else { dpi };
        let is_foreground = GetForegroundWindow() == hwnd;

        Some(SpotifyWindowState {
            hwnd: hwnd.0 as isize,
            rect: window_rect,
            visible,
            minimized,
            maximized,
            dpi,
            is_foreground,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_window_rect_math() {
        let rect = WindowRect {
            left: 100,
            top: 200,
            right: 900,
            bottom: 800,
            width: 800,
            height: 600,
        };
        assert_eq!(rect.width, rect.right - rect.left);
        assert_eq!(rect.height, rect.bottom - rect.top);
    }

    #[test]
    fn test_find_spotify_window_live() {
        attach_to_default_desktop();
        let result = find_spotify_window();
        println!("find_spotify_window result: {:?}", result);
        if let Some(hwnd) = result {
            let state = get_window_state(hwnd);
            assert!(state.is_some());
            let st = state.unwrap();
            println!(
                "Spotify State: HWND=0x{:x}, Rect={:?}, Minimized={}, Maximized={}, DPI={}",
                st.hwnd, st.rect, st.minimized, st.maximized, st.dpi
            );
            assert_ne!(st.hwnd, 0);
        }
    }

    #[test]
    fn test_restore_spotify_and_check_state() {
        use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_RESTORE};
        attach_to_default_desktop();
        if let Some(hwnd) = find_spotify_window() {
            let before = get_window_state(hwnd).unwrap();
            println!("BEFORE restore: Minimized={}, Rect={:?}", before.minimized, before.rect);
            if before.minimized {
                unsafe {
                    let _ = ShowWindow(hwnd, SW_RESTORE);
                }
                std::thread::sleep(Duration::from_millis(200));
                let after = get_window_state(hwnd).unwrap();
                println!("AFTER restore: Minimized={}, Rect={:?}", after.minimized, after.rect);
            }
        }
    }
}
