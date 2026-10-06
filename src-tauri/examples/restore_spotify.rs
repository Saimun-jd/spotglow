use std::time::Duration;
use windows::Win32::Foundation::{HWND, LPARAM};
use windows::core::BOOL;
use windows::Win32::System::StationsAndDesktops::{OpenDesktopW, SetThreadDesktop, DESKTOP_CONTROL_FLAGS};
use windows::Win32::System::Threading::{OpenProcess, QueryFullProcessImageNameW, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetClassNameW, GetWindowTextW, GetWindowThreadProcessId,
    IsIconic, IsWindowVisible, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
};
use windows::core::PWSTR;

fn attach_desktop() {
    unsafe {
        let default_name = windows::core::w!("Default");
        let access = 0x01FF;
        if let Ok(hdesk) = OpenDesktopW(default_name, DESKTOP_CONTROL_FLAGS(0), false, access) {
            let _ = SetThreadDesktop(hdesk);
        }
    }
}

fn find_spotify() -> Option<HWND> {
    attach_desktop();
    struct Ctx { found: Option<HWND> }
    let mut ctx = Ctx { found: None };

    unsafe extern "system" fn proc(hwnd: HWND, lp: LPARAM) -> BOOL {
        let ctx = &mut *(lp.0 as *mut Ctx);
        let mut class_buf = [0u16; 256];
        let len = GetClassNameW(hwnd, &mut class_buf);
        let class = String::from_utf16_lossy(&class_buf[..len as usize]);
        if !class.starts_with("Chrome_WidgetWin_") {
            return BOOL(1);
        }

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) {
            let mut img_buf = [0u16; 1024];
            let mut img_size = img_buf.len() as u32;
            if QueryFullProcessImageNameW(proc, PROCESS_NAME_WIN32, PWSTR(img_buf.as_mut_ptr()), &mut img_size).is_ok() {
                let img = String::from_utf16_lossy(&img_buf[..img_size as usize]);
                if img.to_lowercase().ends_with("spotify.exe") {
                    let mut title_buf = [0u16; 512];
                    let tlen = GetWindowTextW(hwnd, &mut title_buf);
                    let title = String::from_utf16_lossy(&title_buf[..tlen as usize]);
                    if !title.is_empty() || IsIconic(hwnd).as_bool() {
                        ctx.found = Some(hwnd);
                        return BOOL(0);
                    }
                }
            }
        }
        BOOL(1)
    }

    unsafe {
        let _ = EnumWindows(Some(proc), LPARAM(&mut ctx as *mut Ctx as isize));
    }
    ctx.found
}

fn main() {
    println!("Looking for Spotify window...");
    if let Some(hwnd) = find_spotify() {
        println!("Found Spotify HWND: 0x{:x}", hwnd.0 as isize);
        unsafe {
            let iconic = IsIconic(hwnd).as_bool();
            println!("IsIconic before: {}", iconic);
            let _ = ShowWindow(hwnd, SW_RESTORE);
            let _ = ShowWindow(hwnd, SW_SHOW);
            let _ = SetForegroundWindow(hwnd);
            std::thread::sleep(Duration::from_millis(500));
            let iconic_after = IsIconic(hwnd).as_bool();
            let visible_after = IsWindowVisible(hwnd).as_bool();
            println!("IsIconic after: {}, Visible: {}", iconic_after, visible_after);
        }
    } else {
        println!("Spotify window not found!");
    }
}
