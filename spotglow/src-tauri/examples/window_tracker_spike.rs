use std::sync::Arc;
use spotglow_lib::window_tracker::{SpotifyWindowState, WindowTracker};

fn main() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info,spotglow=debug")),
        )
        .init();

    println!("========================================================");
    println!("        SpotGlow: Phase 3 Window Tracker Spike CLI       ");
    println!("========================================================");
    println!("Tracking Spotify Desktop Window in real time...");
    println!("- Drag, resize, snap (Win+Arrows), minimize, maximize Spotify.");
    println!("- Close and relaunch Spotify to test re-discovery.");
    println!("- Press Enter to exit.\n");

    let on_window = Arc::new(|state: SpotifyWindowState| {
        if state.hwnd != 0 {
            println!(
                "\n>>> [SPOTIFY WINDOW] HWND: 0x{:x} | Bounds: [{}, {}, {}, {}] ({}x{}) | Min: {} | Max: {} | DPI: {}",
                state.hwnd,
                state.rect.left,
                state.rect.top,
                state.rect.right,
                state.rect.bottom,
                state.rect.width,
                state.rect.height,
                state.minimized,
                state.maximized,
                state.dpi,
            );
        } else {
            println!("\n>>> [SPOTIFY WINDOW] Closed / not running.");
        }
    });

    let tracker = WindowTracker::new(Some(on_window));
    tracker.start();

    let mut input = String::new();
    let _ = std::io::stdin().read_line(&mut input);

    println!("\nWindow tracker stopped.");
}
