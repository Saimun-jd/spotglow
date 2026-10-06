use crate::palette::{Mood, PaletteInfo};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum PlaybackStatus {
    Playing,
    Paused,
    Stopped,
    Changing,
    Closed,
    Opened,
    Unknown,
}

impl Default for PlaybackStatus {
    fn default() -> Self {
        Self::Unknown
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackTimeline {
    pub position_ms: u64,
    pub start_time_ms: u64,
    pub end_time_ms: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TrackMetadata {
    pub title: String,
    pub artist: String,
    pub album_title: String,
    pub status: PlaybackStatus,
    pub timeline: TrackTimeline,
    pub thumbnail_path: Option<String>,
    pub palette: PaletteInfo,
    pub mood: Mood,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TrackUpdatePayload {
    pub title: String,
    pub artist: String,
    pub album_title: String,
    pub status: PlaybackStatus,
    pub timeline: TrackTimeline,
    pub palette: PaletteInfo,
    pub mood: Mood,
    pub thumbnail_path: Option<String>,
}

impl From<&TrackMetadata> for TrackUpdatePayload {
    fn from(meta: &TrackMetadata) -> Self {
        Self {
            title: meta.title.clone(),
            artist: meta.artist.clone(),
            album_title: meta.album_title.clone(),
            status: meta.status,
            timeline: meta.timeline.clone(),
            palette: meta.palette.clone(),
            mood: meta.mood,
            thumbnail_path: meta.thumbnail_path.clone(),
        }
    }
}

#[derive(Clone, Default)]
pub struct AppState {
    pub current_track: Arc<RwLock<Option<TrackMetadata>>>,
    pub spotify_window: Arc<RwLock<Option<crate::window_tracker::SpotifyWindowState>>>,
    pub window_tracker: Arc<RwLock<Option<crate::window_tracker::WindowTracker>>>,
    pub overlay_controller: Arc<RwLock<Option<crate::overlay::OverlayController>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            current_track: Arc::new(RwLock::new(None)),
            spotify_window: Arc::new(RwLock::new(None)),
            window_tracker: Arc::new(RwLock::new(None)),
            overlay_controller: Arc::new(RwLock::new(None)),
        }
    }
}
