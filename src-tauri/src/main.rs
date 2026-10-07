// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

#[cfg(windows)]
fn attach_console() {
    use windows::Win32::System::Console::{AttachConsole, ATTACH_PARENT_PROCESS};
    unsafe {
        let _ = AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

#[cfg(not(windows))]
fn attach_console() {}

fn print_banner() {
    println!();
    println!("  === SpotGlow v0.2.0 CLI ===");
    println!("  Ambient Spotify Window Border Glow");
    println!("  ----------------------------------");
}

fn print_help() {
    print_banner();
    println!("  Usage: spotglow.exe [OPTIONS]");
    println!();
    println!("  Options:");
    println!("    -u, --uninstall   Uninstall SpotGlow silently from the terminal");
    println!("    -s, --status      Check running status and Spotify connection");
    println!("    -v, --version     Display SpotGlow version");
    println!("    -h, --help        Show this help message");
    println!();
    let _ = std::io::stdout().flush();
}

fn print_version() {
    println!("SpotGlow v0.2.0");
    let _ = std::io::stdout().flush();
}

fn handle_status() {
    print_banner();
    let current_exe = env::current_exe().unwrap_or_default();
    println!("  [+] Binary Path: {}", current_exe.display());

    // Check if Spotify is running
    match spotglow_lib::window_tracker::find_spotify_window() {
        Some(hwnd) => {
            if let Some(state) = spotglow_lib::window_tracker::get_window_state(hwnd) {
                println!("  [+] Spotify Detected: HWND=0x{:x}", state.hwnd);
                println!(
                    "      Bounds: {}x{} at ({}, {})",
                    state.rect.width, state.rect.height, state.rect.left, state.rect.top
                );
                println!(
                    "      State: Visible={}, Minimized={}, Maximized={}, DPI={}",
                    state.visible, state.minimized, state.maximized, state.dpi
                );
            } else {
                println!(
                    "  [+] Spotify Window found: HWND=0x{:x} (Metrics pending)",
                    hwnd.0 as isize
                );
            }
        }
        None => {
            println!("  [-] Spotify is not currently running or window is not visible.");
        }
    }
    println!();
    let _ = std::io::stdout().flush();
}

fn handle_uninstall() {
    print_banner();
    println!("  [+] Initiating SpotGlow uninstallation from terminal...");

    // 1. Terminate other running instances of spotglow.exe
    let current_pid = std::process::id();
    println!("  [+] Closing active SpotGlow background instances...");
    let _ = Command::new("taskkill")
        .args(&[
            "/F",
            "/FI",
            &format!("PID ne {}", current_pid),
            "/IM",
            "spotglow.exe",
        ])
        .output();

    // 2. Locate official NSIS uninstaller
    let mut uninstaller_path: Option<PathBuf> = None;

    // Check directory of running executable
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let local_uninstaller = parent.join("Uninstall SpotGlow.exe");
            if local_uninstaller.exists() {
                uninstaller_path = Some(local_uninstaller);
            }
        }
    }

    // Check standard LocalAppData paths
    if uninstaller_path.is_none() {
        if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
            let cand1 = Path::new(&local_app_data)
                .join("Programs")
                .join("SpotGlow")
                .join("Uninstall SpotGlow.exe");
            let cand2 = Path::new(&local_app_data)
                .join("SpotGlow")
                .join("Uninstall SpotGlow.exe");
            if cand1.exists() {
                uninstaller_path = Some(cand1);
            } else if cand2.exists() {
                uninstaller_path = Some(cand2);
            }
        }
    }

    // Check registry via PowerShell if not found yet
    if uninstaller_path.is_none() {
        let ps_cmd = r#"
            $paths = @(
                'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\com.spotglow.app',
                'HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SpotGlow'
            )
            foreach ($k in $paths) {
                if (Test-Path $k) {
                    $u = (Get-ItemProperty $k -ErrorAction SilentlyContinue).UninstallString
                    if ($u) {
                        $u = $u -replace '"',''
                        if (Test-Path $u) { $u; break }
                    }
                }
            }
        "#;
        if let Ok(output) = Command::new("powershell")
            .args(&["-NoProfile", "-Command", ps_cmd])
            .output()
        {
            let str_val = String::from_utf8_lossy(&output.stdout)
                .trim()
                .trim_matches('"')
                .to_string();
            if !str_val.is_empty() && Path::new(&str_val).exists() {
                uninstaller_path = Some(PathBuf::from(str_val));
            }
        }
    }

    if let Some(ref uninstaller) = uninstaller_path {
        println!("  [+] Found official uninstaller: {}", uninstaller.display());
        println!("  [+] Running silent uninstallation (/S)...");
        let status = Command::new(uninstaller).arg("/S").status();

        match status {
            Ok(s) if s.success() => {
                println!("  [OK] Uninstaller completed successfully.");
            }
            Ok(s) => {
                println!("  [!] Uninstaller exited with status code: {:?}", s.code());
            }
            Err(e) => {
                println!("  [!] Failed to invoke uninstaller: {}", e);
            }
        }
    } else {
        println!("  [i] No NSIS uninstaller found (portable or development build). Cleaning up system links...");
    }

    // 3. Clean up Start Menu and Desktop shortcuts
    if let Ok(app_data) = env::var("APPDATA") {
        let start_shortcut = Path::new(&app_data)
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs")
            .join("SpotGlow.lnk");
        if start_shortcut.exists() {
            let _ = std::fs::remove_file(&start_shortcut);
            println!("  [+] Removed Start Menu shortcut.");
        }
    }

    if let Ok(user_profile) = env::var("USERPROFILE") {
        let desktop_shortcut = Path::new(&user_profile)
            .join("Desktop")
            .join("SpotGlow.lnk");
        if desktop_shortcut.exists() {
            let _ = std::fs::remove_file(&desktop_shortcut);
            println!("  [+] Removed Desktop shortcut.");
        }
    }

    // 4. Remove from User PATH
    if let Some(uninstaller) = &uninstaller_path {
        if let Some(parent) = uninstaller.parent() {
            remove_from_user_path(parent);
            println!("  [+] Removed SpotGlow from User PATH.");
        }
    }

    println!("  [OK] SpotGlow uninstallation finished.");
    println!();
    let _ = std::io::stdout().flush();
}

#[cfg(windows)]
fn ensure_user_path() {
    if let Ok(exe) = env::current_exe() {
        if let Some(parent) = exe.parent() {
            let dir_str = parent.to_string_lossy();
            let is_installed = dir_str.contains("AppData") || dir_str.contains("Program Files");
            if is_installed {
                let cmd = format!(
                    r#"
                    $d = '{}'
                    $p = [Environment]::GetEnvironmentVariable('Path', 'User')
                    $items = if ($p) {{ $p -split ';' }} else {{ @() }}
                    $found = $false
                    foreach ($i in $items) {{
                        if ($i.TrimEnd('\') -ieq $d.TrimEnd('\')) {{ $found = $true; break }}
                    }}
                    if (-not $found) {{
                        $n = if ($p) {{ "$p;$d" }} else {{ $d }}
                        [Environment]::SetEnvironmentVariable('Path', $n, 'User')
                    }}
                    "#,
                    dir_str.replace("'", "''")
                );
                let _ = Command::new("powershell")
                    .args(&["-NoProfile", "-WindowStyle", "Hidden", "-Command", &cmd])
                    .output();
            }
        }
    }
}

#[cfg(windows)]
fn remove_from_user_path(dir: &Path) {
    let dir_str = dir.to_string_lossy();
    let cmd = format!(
        r#"
        $d = '{}'
        $p = [Environment]::GetEnvironmentVariable('Path', 'User')
        if ($p) {{
            $items = $p -split ';' | Where-Object {{ $_ -and ($_.TrimEnd('\') -ine $d.TrimEnd('\')) }}
            $n = $items -join ';'
            [Environment]::SetEnvironmentVariable('Path', $n, 'User')
        }}
        "#,
        dir_str.replace("'", "''")
    );
    let _ = Command::new("powershell")
        .args(&["-NoProfile", "-WindowStyle", "Hidden", "-Command", &cmd])
        .output();
}

#[cfg(not(windows))]
fn ensure_user_path() {}

#[cfg(not(windows))]
fn remove_from_user_path(_dir: &Path) {}

fn main() {
    let args: Vec<String> = env::args().collect();

    // Check for CLI flags
    if args.len() > 1 {
        let flag = args[1].as_str();
        match flag {
            "-u" | "--uninstall" => {
                attach_console();
                handle_uninstall();
                return;
            }
            "-s" | "--status" => {
                attach_console();
                handle_status();
                return;
            }
            "-v" | "--version" => {
                attach_console();
                print_version();
                return;
            }
            "-h" | "--help" | "/?" => {
                attach_console();
                print_help();
                return;
            }
            _ => {
                // If unknown flag starting with '-', show help
                if flag.starts_with('-') {
                    attach_console();
                    println!("Unknown option: {}", flag);
                    print_help();
                    return;
                }
            }
        }
    }

    // Ensure installed directory is in User PATH
    ensure_user_path();

    // Normal GUI execution
    spotglow_lib::run();
}
