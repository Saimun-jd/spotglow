use std::sync::atomic::{AtomicBool, AtomicI32, AtomicU8, Ordering};
use std::sync::Arc;
use tracing::{info, warn};

use windows::Win32::Foundation::HWND;
use windows::Win32::UI::WindowsAndMessaging::{
    GetClassNameW, GetWindow, GetWindowLongPtrW, SetLayeredWindowAttributes, SetWindowLongPtrW,
    SetWindowPos, ShowWindow, GWL_EXSTYLE, GW_HWNDPREV, HWND_TOP, IsWindowVisible,
    SET_WINDOW_POS_FLAGS, SWP_FRAMECHANGED, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, SWP_NOZORDER,
    SWP_SHOWWINDOW, SW_HIDE, SW_SHOWNOACTIVATE, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
    LWA_ALPHA,
};

use crate::state::DisplayMode;
use crate::window_tracker::SpotifyWindowState;

pub const MODE_BORDER_GLOW: u8 = 0;
pub const MODE_COVER_ART: u8 = 1;

/// Completely hides the Spotify window (both SW_HIDE and alpha = 0 layered attribute)
pub unsafe fn hide_spotify_window(spotify_hwnd: HWND) {
    if spotify_hwnd.0.is_null() {
        return;
    }
    let cur_ex = GetWindowLongPtrW(spotify_hwnd, GWL_EXSTYLE) as u32;
    let _ = SetWindowLongPtrW(spotify_hwnd, GWL_EXSTYLE, (cur_ex | WS_EX_LAYERED.0) as isize);
    let _ = SetLayeredWindowAttributes(spotify_hwnd, windows::Win32::Foundation::COLORREF(0), 0, LWA_ALPHA);
    let _ = ShowWindow(spotify_hwnd, SW_HIDE);
    info!("[OverlayController] Spotify HWND 0x{:x} made invisible (SW_HIDE + alpha 0)", spotify_hwnd.0 as isize);
}

/// Restores the Spotify window to visible and opaque
pub unsafe fn restore_spotify_window(spotify_hwnd: HWND) {
    if spotify_hwnd.0.is_null() {
        return;
    }
    let _ = ShowWindow(spotify_hwnd, SW_SHOWNOACTIVATE);
    let _ = SetLayeredWindowAttributes(spotify_hwnd, windows::Win32::Foundation::COLORREF(0), 255, LWA_ALPHA);
    let cur_ex = GetWindowLongPtrW(spotify_hwnd, GWL_EXSTYLE) as u32;
    let _ = SetWindowLongPtrW(spotify_hwnd, GWL_EXSTYLE, (cur_ex & !WS_EX_LAYERED.0) as isize);
    let _ = SetWindowPos(spotify_hwnd, None, 0, 0, 0, 0, SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED);
    info!("[OverlayController] Spotify HWND 0x{:x} restored to visible", spotify_hwnd.0 as isize);
}

/// Overlay margin in LOGICAL pixels. Must equal the margin passed to `GlowRenderer`
/// in glow.ts (`getRecommendedMargin()` returns 18 at thickness 1.0).
/// It is multiplied by the monitor DPI scale before being applied (see `scaled_margin`).
pub const DEFAULT_MARGIN: i32 = 18;

/// Converts a logical-pixel margin to physical pixels for the given monitor DPI
/// (96 = 100%, 120 = 125%, 144 = 150%, ...). A DPI of 0 is treated as 96.
pub fn scaled_margin(base_logical: i32, dpi: f32) -> i32 {
    let scale = if dpi > 0.0 { dpi / 96.0 } else { 1.0 };
    (base_logical as f32 * scale).round() as i32
}

/// Finds the visible window immediately above Spotify in Z-order,
/// skipping invisible, tooltip, and IME helper windows.
/// Returns None if Spotify is at the top of the normal window stack (i.e. HWND_TOP).
pub unsafe fn find_window_above(spotify_hwnd: HWND, overlay_hwnd: HWND) -> Option<HWND> {
    let mut curr = spotify_hwnd;
    while let Ok(prev) = GetWindow(curr, GW_HWNDPREV) {
        if prev.0.is_null() {
            break;
        }
        if prev != overlay_hwnd && IsWindowVisible(prev).as_bool() {
            let mut class_buf = [0u16; 64];
            let len = GetClassNameW(prev, &mut class_buf);
            let class_name = String::from_utf16_lossy(&class_buf[..len as usize]);
            if class_name != "IME" && class_name != "MSCTFIME UI" && class_name != "tooltips_class32" {
                return Some(prev);
            }
        }
        curr = prev;
    }
    None
}

#[derive(Clone)]
pub struct OverlayController {
    overlay_hwnd: isize,
    window: Option<tauri::WebviewWindow>,
    margin: Arc<AtomicI32>, // logical px
    is_visible: Arc<AtomicBool>,
    display_mode: Arc<AtomicU8>,
    last_x: Arc<AtomicI32>,
    last_y: Arc<AtomicI32>,
    last_w: Arc<AtomicI32>,
    last_h: Arc<AtomicI32>,
}

unsafe impl Send for OverlayController {}
unsafe impl Sync for OverlayController {}

impl OverlayController {
    pub fn new(overlay_hwnd: HWND, window: Option<tauri::WebviewWindow>) -> Self {
        Self {
            overlay_hwnd: overlay_hwnd.0 as isize,
            window,
            margin: Arc::new(AtomicI32::new(DEFAULT_MARGIN)),
            is_visible: Arc::new(AtomicBool::new(false)),
            display_mode: Arc::new(AtomicU8::new(MODE_BORDER_GLOW)),
            last_x: Arc::new(AtomicI32::new(i32::MIN)),
            last_y: Arc::new(AtomicI32::new(i32::MIN)),
            last_w: Arc::new(AtomicI32::new(0)),
            last_h: Arc::new(AtomicI32::new(0)),
        }
    }

    pub fn get_hwnd(&self) -> HWND {
        HWND(self.overlay_hwnd as *mut _)
    }

    /// Margin in logical pixels.
    pub fn margin(&self) -> i32 {
        self.margin.load(Ordering::Relaxed)
    }

    /// Sets the margin in logical pixels (call with `renderer.getRecommendedMargin()` from the frontend).
    pub fn set_margin(&self, margin: i32) {
        self.margin.store(margin, Ordering::Relaxed);
    }

    pub fn display_mode(&self) -> DisplayMode {
        if self.display_mode.load(Ordering::Relaxed) == MODE_COVER_ART {
            DisplayMode::CoverArt
        } else {
            DisplayMode::BorderGlow
        }
    }

    pub fn set_display_mode(&self, mode: DisplayMode) {
        let val = match mode {
            DisplayMode::BorderGlow => MODE_BORDER_GLOW,
            DisplayMode::CoverArt => MODE_COVER_ART,
        };
        self.display_mode.store(val, Ordering::SeqCst);
        if let Some(win) = &self.window {
            match mode {
                DisplayMode::CoverArt => {
                    let _ = win.set_ignore_cursor_events(false);
                    info!("[OverlayController] Switched to CoverArt mode: cursor events ENABLED");
                }
                DisplayMode::BorderGlow => {
                    let _ = win.set_ignore_cursor_events(true);
                    info!("[OverlayController] Switched to BorderGlow mode: cursor events DISABLED (click-through)");
                }
            }
        }
    }

    /// Applies non-activating and tool-window ex-styles, and makes the overlay click-through.
    /// Note: WS_EX_LAYERED is deliberately omitted because it breaks WebView2 DirectComposition.
    /// Tauri's native transparency + set_ignore_cursor_events handles click-through and alpha cleanly.
    pub fn apply_styles(&self) -> bool {
        let hwnd = self.get_hwnd();

        // Click-through (important in maximized mode, where the overlay sits above Spotify)
        if let Some(win) = &self.window {
            let _ = win.set_ignore_cursor_events(true);
        }

        unsafe {
            let cur_ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
            let new_ex_style = cur_ex_style
                | WS_EX_NOACTIVATE.0
                | WS_EX_TOOLWINDOW.0;
            let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, new_ex_style as isize);

            // Notify Windows to refresh window frame and styles (only needed here, not on every move)
            let flags: SET_WINDOW_POS_FLAGS = SWP_NOMOVE | SWP_NOSIZE | SWP_NOZORDER | SWP_FRAMECHANGED | SWP_NOACTIVATE;
            let res = SetWindowPos(hwnd, None, 0, 0, 0, 0, flags);

            if res.is_ok() {
                info!("[OverlayController] Applied non-activating & toolwindow ex-styles to overlay HWND 0x{:x}", self.overlay_hwnd);
                true
            } else {
                warn!("[OverlayController] Failed to apply ex-styles: {:?}", res.err());
                false
            }
        }
    }

    /// Repositions the overlay window to follow Spotify with pixel precision:
    /// - When restored (floating): Behind Mode — directly behind Spotify, padded by the DPI-scaled
    ///   logical margin on every side, so the glow halo shows around Spotify's edges.
    /// - When maximized: Inner Edge Mode — exact screen bounds, directly in front of Spotify, inner glow.
    ///
    /// Call this on every Spotify window event, including foreground changes, so the overlay
    /// stays directly behind Spotify when Spotify is raised.
    pub fn reposition_behind(&self, state: &SpotifyWindowState) {
        let hwnd = self.get_hwnd();
        unsafe {
            // Case 1: Spotify is closed, not found, minimized, or hidden
            if state.hwnd == 0 || state.minimized || !state.visible || state.rect.width <= 0 || state.rect.height <= 0 {
                self.hide();
                return;
            }

            // Case 2: Spotify is active and visible
            let is_cover_art = self.display_mode.load(Ordering::Relaxed) == MODE_COVER_ART;

            let margin = if state.maximized {
                0
            } else {
                scaled_margin(self.margin.load(Ordering::Relaxed), state.dpi as f32)
            };

            let (target_x, target_y, target_width, target_height) = (
                state.rect.left - margin,
                state.rect.top - margin,
                state.rect.width + margin * 2,
                state.rect.height + margin * 2,
            );

            let insert_after = if is_cover_art || state.maximized {
                let spotify_hwnd = HWND(state.hwnd as *mut _);
                let above = find_window_above(spotify_hwnd, hwnd);
                match above {
                    Some(w) => Some(w),
                    None => Some(HWND_TOP),
                }
            } else {
                let spotify_hwnd = HWND(state.hwnd as *mut _);
                Some(spotify_hwnd)
            };

            self.last_x.swap(target_x, Ordering::SeqCst);
            self.last_y.swap(target_y, Ordering::SeqCst);
            let prev_w = self.last_w.swap(target_width, Ordering::SeqCst);
            let prev_h = self.last_h.swap(target_height, Ordering::SeqCst);
            let size_changed = target_width != prev_w || target_height != prev_h;

            // Keep the WebView2 viewport in sync, but only when the SIZE changes.
            // Position is handled by SetWindowPos below (avoids moving the window twice per event).
            if size_changed {
                if let Some(win) = &self.window {
                    let _ = win.set_size(tauri::Size::Physical(tauri::PhysicalSize {
                        width: target_width as u32,
                        height: target_height as u32,
                    }));
                }
            }

            // Ensure overlay window is shown without activating or stealing focus
            if !self.is_visible.swap(true, Ordering::SeqCst) {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }

            // No SWP_FRAMECHANGED here: it forces a frame recalculation on every drag event.
            let flags: SET_WINDOW_POS_FLAGS = SWP_NOACTIVATE | SWP_SHOWWINDOW;
            let pos_res = SetWindowPos(
                hwnd,
                insert_after,
                target_x,
                target_y,
                target_width,
                target_height,
                flags,
            );

            if pos_res.is_err() {
                warn!("[OverlayController] SetWindowPos failed: {:?}", pos_res.err());
            } else if size_changed {
                info!(
                    "[OverlayController] Overlay positioned: {}x{} at ({},{}) (Maximized: {})",
                    target_width, target_height, target_x, target_y, state.maximized
                );
            }
        }
    }

    /// Hides the overlay immediately without changing focus.
    pub fn hide(&self) {
        if self.is_visible.swap(false, Ordering::SeqCst) {
            info!("[OverlayController] Hiding overlay.");
            let hwnd = self.get_hwnd();
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::window_tracker::WindowRect;

    #[test]
    fn test_margin_calculations() {
        let state = SpotifyWindowState {
            hwnd: 0x1234,
            rect: WindowRect {
                left: 100,
                top: 100,
                right: 900,
                bottom: 700,
                width: 800,
                height: 600,
            },
            visible: true,
            minimized: false,
            maximized: false,
            dpi: 96,
            is_foreground: true,
        };

        let margin = scaled_margin(DEFAULT_MARGIN, state.dpi as f32);
        let x = state.rect.left - margin;
        let y = state.rect.top - margin;
        let width = state.rect.width + margin * 2;
        let height = state.rect.height + margin * 2;

        assert_eq!(margin, 18);
        assert_eq!(x, 82);
        assert_eq!(y, 82);
        assert_eq!(width, 836);
        assert_eq!(height, 636);
    }

    #[test]
    fn test_scaled_margin() {
        assert_eq!(scaled_margin(18, 96.0), 18); // 100%
        assert_eq!(scaled_margin(18, 120.0), 23); // 125%
        assert_eq!(scaled_margin(18, 144.0), 27); // 150%
        assert_eq!(scaled_margin(18, 192.0), 36); // 200%
        assert_eq!(scaled_margin(18, 0.0), 18); // unknown DPI falls back to 100%
    }

    #[test]
    fn test_maximized_calculations() {
        let state = SpotifyWindowState {
            hwnd: 0x1234,
            rect: WindowRect {
                left: 0,
                top: 0,
                right: 1920,
                bottom: 1080,
                width: 1920,
                height: 1080,
            },
            visible: true,
            minimized: false,
            maximized: true,
            dpi: 96,
            is_foreground: true,
        };

        // Maximized should match exact screen bounds without margin offset
        let target_x = state.rect.left;
        let target_y = state.rect.top;
        let target_width = state.rect.width;
        let target_height = state.rect.height;

        assert_eq!(target_x, 0);
        assert_eq!(target_y, 0);
        assert_eq!(target_width, 1920);
        assert_eq!(target_height, 1080);
    }

    // ─────────────────────────────────────────────────────────────────────────
    // Live tests: need Spotify (and for the last one, SpotGlow) running on your desktop.
    // They are ignored by default so plain `cargo test` passes everywhere.
    // Run them with:  cargo test -- --ignored --nocapture
    // ─────────────────────────────────────────────────────────────────────────

    #[test]
    #[ignore = "needs a live desktop with SpotGlow/Spotify running"]
    fn test_inspect_spotglow_and_spotify_windows() {
        use windows::Win32::UI::WindowsAndMessaging::*;
        use windows::Win32::Foundation::RECT;
        use windows::Win32::System::Threading::*;
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
        use windows::core::PWSTR;
        use crate::window_tracker::attach_to_default_desktop;

        attach_to_default_desktop();

        struct WinInfo {
            hwnd: isize,
            pid: u32,
            vis: bool,
            rect: RECT,
            class: String,
            title: String,
            proc_name: String,
        }

        struct Ctx {
            list: Vec<WinInfo>,
        }

        let mut ctx = Ctx { list: Vec::new() };

        unsafe extern "system" fn enum_proc(hwnd: HWND, lparam: windows::Win32::Foundation::LPARAM) -> windows::core::BOOL {
            let ctx = &mut *(lparam.0 as *mut Ctx);
            let mut pid: u32 = 0;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid == 0 {
                return windows::core::BOOL(1);
            }

            if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                let mut img_buf = [0u16; 1024];
                let mut img_size = img_buf.len() as u32;
                if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(img_buf.as_mut_ptr()), &mut img_size).is_ok() {
                    let img_name = String::from_utf16_lossy(&img_buf[..img_size as usize]);
                    let lower = img_name.to_lowercase();
                    if lower.contains("spotglow") || lower.contains("spotify") {
                        let mut class_buf = [0u16; 256];
                        let len = GetClassNameW(hwnd, &mut class_buf);
                        let class = String::from_utf16_lossy(&class_buf[..len as usize]);

                        let mut title_buf = [0u16; 512];
                        let title_len = GetWindowTextW(hwnd, &mut title_buf);
                        let title = String::from_utf16_lossy(&title_buf[..title_len as usize]);

                        let vis = IsWindowVisible(hwnd).as_bool();
                        let mut rect = RECT::default();
                        let _ = DwmGetWindowAttribute(hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut rect as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32);

                        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;

                        ctx.list.push(WinInfo {
                            hwnd: hwnd.0 as isize,
                            pid,
                            vis,
                            rect,
                            class,
                            title,
                            proc_name: format!("{} (ex_style: 0x{:08x})", img_name, ex_style),
                        });
                    }
                }
                let _ = windows::Win32::Foundation::CloseHandle(process);
            }
            windows::core::BOOL(1)
        }

        let _ = unsafe {
            EnumWindows(
                Some(enum_proc),
                windows::Win32::Foundation::LPARAM(&mut ctx as *mut Ctx as isize),
            )
        };

        println!("\n=== RUNNING SPOTGLOW & SPOTIFY WINDOWS ===");
        for w in &ctx.list {
            println!(
                "HWND: 0x{:x} (PID {}) | Vis: {} | Bounds: [{}, {}, {}, {}] ({}x{}) | Class: '{}' | Title: '{}' | Exe: {}",
                w.hwnd, w.pid, w.vis, w.rect.left, w.rect.top, w.rect.right, w.rect.bottom,
                w.rect.right - w.rect.left, w.rect.bottom - w.rect.top,
                w.class, w.title, w.proc_name
            );
        }
    }

    #[test]
    #[ignore = "needs Spotify running; restores and focuses its window"]
    fn test_restore_spotify_and_observe_spotglow() {
        use std::time::Duration;
        use crate::window_tracker::find_spotify_window;

        if let Some(spotify_hwnd) = find_spotify_window() {
            println!("Restoring Spotify HWND 0x{:x}...", spotify_hwnd.0 as isize);
            unsafe {
                use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SetForegroundWindow, SW_SHOW, SW_RESTORE};
                let _ = ShowWindow(spotify_hwnd, SW_SHOW);
                let _ = ShowWindow(spotify_hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(spotify_hwnd);
            }
            std::thread::sleep(Duration::from_millis(1500));
        }

        test_inspect_spotglow_and_spotify_windows();
    }

    #[test]
    #[ignore = "needs Spotify and SpotGlow running"]
    fn test_live_tracker_and_move_spotglow() {
        use windows::Win32::Foundation::RECT;
        use crate::window_tracker::{find_spotify_window, get_window_state};

        let spot = find_spotify_window().expect("Spotify window should be found");
        let state = get_window_state(spot).expect("State should be available");
        println!("\n>>> Live Spotify state: HWND=0x{:x}, rect={:?}, min={}, vis={}", state.hwnd, state.rect, state.minimized, state.visible);

        // Find SpotGlow HWND
        struct FindCtx { hwnd: Option<HWND> }
        let mut fctx = FindCtx { hwnd: None };
        unsafe extern "system" fn fproc(h: HWND, lp: windows::Win32::Foundation::LPARAM) -> windows::core::BOOL {
            let ctx = &mut *(lp.0 as *mut FindCtx);
            let mut title_buf = [0u16; 128];
            let len = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(h, &mut title_buf);
            let title = String::from_utf16_lossy(&title_buf[..len as usize]);
            if title == "SpotGlow Overlay" {
                ctx.hwnd = Some(h);
                return windows::core::BOOL(0);
            }
            windows::core::BOOL(1)
        }
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::EnumWindows(
                Some(fproc),
                windows::Win32::Foundation::LPARAM(&mut fctx as *mut FindCtx as isize),
            );
        }

        if let Some(glow_hwnd) = fctx.hwnd {
            println!(">>> Found SpotGlow HWND: 0x{:x}", glow_hwnd.0 as isize);
            let ctrl = OverlayController::new(glow_hwnd, None);
            println!(">>> Calling reposition_behind live...");
            ctrl.reposition_behind(&state);

            // Check new bounds of SpotGlow
            use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
            let mut r = RECT::default();
            let _ = unsafe { DwmGetWindowAttribute(glow_hwnd, DWMWA_EXTENDED_FRAME_BOUNDS, &mut r as *mut _ as *mut _, std::mem::size_of::<RECT>() as u32) };
            println!(">>> New SpotGlow bounds after reposition: [{}, {}, {}, {}] ({}x{})",
                r.left, r.top, r.right, r.bottom, r.right - r.left, r.bottom - r.top
            );
        } else {
            println!(">>> SpotGlow window not found running!");
        }
    }
}