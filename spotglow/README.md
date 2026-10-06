# SpotGlow

A Windows 10/11 app that draws an animated glow around the edge of the Spotify desktop window, colored and styled by the currently playing song.

**Hard constraint:** Windows SMTC (`GlobalSystemMediaTransportControlsSessionManager`) is the exclusive data source. No Spotify Web API, no OAuth, no dev account, no audio capture.

---

## Architecture & Repo Layout

```
spotglow/
  src-tauri/
    src/
      main.rs           # Windows subsystem entrypoint
      lib.rs            # Tauri 2 app builder, tracing init, event bridging
      state.rs          # AppState (TrackMetadata, PlaybackStatus, TrackUpdatePayload)
      smtc.rs           # WinRT session manager, event listeners, thumbnail fetch
      palette.rs        # K-means clustering (k=5), HSL scoring, mood metrics
      window_tracker.rs # Spotify HWND finder & WinEvent hooks stub (Phase 3)
      overlay.rs        # Click-through overlay & DWM bounds stub (Phase 4)
      settings.rs       # Settings persistence stub (Phase 6)
      tray.rs           # Tray menu stub (Phase 6)
    examples/
      smtc_spike.rs     # Standalone CLI runner to test SMTC + Palette integration
  src/
    main.ts             # Frontend event listener for "track_update" & renderer setup
    glow.ts             # Transparent canvas renderer
    styles.css          # Click-through, borderless transparent styles
  PLAN.md               # Master phase implementation plan
```

---

## How to Run & Verify

### Running Unit Tests (Palette extraction & mood)
```powershell
cd spotglow/src-tauri
cargo test palette -- --nocapture
```
Runs 5 automated unit tests covering:
- Vivid / neon palette extraction
- Dark / mellow palette extraction
- Pure monochrome / B&W handling
- Dual-tone complementary hue detection
- Performance benchmark (< 30 ms target; completes in ~26 ms)

### Phase 1 & 2 SMTC + Palette CLI Spike
Run the standalone CLI spike in your terminal:
```powershell
cd spotglow/src-tauri
cargo run --example smtc_spike
```
1. Play, pause, or switch songs in Spotify.
2. Observe real-time console updates:
   ```text
   >>> [TRACK UPDATE] "Road Back Home" by "Zakk Wylde" [Paused] | Mood: Mellow
       Palette -> Primary: #904d38 | Secondary: #d5884e | Accent: #d5884e
       Avg Sat: 0.17 | Avg Brightness: 0.15
   ```
3. Extracted album art is saved to `%APPDATA%\SpotGlow\current_thumbnail.png` and `.\current_thumbnail.png`.

### Running the Full Tauri Application
```powershell
cd spotglow
npm run tauri dev
```
Launches the transparent overlay window and listens for `"track_update"` events via the Tauri IPC bridge.
