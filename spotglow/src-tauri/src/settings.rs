// App settings & persistence stub (Phase 6 implementation)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    pub enabled: bool,
    pub mode: String,
    pub intensity: f32,
    pub thickness: f32,
    pub hide_when_paused: bool,
    pub autostart: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            mode: "flow".to_string(),
            intensity: 1.0,
            thickness: 8.0,
            hide_when_paused: false,
            autostart: false,
        }
    }
}
