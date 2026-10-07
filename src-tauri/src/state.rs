use crate::palette::{Mood, PaletteInfo};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayMode {
    BorderGlow,
    CoverArt,
}

impl Default for DisplayMode {
    fn default() -> Self {
        Self::BorderGlow
    }
}

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
    pub thumbnail_data_url: Option<String>,
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
    pub thumbnail_data_url: Option<String>,
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
            thumbnail_data_url: meta.thumbnail_data_url.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GlowStyle {
    AuroraFlow,
    CyberPulse,
    CometOrbit,
    AudioEq,
    PlasmaStorm,
    ZenProgress,
}

impl GlowStyle {
    pub fn all() -> &'static [GlowStyle] {
        &[
            GlowStyle::AuroraFlow,
            GlowStyle::CyberPulse,
            GlowStyle::CometOrbit,
            GlowStyle::AudioEq,
            GlowStyle::PlasmaStorm,
            GlowStyle::ZenProgress,
        ]
    }

    pub fn id(&self) -> &'static str {
        match self {
            GlowStyle::AuroraFlow => "style_aurora_flow",
            GlowStyle::CyberPulse => "style_cyber_pulse",
            GlowStyle::CometOrbit => "style_comet_orbit",
            GlowStyle::AudioEq => "style_audio_eq",
            GlowStyle::PlasmaStorm => "style_plasma_storm",
            GlowStyle::ZenProgress => "style_zen_progress",
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            GlowStyle::AuroraFlow => "Aurora Flow (Fluid Wave)",
            GlowStyle::CyberPulse => "Cyber Pulse (Neon Beat)",
            GlowStyle::CometOrbit => "Comet Orbit (Dual Celestial)",
            GlowStyle::AudioEq => "Audio EQ (Soundwave Bars)",
            GlowStyle::PlasmaStorm => "Plasma Storm (Liquid Collision)",
            GlowStyle::ZenProgress => "Zen Progress (Minimalist Tracer)",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        match id {
            "style_aurora_flow" | "aurora_flow" => Some(GlowStyle::AuroraFlow),
            "style_cyber_pulse" | "cyber_pulse" => Some(GlowStyle::CyberPulse),
            "style_comet_orbit" | "comet_orbit" => Some(GlowStyle::CometOrbit),
            "style_audio_eq" | "audio_eq" => Some(GlowStyle::AudioEq),
            "style_plasma_storm" | "plasma_storm" => Some(GlowStyle::PlasmaStorm),
            "style_zen_progress" | "zen_progress" => Some(GlowStyle::ZenProgress),
            _ => None,
        }
    }

    pub fn next(&self) -> Self {
        match self {
            GlowStyle::AuroraFlow => GlowStyle::CyberPulse,
            GlowStyle::CyberPulse => GlowStyle::CometOrbit,
            GlowStyle::CometOrbit => GlowStyle::AudioEq,
            GlowStyle::AudioEq => GlowStyle::PlasmaStorm,
            GlowStyle::PlasmaStorm => GlowStyle::ZenProgress,
            GlowStyle::ZenProgress => GlowStyle::AuroraFlow,
        }
    }
}

impl Default for GlowStyle {
    fn default() -> Self {
        Self::AuroraFlow
    }
}

#[derive(Clone, Default)]
pub struct AppState {
    pub current_track: Arc<RwLock<Option<TrackMetadata>>>,
    pub spotify_window: Arc<RwLock<Option<crate::window_tracker::SpotifyWindowState>>>,
    pub window_tracker: Arc<RwLock<Option<crate::window_tracker::WindowTracker>>>,
    pub overlay_controller: Arc<RwLock<Option<crate::overlay::OverlayController>>>,
    pub display_mode: Arc<RwLock<DisplayMode>>,
    pub glow_style: Arc<RwLock<GlowStyle>>,
    pub audio_engine: Arc<RwLock<Option<Arc<crate::audio_reactive::AudioReactiveEngine>>>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            current_track: Arc::new(RwLock::new(None)),
            spotify_window: Arc::new(RwLock::new(None)),
            window_tracker: Arc::new(RwLock::new(None)),
            overlay_controller: Arc::new(RwLock::new(None)),
            display_mode: Arc::new(RwLock::new(DisplayMode::BorderGlow)),
            glow_style: Arc::new(RwLock::new(GlowStyle::default())),
            audio_engine: Arc::new(RwLock::new(None)),
        }
    }
}
