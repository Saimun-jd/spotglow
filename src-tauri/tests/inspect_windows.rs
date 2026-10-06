use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::Win32::System::Threading::*;
use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_EXTENDED_FRAME_BOUNDS};
use windows::core::PWSTR;

#[test]
fn test_inspect_spotglow_and_spotify_windows() {
    spotglow_lib::window_tracker::attach_to_default_desktop();

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
fn test_restore_spotify_and_observe_spotglow() {
    use std::time::Duration;
    use spotglow_lib::window_tracker::find_spotify_window;

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

