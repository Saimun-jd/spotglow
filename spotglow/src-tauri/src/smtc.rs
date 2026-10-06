use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use windows::Foundation::TypedEventHandler;
use windows::Media::Control::{
    GlobalSystemMediaTransportControlsSession,
    GlobalSystemMediaTransportControlsSessionManager,
    GlobalSystemMediaTransportControlsSessionPlaybackStatus,
};
use windows::Storage::Streams::DataReader;

use crate::palette::{extract_palette, PaletteInfo};
use crate::state::{AppState, PlaybackStatus, TrackMetadata, TrackTimeline, TrackUpdatePayload};

pub type TrackUpdateCallback = Arc<dyn Fn(TrackUpdatePayload) + Send + Sync>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SmtcEventReason {
    SessionChanged,
    MediaPropsChanged,
    PlaybackInfoChanged,
    TimelinePropertiesChanged,
    PollCheck,
}

pub struct SmtcService {
    app_state: AppState,
    on_update: Option<TrackUpdateCallback>,
    event_tx: mpsc::Sender<SmtcEventReason>,
    _last_processed_version: Arc<AtomicU64>,
}

impl SmtcService {
    pub fn new(app_state: AppState, on_update: Option<TrackUpdateCallback>) -> (Self, mpsc::Receiver<SmtcEventReason>) {
        let (event_tx, event_rx) = mpsc::channel(64);
        (
            Self {
                app_state,
                on_update,
                event_tx,
                _last_processed_version: Arc::new(AtomicU64::new(0)),
            },
            event_rx,
        )
    }

    /// Spawns the SMTC manager listener and polling loop in a dedicated background runtime.
    pub fn start(self, mut event_rx: mpsc::Receiver<SmtcEventReason>) {
        std::thread::spawn(move || {
            let rt = match tokio::runtime::Builder::new_multi_thread()
                .enable_all()
                .build()
            {
                Ok(r) => r,
                Err(e) => {
                    error!("[SMTC] Failed to create dedicated Tokio runtime: {:?}", e);
                    return;
                }
            };

            rt.block_on(async move {
                info!("[SMTC] Initializing Windows SMTC listener for Spotify...");

                // Channel to trigger updates from WinRT event callbacks
                let notify_tx = self.event_tx.clone();

                // Setup WinRT session manager
                let manager_res = tokio::task::spawn_blocking(|| {
                    GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
                        .and_then(|op| op.join())
                })
                .await;

                let manager = match manager_res {
                    Ok(Ok(m)) => m,
                    Ok(Err(e)) => {
                        error!("[SMTC] Failed to get SMTC SessionManager: {:?}", e);
                        return;
                    }
                    Err(e) => {
                        error!("[SMTC] Task join error acquiring SessionManager: {:?}", e);
                        return;
                    }
                };

                info!("[SMTC] SessionManager acquired successfully.");

                // Register SessionsChanged event
                {
                    let notify = notify_tx.clone();
                    let handler = TypedEventHandler::new(move |_manager, _args| {
                        debug!("[SMTC] SessionsChanged event fired");
                        let _ = notify.try_send(SmtcEventReason::SessionChanged);
                        Ok(())
                    });
                    if let Err(e) = manager.SessionsChanged(&handler) {
                        warn!("[SMTC] Failed to register SessionsChanged handler: {:?}", e);
                    }
                }

                // Keep track of currently subscribed session ID & state
                let mut current_session_id: Option<String> = None;
                let mut last_title = String::new();
                let mut last_artist = String::new();
                let mut last_status = PlaybackStatus::Unknown;
                let mut last_position_ms: u64 = 0;
                let mut cached_thumbnail_path: Option<String> = None;
                let mut cached_thumbnail_data_url: Option<String> = None;
                let mut cached_palette: PaletteInfo = PaletteInfo::fallback();

                // Trigger an initial check
                let _ = notify_tx.send(SmtcEventReason::SessionChanged).await;

                // We maintain a periodic poll (1.5s) to guarantee timeline freshness
                // and catch cases if Spotify launches without firing SessionsChanged
                let mut poll_interval = tokio::time::interval(Duration::from_millis(1500));

                loop {
                    tokio::select! {
                        _ = poll_interval.tick() => {
                            let _ = notify_tx.try_send(SmtcEventReason::PollCheck);
                        }
                        reason = event_rx.recv() => {
                            let Some(reason) = reason else {
                                info!("[SMTC] Event channel closed, shutting down SMTC worker.");
                                break;
                            };

                            let manager_clone = manager.clone();
                            let notify_for_session = notify_tx.clone();
                            let active_session_id = current_session_id.clone();
                            let need_thumbnail = cached_thumbnail_path.is_none()
                                || reason == SmtcEventReason::MediaPropsChanged
                                || reason == SmtcEventReason::SessionChanged;

                            let res = tokio::task::spawn_blocking(move || {
                                process_smtc_tick(&manager_clone, &notify_for_session, active_session_id, need_thumbnail)
                            }).await;

                            match res {
                                Ok(Ok(Some((metadata, thumb_bytes, new_session_id)))) => {
                                    current_session_id = new_session_id;

                                    let title_changed = metadata.title != last_title || metadata.artist != last_artist;
                                    let status_changed = metadata.status != last_status;
                                    let position_diff = (metadata.timeline.position_ms as i64 - last_position_ms as i64).abs();

                                    if title_changed || status_changed || reason != SmtcEventReason::PollCheck {
                                        info!(
                                            "[SMTC] Track: \"{}\" by \"{}\" | Album: \"{}\" | Status: {:?} | Pos: {}s / {}s (Event: {:?})",
                                            metadata.title,
                                            metadata.artist,
                                            metadata.album_title,
                                            metadata.status,
                                            metadata.timeline.position_ms / 1000,
                                            metadata.timeline.end_time_ms / 1000,
                                            reason
                                        );
                                    }

                                    if title_changed {
                                        cached_thumbnail_path = None;
                                        cached_thumbnail_data_url = None;
                                    }

                                    // If new thumbnail bytes are available, extract palette, base64 data url, and save to disk
                                    if let Some(bytes) = thumb_bytes {
                                        let palette = extract_palette(&bytes);
                                        info!(
                                            "[Palette] Extracted: Primary: {} | Secondary: {} | Accent: {} | Mood: {:?} (Avg Sat: {:.2}, Avg Bright: {:.2})",
                                            palette.primary.to_hex(),
                                            palette.secondary.to_hex(),
                                            palette.accent.to_hex(),
                                            palette.mood,
                                            palette.avg_saturation,
                                            palette.avg_brightness
                                        );
                                        cached_palette = palette;

                                        use base64::Engine;
                                        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
                                        cached_thumbnail_data_url = Some(format!("data:image/png;base64,{}", b64));

                                        if let Some(path) = save_thumbnail_to_disk(&bytes) {
                                            info!("[SMTC] Album art thumbnail saved to: {}", path.display());
                                            cached_thumbnail_path = Some(path.to_string_lossy().to_string());
                                        }
                                    } else if title_changed && cached_thumbnail_path.is_none() {
                                        cached_palette = PaletteInfo::fallback();
                                        cached_thumbnail_data_url = None;
                                    }

                                    let mut final_metadata = metadata;
                                    final_metadata.thumbnail_path = cached_thumbnail_path.clone();
                                    final_metadata.thumbnail_data_url = cached_thumbnail_data_url.clone();
                                    final_metadata.palette = cached_palette.clone();
                                    final_metadata.mood = cached_palette.mood;

                                    // Update AppState
                                    {
                                        let mut state_guard = self.app_state.current_track.write().await;
                                        *state_guard = Some(final_metadata.clone());
                                    }

                                    // Trigger on_update callback when metadata changes or on significant timeline updates
                                    let should_emit = title_changed
                                        || status_changed
                                        || reason == SmtcEventReason::SessionChanged
                                        || reason == SmtcEventReason::MediaPropsChanged
                                        || reason == SmtcEventReason::PlaybackInfoChanged
                                        || (reason == SmtcEventReason::TimelinePropertiesChanged && position_diff >= 1000);

                                    if should_emit {
                                        if let Some(cb) = &self.on_update {
                                            let payload = TrackUpdatePayload::from(&final_metadata);
                                            cb(payload);
                                        }
                                    }

                                    last_title = final_metadata.title;
                                    last_artist = final_metadata.artist;
                                    last_status = final_metadata.status;
                                    last_position_ms = final_metadata.timeline.position_ms;
                                }
                                Ok(Ok(None)) => {
                                    current_session_id = None;
                                    cached_thumbnail_path = None;
                                    cached_thumbnail_data_url = None;
                                    cached_palette = PaletteInfo::fallback();

                                    if !last_title.is_empty() {
                                        info!("[SMTC] Spotify session ended or not active.");
                                        last_title.clear();
                                        last_artist.clear();
                                        last_status = PlaybackStatus::Unknown;
                                        last_position_ms = 0;

                                        let mut state_guard = self.app_state.current_track.write().await;
                                        *state_guard = None;

                                        if let Some(cb) = &self.on_update {
                                            let empty_payload = TrackUpdatePayload {
                                                title: String::new(),
                                                artist: String::new(),
                                                album_title: String::new(),
                                                status: PlaybackStatus::Closed,
                                                timeline: TrackTimeline::default(),
                                                palette: PaletteInfo::fallback(),
                                                mood: crate::palette::Mood::Balanced,
                                                thumbnail_path: None,
                                                thumbnail_data_url: None,
                                            };
                                            cb(empty_payload);
                                        }
                                    }
                                }
                                Ok(Err(e)) => {
                                    warn!("[SMTC] Error processing SMTC tick: {:?}", e);
                                }
                                Err(e) => {
                                    error!("[SMTC] Join error in SMTC tick: {:?}", e);
                                }
                            }
                        }
                    }
                }
            });
        });
    }
}

/// Discovers the Spotify session, attaches listeners if new, and reads its current state.
fn process_smtc_tick(
    manager: &GlobalSystemMediaTransportControlsSessionManager,
    notify: &mpsc::Sender<SmtcEventReason>,
    active_session_id: Option<String>,
    need_thumbnail: bool,
) -> windows::core::Result<Option<(TrackMetadata, Option<Vec<u8>>, Option<String>)>> {
    let sessions = manager.GetSessions()?;
    let mut spotify_session: Option<(GlobalSystemMediaTransportControlsSession, String)> = None;

    for session in sessions {
        if let Ok(app_id) = session.SourceAppUserModelId() {
            let app_id_str = app_id.to_string();
            if app_id_str.to_lowercase().contains("spotify") {
                spotify_session = Some((session, app_id_str));
                break;
            }
        }
    }

    let Some((session, session_id)) = spotify_session else {
        return Ok(None);
    };

    // If this is a newly discovered session, attach per-session event handlers
    let is_new_session = active_session_id.as_deref() != Some(&session_id);
    if is_new_session {
        info!("[SMTC] Found Spotify session: {}", session_id);

        let notify_media = notify.clone();
        let _ = session.MediaPropertiesChanged(&TypedEventHandler::new(move |_sender, _args| {
            debug!("[SMTC] MediaPropertiesChanged event fired");
            let _ = notify_media.try_send(SmtcEventReason::MediaPropsChanged);
            Ok(())
        }));

        let notify_playback = notify.clone();
        let _ = session.PlaybackInfoChanged(&TypedEventHandler::new(move |_sender, _args| {
            debug!("[SMTC] PlaybackInfoChanged event fired");
            let _ = notify_playback.try_send(SmtcEventReason::PlaybackInfoChanged);
            Ok(())
        }));

        let notify_timeline = notify.clone();
        let _ = session.TimelinePropertiesChanged(&TypedEventHandler::new(move |_sender, _args| {
            debug!("[SMTC] TimelinePropertiesChanged event fired");
            let _ = notify_timeline.try_send(SmtcEventReason::TimelinePropertiesChanged);
            Ok(())
        }));
    }

    // Extract playback status
    let status = if let Ok(info) = session.GetPlaybackInfo() {
        if let Ok(st) = info.PlaybackStatus() {
            match st {
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Playing => {
                    PlaybackStatus::Playing
                }
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Paused => {
                    PlaybackStatus::Paused
                }
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Stopped => {
                    PlaybackStatus::Stopped
                }
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Changing => {
                    PlaybackStatus::Changing
                }
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Closed => {
                    PlaybackStatus::Closed
                }
                GlobalSystemMediaTransportControlsSessionPlaybackStatus::Opened => {
                    PlaybackStatus::Opened
                }
                _ => PlaybackStatus::Unknown,
            }
        } else {
            PlaybackStatus::Unknown
        }
    } else {
        PlaybackStatus::Unknown
    };

    // Extract timeline
    let mut timeline = TrackTimeline::default();
    if let Ok(tl) = session.GetTimelineProperties() {
        if let Ok(pos) = tl.Position() {
            timeline.position_ms = (pos.Duration.max(0) as u64) / 10_000;
        }
        if let Ok(start) = tl.StartTime() {
            timeline.start_time_ms = (start.Duration.max(0) as u64) / 10_000;
        }
        if let Ok(end) = tl.EndTime() {
            timeline.end_time_ms = (end.Duration.max(0) as u64) / 10_000;
        }
    }

    // Extract media properties with retry for thumbnail
    let mut title = String::new();
    let mut artist = String::new();
    let mut album_title = String::new();
    let mut thumb_bytes: Option<Vec<u8>> = None;

    let retry_limit = if need_thumbnail { 5 } else { 1 };

    for attempt in 0..retry_limit {
        if let Ok(props_op) = session.TryGetMediaPropertiesAsync() {
            if let Ok(props) = props_op.join() {
                if let Ok(t) = props.Title() {
                    title = t.to_string();
                }
                if let Ok(a) = props.Artist() {
                    artist = a.to_string();
                }
                if let Ok(alb) = props.AlbumTitle() {
                    album_title = alb.to_string();
                }

                if need_thumbnail {
                    if let Ok(thumb_stream_ref) = props.Thumbnail() {
                        if let Ok(open_op) = thumb_stream_ref.OpenReadAsync() {
                            if let Ok(stream) = open_op.join() {
                                if let Ok(size) = stream.Size() {
                                    if size > 0 {
                                        if let Ok(reader) = DataReader::CreateDataReader(&stream) {
                                            if let Ok(load_op) = reader.LoadAsync(size as u32) {
                                                if load_op.join().is_ok() {
                                                    let mut buf = vec![0u8; size as usize];
                                                    if reader.ReadBytes(&mut buf).is_ok() {
                                                        thumb_bytes = Some(buf);
                                                        break;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                } else {
                    break;
                }
            }
        }

        if thumb_bytes.is_some() || attempt == (retry_limit - 1) || title.is_empty() {
            break;
        }

        std::thread::sleep(Duration::from_millis(300));
    }

    let metadata = TrackMetadata {
        title,
        artist,
        album_title,
        status,
        timeline,
        thumbnail_path: None,
        thumbnail_data_url: None,
        palette: PaletteInfo::fallback(),
        mood: crate::palette::Mood::Balanced,
    };

    Ok(Some((metadata, thumb_bytes, Some(session_id))))
}

/// Decodes album art bytes with the image crate, saves to disk, and returns the path.
fn save_thumbnail_to_disk(bytes: &[u8]) -> Option<PathBuf> {
    info!("[SMTC] Processing thumbnail payload ({} bytes)...", bytes.len());

    let img = match image::load_from_memory(bytes) {
        Ok(decoded) => {
            info!(
                "[SMTC] Successfully decoded album art image: {}x{} ({:?})",
                decoded.width(),
                decoded.height(),
                decoded.color()
            );
            decoded
        }
        Err(e) => {
            warn!("[SMTC] Failed to decode thumbnail with image crate: {:?}", e);
            return None;
        }
    };

    let app_dir = match std::env::var("APPDATA") {
        Ok(appdata) => PathBuf::from(appdata).join("SpotGlow"),
        Err(_) => std::env::temp_dir().join("SpotGlow"),
    };

    if let Err(e) = std::fs::create_dir_all(&app_dir) {
        warn!("[SMTC] Failed to create AppData directory: {:?}", e);
        return None;
    }

    let out_path = app_dir.join("current_thumbnail.png");
    let temp_path = app_dir.join("current_thumbnail.tmp");

    // Write to a temporary file first, flush, then rename for atomic file update
    match std::fs::File::create(&temp_path) {
        Ok(mut file) => {
            if let Err(e) = img.write_to(&mut file, image::ImageFormat::Png) {
                warn!("[SMTC] Failed to encode PNG to temporary file: {:?}", e);
                let _ = std::fs::remove_file(&temp_path);
                return None;
            }
            if let Err(e) = file.flush() {
                warn!("[SMTC] Failed to flush temporary file: {:?}", e);
                let _ = std::fs::remove_file(&temp_path);
                return None;
            }
            drop(file);

            if let Err(e) = std::fs::rename(&temp_path, &out_path) {
                warn!("[SMTC] Failed to rename temp thumbnail to target: {:?}", e);
                // Fallback to direct save
                if let Err(e) = img.save_with_format(&out_path, image::ImageFormat::Png) {
                    warn!("[SMTC] Fallback save failed: {:?}", e);
                    return None;
                }
            }

            Some(out_path)
        }
        Err(e) => {
            warn!("[SMTC] Failed to create temporary file for thumbnail: {:?}", e);
            None
        }
    }
}

/// Dispatches media control commands to Spotify via Windows SMTC
pub async fn send_media_command(command: &str) -> Result<bool, String> {
    let cmd = command.to_string();
    tokio::task::spawn_blocking(move || {
        let manager = GlobalSystemMediaTransportControlsSessionManager::RequestAsync()
            .map_err(|e| format!("Failed to request SMTC manager: {:?}", e))?
            .join()
            .map_err(|e| format!("Failed to join SMTC manager op: {:?}", e))?;

        let sessions = manager.GetSessions().map_err(|e| format!("Failed to get sessions: {:?}", e))?;
        for session in sessions {
            if let Ok(app_id) = session.SourceAppUserModelId() {
                if app_id.to_string().to_lowercase().contains("spotify") {
                    let op_res = match cmd.as_str() {
                        "play_pause" | "toggle" => {
                            session.TryTogglePlayPauseAsync().and_then(|op| op.join())
                        }
                        "play" => {
                            session.TryPlayAsync().and_then(|op| op.join())
                        }
                        "pause" => {
                            session.TryPauseAsync().and_then(|op| op.join())
                        }
                        "next" => {
                            session.TrySkipNextAsync().and_then(|op| op.join())
                        }
                        "previous" => {
                            session.TrySkipPreviousAsync().and_then(|op| op.join())
                        }
                        _ => return Err(format!("Unknown media command: {}", cmd)),
                    };
                    return op_res.map_err(|e| format!("SMTC command failed: {:?}", e));
                }
            }
        }
        Err("Active Spotify SMTC session not found".to_string())
    })
    .await
    .map_err(|e| format!("Task join error: {:?}", e))?
}

