use tauri::menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem, Submenu};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Emitter};
use tracing::info;

use crate::state::{AppState, DisplayMode, GlowStyle};

pub fn setup_tray(app: &AppHandle, app_state: AppState) -> Result<(), Box<dyn std::error::Error>> {
    let toggle_item = MenuItem::with_id(app, "toggle_mode", "Toggle Cover Focus Mode", true, None::<&str>)?;

    // Glow Style Submenu
    let current_style = { *app_state.glow_style.blocking_read() };
    let item_aurora = CheckMenuItem::with_id(app, "style_aurora_flow", "Aurora Flow (Fluid Wave)", true, current_style == GlowStyle::AuroraFlow, None::<&str>)?;
    let item_pulse = CheckMenuItem::with_id(app, "style_cyber_pulse", "Cyber Pulse (Neon Beat)", true, current_style == GlowStyle::CyberPulse, None::<&str>)?;
    let item_comet = CheckMenuItem::with_id(app, "style_comet_orbit", "Comet Orbit (Dual Celestial)", true, current_style == GlowStyle::CometOrbit, None::<&str>)?;
    let item_eq = CheckMenuItem::with_id(app, "style_audio_eq", "Audio EQ (Soundwave Bars)", true, current_style == GlowStyle::AudioEq, None::<&str>)?;
    let item_plasma = CheckMenuItem::with_id(app, "style_plasma_storm", "Plasma Storm (Liquid Collision)", true, current_style == GlowStyle::PlasmaStorm, None::<&str>)?;
    let item_zen = CheckMenuItem::with_id(app, "style_zen_progress", "Zen Progress (Minimalist Tracer)", true, current_style == GlowStyle::ZenProgress, None::<&str>)?;

    let style_submenu = Submenu::with_items(
        app,
        "Glow Animation Style",
        true,
        &[
            &item_aurora,
            &item_pulse,
            &item_comet,
            &item_eq,
            &item_plasma,
            &item_zen,
        ],
    )?;

    let play_pause_item = MenuItem::with_id(app, "media_toggle", "Play / Pause", true, None::<&str>)?;
    let next_item = MenuItem::with_id(app, "media_next", "Next Track", true, None::<&str>)?;
    let prev_item = MenuItem::with_id(app, "media_prev", "Previous Track", true, None::<&str>)?;
    let quit_item = MenuItem::with_id(app, "quit", "Quit SpotGlow", true, None::<&str>)?;

    let menu = Menu::with_items(app, &[
        &toggle_item,
        &style_submenu,
        &PredefinedMenuItem::separator(app)?,
        &play_pause_item,
        &next_item,
        &prev_item,
        &PredefinedMenuItem::separator(app)?,
        &quit_item,
    ])?;

    let state_menu = app_state.clone();
    let state_click = app_state.clone();
    let handle_click = app.clone();

    let _tray = TrayIconBuilder::new()
        .icon(app.default_window_icon().unwrap().clone())
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(move |app, event| {
            let id = event.id().as_ref();
            if let Some(style) = GlowStyle::from_id(id) {
                let state = state_menu.clone();
                let handle = app.clone();
                tauri::async_runtime::spawn(async move {
                    set_glow_style_explicit(&state, &handle, style).await;
                });
                return;
            }

            match id {
                "toggle_mode" => {
                    let state = state_menu.clone();
                    let handle = app.clone();
                    tauri::async_runtime::spawn(async move {
                        toggle_display_mode(&state, &handle).await;
                    });
                }
                "media_toggle" => {
                    tauri::async_runtime::spawn(async {
                        let _ = crate::smtc::send_media_command("toggle").await;
                    });
                }
                "media_next" => {
                    tauri::async_runtime::spawn(async {
                        let _ = crate::smtc::send_media_command("next").await;
                    });
                }
                "media_prev" => {
                    tauri::async_runtime::spawn(async {
                        let _ = crate::smtc::send_media_command("previous").await;
                    });
                }
                "quit" => {
                    let spotify_guard = state_menu.spotify_window.blocking_read();
                    if let Some(win) = spotify_guard.as_ref() {
                        unsafe {
                            crate::overlay::restore_spotify_window(windows::Win32::Foundation::HWND(win.hwnd as *mut _));
                        }
                    }
                    app.exit(0);
                }
                _ => {}
            }
        })
        .on_tray_icon_event(move |_tray, event| {
            if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
                let state = state_click.clone();
                let handle = handle_click.clone();
                tauri::async_runtime::spawn(async move {
                    toggle_display_mode(&state, &handle).await;
                });
            }
        })
        .build(app)?;

    info!("[Tray] System tray icon initialized successfully.");
    Ok(())
}

pub async fn set_glow_style_explicit(state: &AppState, app: &AppHandle, target_style: GlowStyle) {
    {
        let mut guard = state.glow_style.write().await;
        *guard = target_style;
    }
    let _ = app.emit("glow_style_changed", &target_style);
    info!("[SpotGlow] Glow style changed to {:?}", target_style);
}

pub async fn cycle_glow_style(state: &AppState, app: &AppHandle) -> GlowStyle {
    let next_style = {
        let guard = state.glow_style.read().await;
        guard.next()
    };
    set_glow_style_explicit(state, app, next_style).await;
    next_style
}

pub async fn set_mode_explicit(state: &AppState, app: &AppHandle, target_mode: DisplayMode) {
    {
        let mut guard = state.display_mode.write().await;
        *guard = target_mode;
    }

    let spotify_hwnd = {
        let guard = state.spotify_window.read().await;
        guard.as_ref().map(|w| windows::Win32::Foundation::HWND(w.hwnd as *mut _))
    };

    match target_mode {
        DisplayMode::CoverArt => {
            crate::window_tracker::SPOTIFY_DELIBERATELY_HIDDEN.store(true, std::sync::atomic::Ordering::SeqCst);
            if let Some(hwnd) = spotify_hwnd {
                unsafe {
                    crate::overlay::hide_spotify_window(hwnd);
                }
            }
        }
        DisplayMode::BorderGlow => {
            crate::window_tracker::SPOTIFY_DELIBERATELY_HIDDEN.store(false, std::sync::atomic::Ordering::SeqCst);
            if let Some(hwnd) = spotify_hwnd {
                unsafe {
                    crate::overlay::restore_spotify_window(hwnd);
                }
            }
        }
    }

    if let Some(controller) = state.overlay_controller.read().await.as_ref() {
        controller.set_display_mode(target_mode);
        if let Some(win_state) = state.spotify_window.read().await.as_ref() {
            controller.reposition_behind(win_state);
        }
    }

    let _ = app.emit("display_mode_changed", &target_mode);
    info!("[SpotGlow] Mode switched to {:?} (Spotify hidden: {:?})", target_mode, target_mode == DisplayMode::CoverArt);
}

pub async fn toggle_display_mode(state: &AppState, app: &AppHandle) {
    let cur_mode = { *state.display_mode.read().await };
    let new_mode = match cur_mode {
        DisplayMode::BorderGlow => DisplayMode::CoverArt,
        DisplayMode::CoverArt => DisplayMode::BorderGlow,
    };
    set_mode_explicit(state, app, new_mode).await;
}
