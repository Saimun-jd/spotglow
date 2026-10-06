use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::{
    GetWindow, GetWindowLongPtrW, GetWindowRect, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    GWL_EXSTYLE, GWL_STYLE, GW_HWNDNEXT, GW_HWNDPREV,
};

fn main() {
    spotglow_lib::window_tracker::attach_to_default_desktop();
    println!("Looking for Spotify window...");
    let spotify_opt = spotglow_lib::window_tracker::find_spotify_window();
    println!("Spotify window found: {:?}", spotify_opt);

    if let Some(spotify_hwnd) = spotify_opt {
        if let Some(state) = spotglow_lib::window_tracker::get_window_state(spotify_hwnd) {
            println!(
                "Spotify State: HWND=0x{:x}, Rect={:?}, Min={}, Max={}, Vis={}, FG={}, DPI={}",
                state.hwnd, state.rect, state.minimized, state.maximized, state.visible, state.is_foreground, state.dpi
            );
            if !state.maximized {
                println!("Testing Spotify MAXIMIZE via Win32 ShowWindow(SW_MAXIMIZE)...");
                unsafe {
                    use windows::Win32::UI::WindowsAndMessaging::{ShowWindow, SW_MAXIMIZE, SW_RESTORE};
                    let _ = ShowWindow(spotify_hwnd, SW_MAXIMIZE);
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    let max_st = spotglow_lib::window_tracker::get_window_state(spotify_hwnd).unwrap();
                    println!("MAXIMIZED State: Rect={:?}, Max={}", max_st.rect, max_st.maximized);

                    println!("Restoring Spotify back to windowed mode via ShowWindow(SW_RESTORE)...");
                    let _ = ShowWindow(spotify_hwnd, SW_RESTORE);
                    std::thread::sleep(std::time::Duration::from_millis(1500));
                    let rest_st = spotglow_lib::window_tracker::get_window_state(spotify_hwnd).unwrap();
                    println!("RESTORED State: Rect={:?}, Max={}", rest_st.rect, rest_st.maximized);
                }
            }
        }
    }

    println!("Checking SpotGlow HWND 0x1b12a6 directly...");
    unsafe {
        let spotglow_hwnd = HWND(0x1b12a6 as *mut _);
        let is_win = windows::Win32::UI::WindowsAndMessaging::IsWindow(Some(spotglow_hwnd)).as_bool();
        let is_vis = windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(spotglow_hwnd).as_bool();
        let mut r = windows::Win32::Foundation::RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetWindowRect(spotglow_hwnd, &mut r);
        let style = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(spotglow_hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_STYLE);
        let ex_style = windows::Win32::UI::WindowsAndMessaging::GetWindowLongPtrW(spotglow_hwnd, windows::Win32::UI::WindowsAndMessaging::GWL_EXSTYLE);
        let mut title_buf = [0u16; 512];
        let tlen = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(spotglow_hwnd, &mut title_buf);
        let title = String::from_utf16_lossy(&title_buf[..tlen as usize]);
        let mut cls_buf = [0u16; 256];
        let clen = windows::Win32::UI::WindowsAndMessaging::GetClassNameW(spotglow_hwnd, &mut cls_buf);
        let cls = String::from_utf16_lossy(&cls_buf[..clen as usize]);

        let prev = windows::Win32::UI::WindowsAndMessaging::GetWindow(spotglow_hwnd, windows::Win32::UI::WindowsAndMessaging::GW_HWNDPREV);
        let next = windows::Win32::UI::WindowsAndMessaging::GetWindow(spotglow_hwnd, windows::Win32::UI::WindowsAndMessaging::GW_HWNDNEXT);

        println!("Direct HWND 0x1b12a6 check:");
        println!("  IsWindow: {}, IsVisible: {}", is_win, is_vis);
        println!("  Rect: [{}, {}, {}, {}] ({}x{})", r.left, r.top, r.right, r.bottom, r.right - r.left, r.bottom - r.top);
        println!("  Style: 0x{:08x}, ExStyle: 0x{:08x}", style, ex_style);
        println!("  Class: '{}', Title: '{}'", cls, title);
        println!("  Z-Order: Prev (above): {:?}, Next (below): {:?}", prev, next);
    }

    println!("Enumerating all windows to find spotglow.exe...");
    unsafe {
        use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindowThreadProcessId, GetWindowTextW, GetWindowRect, IsWindowVisible, GetWindowLongPtrW, GWL_STYLE, GWL_EXSTYLE};
        use windows::Win32::Foundation::{CloseHandle, LPARAM, RECT};
        use windows::core::{BOOL, PWSTR};

        static mut COUNT: u32 = 0;
        unsafe extern "system" fn enum_proc(hwnd: HWND, _lparam: LPARAM) -> BOOL {
            COUNT += 1;
            let mut pid = 0u32;
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
            if pid != 0 {
                let mut title_buf = [0u16; 512];
                let len = GetWindowTextW(hwnd, &mut title_buf);
                let title = String::from_utf16_lossy(&title_buf[..len as usize]);

                if let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
                    let mut img_buf = [0u16; 1024];
                    let mut img_size = img_buf.len() as u32;
                    let is_spotglow = if QueryFullProcessImageNameW(process, PROCESS_NAME_WIN32, PWSTR(img_buf.as_mut_ptr()), &mut img_size).is_ok() {
                        let img_name = String::from_utf16_lossy(&img_buf[..img_size as usize]).to_lowercase();
                        img_name.contains("spotglow")
                    } else {
                        false
                    };
                    let _ = CloseHandle(process);

                    if is_spotglow || title.to_lowercase().contains("spotglow") {
                        let mut r = RECT::default();
                        let _ = GetWindowRect(hwnd, &mut r);
                        let style = GetWindowLongPtrW(hwnd, GWL_STYLE);
                        let ex_style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
                        println!(
                            "-> SpotGlow HWND: 0x{:x} | PID: {} | Vis: {} | Rect: ({},{}) {}x{} | Style: 0x{:x} | ExStyle: 0x{:x} | Title: '{}'",
                            hwnd.0 as isize, pid, IsWindowVisible(hwnd).as_bool(),
                            r.left, r.top, r.right - r.left, r.bottom - r.top,
                            style, ex_style, title
                        );
                    }
                }
            }
            BOOL(1)
        }

        let _ = EnumWindows(Some(enum_proc), LPARAM(0));
        println!("Total windows scanned: {}", COUNT);
    }
}
