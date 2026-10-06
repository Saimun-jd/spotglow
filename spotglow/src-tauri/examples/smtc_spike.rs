use std::sync::Arc;
use spotglow_lib::smtc::SmtcService;
use spotglow_lib::state::{AppState, TrackUpdatePayload};

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,spotglow=debug")),
        )
        .init();

    println!("========================================================");
    println!("           SpotGlow: Phase 1 & 2 SMTC + Palette Spike   ");
    println!("========================================================");
    println!("Monitoring Windows SMTC for Spotify sessions...");
    println!("- Play / pause or skip tracks in Spotify to test.");
    println!("- Real-time palette extraction & mood classification.");
    println!("- Thumbnails saved to %APPDATA%\\SpotGlow and .\\current_thumbnail.png");
    println!("- Press Ctrl+C to exit.\n");

    let app_state = AppState::new();

    let on_update = Arc::new(|payload: TrackUpdatePayload| {
        if !payload.title.is_empty() {
            println!(
                "\n>>> [TRACK UPDATE] \"{}\" by \"{}\" [{:?}] | Mood: {:?}\n    Palette -> Primary: {} | Secondary: {} | Accent: {}\n    Avg Sat: {:.2} | Avg Brightness: {:.2}\n",
                payload.title,
                payload.artist,
                payload.status,
                payload.mood,
                payload.palette.primary.to_hex(),
                payload.palette.secondary.to_hex(),
                payload.palette.accent.to_hex(),
                payload.palette.avg_saturation,
                payload.palette.avg_brightness,
            );
        }
    });

    let (service, rx) = SmtcService::new(app_state, Some(on_update));
    service.start(rx);

    tokio::signal::ctrl_c()
        .await
        .expect("Failed to listen for Ctrl+C");

    println!("\nSMTC spike stopped.");
}
