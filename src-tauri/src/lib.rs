pub mod overlay;
pub mod palette;
pub mod settings;
pub mod smtc;
pub mod state;
pub mod tray;
pub mod window_tracker;

use std::sync::Arc;
use state::{AppState, TrackUpdatePayload};
use smtc::SmtcService;
use tauri::Emitter;
use tracing::{info, warn};
use window_tracker::{SpotifyWindowState, WindowTracker};

#[tauri::command]
async fn get_spotify_window(state: tauri::State<'_, AppState>) -> Result<SpotifyWindowState, String> {
    let guard = state.spotify_window.read().await;
    Ok(guard.clone().unwrap_or_default())
}

#[tauri::command]
async fn get_current_track(state: tauri::State<'_, AppState>) -> Result<Option<TrackUpdatePayload>, String> {
    let guard = state.current_track.read().await;
    Ok(guard.as_ref().map(TrackUpdatePayload::from))
}

#[tauri::command]
async fn get_display_mode(state: tauri::State<'_, AppState>) -> Result<state::DisplayMode, String> {
    let guard = state.display_mode.read().await;
    Ok(*guard)
}

#[tauri::command]
async fn set_display_mode(
    mode: state::DisplayMode,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    tray::set_mode_explicit(&state, &app_handle, mode).await;
    Ok(())
}

#[tauri::command]
async fn get_glow_style(state: tauri::State<'_, AppState>) -> Result<state::GlowStyle, String> {
    let guard = state.glow_style.read().await;
    Ok(*guard)
}

#[tauri::command]
async fn set_glow_style(
    style: state::GlowStyle,
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<(), String> {
    tray::set_glow_style_explicit(&state, &app_handle, style).await;
    Ok(())
}

#[tauri::command]
async fn cycle_glow_style(
    state: tauri::State<'_, AppState>,
    app_handle: tauri::AppHandle,
) -> Result<state::GlowStyle, String> {
    Ok(tray::cycle_glow_style(&state, &app_handle).await)
}

#[tauri::command]
async fn media_play_pause() -> Result<bool, String> {
    smtc::send_media_command("toggle").await
}

#[tauri::command]
async fn media_next() -> Result<bool, String> {
    smtc::send_media_command("next").await
}

#[tauri::command]
async fn media_previous() -> Result<bool, String> {
    smtc::send_media_command("previous").await
}

#[tauri::command]
async fn save_canvas_snapshot(data_url: String, metrics: String) -> Result<(), String> {
    info!("[SpotGlow Frontend Metrics] {}", metrics);
    if let Some(base64_str) = data_url.strip_prefix("data:image/png;base64,") {
        use base64::Engine;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(base64_str) {
            let path = std::path::PathBuf::from(r"C:\Users\user\.gemini\antigravity-ide\brain\5c87f282-46a7-4608-96e7-bf0b9f449e3f\spotglow_canvas.png");
            if let Err(e) = std::fs::write(&path, &bytes) {
                warn!("[SpotGlow] Failed to write canvas snapshot: {:?}", e);
            } else {
                info!("[SpotGlow] Successfully saved canvas snapshot to {:?} ({} bytes)", path, bytes.len());
            }
        }
    }
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // Initialize tracing logger
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,spotglow=debug")),
        )
        .init();

    info!("[SpotGlow] Starting application...");

    let app_state = AppState::new();

    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .manage(app_state.clone())
        .invoke_handler(tauri::generate_handler![
            get_spotify_window,
            get_current_track,
            save_canvas_snapshot,
            get_display_mode,
            set_display_mode,
            get_glow_style,
            set_glow_style,
            cycle_glow_style,
            media_play_pause,
            media_next,
            media_previous
        ])
        .setup({
            let app_state = app_state.clone();
            move |app| {
                let app_handle = app.handle().clone();

                // Bridge SMTC track updates to Tauri "track_update" frontend event
                let handle_for_track = app_handle.clone();
                let on_track_update = Arc::new(move |payload: TrackUpdatePayload| {
                    if let Err(e) = handle_for_track.emit("track_update", &payload) {
                        warn!("[SpotGlow] Failed to emit track_update event: {:?}", e);
                    } else {
                        tracing::debug!("[SpotGlow] Emitted track_update event to webview");
                    }
                });

                let (smtc_service, smtc_rx) = SmtcService::new(app_state.clone(), Some(on_track_update));
                smtc_service.start(smtc_rx);

                use tauri::Manager;
                let main_window = app.get_webview_window("main").expect("failed to get main window");
                let overlay_hwnd = main_window.hwnd().expect("failed to get overlay HWND");

                // Enable click-through natively in Tauri without breaking DirectComposition
                if let Err(e) = main_window.set_ignore_cursor_events(true) {
                    warn!("[SpotGlow] Failed to set_ignore_cursor_events: {:?}", e);
                } else {
                    info!("[SpotGlow] Enabled cursor passthrough on overlay window.");
                }

                let overlay_controller = overlay::OverlayController::new(overlay_hwnd, Some(main_window.clone()));
                overlay_controller.apply_styles();

                // Store overlay_controller in app_state
                {
                    let mut guard = app_state.overlay_controller.blocking_write();
                    *guard = Some(overlay_controller.clone());
                }

                // Bridge Spotify window events to overlay positioning and Tauri "spotify_window" frontend event
                let overlay_ctrl = overlay_controller.clone();
                let handle_for_win = app_handle.clone();
                let state_for_win = app_state.clone();
                let on_window_event = Arc::new(move |state: SpotifyWindowState| {
                    // Update shared state for invoke queries
                    {
                        let mut guard = state_for_win.spotify_window.blocking_write();
                        *guard = Some(state.clone());
                    }

                    // Instantly reposition overlay window behind Spotify in Z-order
                    overlay_ctrl.reposition_behind(&state);

                    if let Err(e) = handle_for_win.emit("spotify_window", &state) {
                        warn!("[SpotGlow] Failed to emit spotify_window event: {:?}", e);
                    } else {
                        tracing::debug!("[SpotGlow] Emitted spotify_window event to webview");
                    }
                });

                let tracker = WindowTracker::new(Some(on_window_event));
                tracker.start();

                // Store tracker in app_state so it is NOT dropped at the end of setup!
                {
                    let mut guard = app_state.window_tracker.blocking_write();
                    *guard = Some(tracker);
                }

                // Initialize system tray
                if let Err(e) = tray::setup_tray(&app_handle, app_state.clone()) {
                    warn!("[SpotGlow] Failed to setup system tray: {:?}", e);
                }

                // Global hotkey background thread:
                // - Ctrl + Shift + G or F9: Toggle Mode (Border Glow <-> Cover Art Focus)
                // - Ctrl + Shift + A or F8: Cycle Glow Animation Style (Aurora -> Pulse -> Comet -> EQ -> Plasma -> Zen)
                let hotkey_state = app_state.clone();
                let hotkey_handle = app_handle.clone();
                std::thread::spawn(move || {
                    use windows::Win32::UI::Input::KeyboardAndMouse::{
                        RegisterHotKey, UnregisterHotKey, HOT_KEY_MODIFIERS, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT,
                    };
                    use windows::Win32::UI::WindowsAndMessaging::{GetMessageW, MSG, WM_HOTKEY};

                    unsafe {
                        let _ = RegisterHotKey(None, 101, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, 0x47 /* 'G' */);
                        let _ = RegisterHotKey(None, 102, HOT_KEY_MODIFIERS(0) | MOD_NOREPEAT, 0x78 /* VK_F9 */);
                        let _ = RegisterHotKey(None, 103, MOD_CONTROL | MOD_SHIFT | MOD_NOREPEAT, 0x41 /* 'A' */);
                        let _ = RegisterHotKey(None, 104, HOT_KEY_MODIFIERS(0) | MOD_NOREPEAT, 0x77 /* VK_F8 */);

                        let mut msg = MSG::default();
                        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
                            if msg.message == WM_HOTKEY {
                                match msg.wParam.0 {
                                    101 | 102 => {
                                        let state = hotkey_state.clone();
                                        let handle = hotkey_handle.clone();
                                        tauri::async_runtime::spawn(async move {
                                            tray::toggle_display_mode(&state, &handle).await;
                                        });
                                    }
                                    103 | 104 => {
                                        let state = hotkey_state.clone();
                                        let handle = hotkey_handle.clone();
                                        tauri::async_runtime::spawn(async move {
                                            tray::cycle_glow_style(&state, &handle).await;
                                        });
                                    }
                                    _ => {}
                                }
                            }
                        }
                        let _ = UnregisterHotKey(None, 101);
                        let _ = UnregisterHotKey(None, 102);
                        let _ = UnregisterHotKey(None, 103);
                        let _ = UnregisterHotKey(None, 104);
                    }
                });

                Ok(())
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
