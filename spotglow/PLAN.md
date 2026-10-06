# SpotGlow: Implementation Plan (SMTC-only)

A Windows 10/11 app that draws an animated glow around the edge of the Spotify desktop window, colored and styled by the currently playing song.

**Hard constraint:** the only data source is Windows SMTC (System Media Transport Controls). No Spotify Web API, no OAuth, no dev account, no audio capture.

---

## 1. What SMTC gives us (and what we derive)

| SMTC provides | We derive from it |
|---|---|
| Title, artist, album | Track-change detection |
| Album art thumbnail | Dominant palette, brightness, saturation, "mood" |
| Playback status (playing / paused / stopped) | Glow on / dim / off |
| Timeline (position, start, end) | Progress sweep around the border |
| Source app id (e.g. contains `Spotify`) | Filter the right session |

**"Aesthetic" mapping (art-only):**
- High saturation + bright → fast, vivid, pulsing glow
- Low saturation + dark → slow, soft, breathing glow
- Monochrome art → single-hue glow, low intensity
- Two strong hues → gradient rotating around the border

No beat sync, since that would need audio capture. Motion is time-based animation tuned by the art's mood.

---

## 2. Stack

- **Tauri 2 + Rust** (`windows` crate for Win32 and SMTC)
- **Frontend:** TypeScript + plain canvas/CSS (no framework needed)
- **Crates:** `windows`, `image`, `serde`, `tokio`, `tauri-plugin-autostart`, a small k-means (own implementation or `color_quant`/`palette`)
- **Prereqs:** Rust toolchain, Node 20+, Visual Studio Build Tools (C++), WebView2 runtime

Alternative if Rust friction is high: C# / WPF (SMTC and Win32 are easier there). Keep module boundaries identical.

---

## 3. Architecture

```
┌──────────────┐  events   ┌──────────────┐   IPC    ┌───────────────┐
│ smtc module  │──────────▶│ core/state   │─────────▶│ overlay UI    │
│ (WinRT)      │           │ (track, pal, │          │ (canvas glow) │
└──────────────┘           │  playback)   │          └───────────────┘
┌──────────────┐  events   │              │   cmds   ┌───────────────┐
│ window_track │──────────▶│              │◀─────────│ tray/settings │
│ (WinEvent)   │           └──────────────┘          └───────────────┘
└──────────────┘                  ▲
┌──────────────┐                  │
│ palette      │──────────────────┘
│ (image→color)│
└──────────────┘
```

**Repo layout**
```
spotglow/
  src-tauri/src/
    main.rs
    state.rs          # AppState, event bus
    smtc.rs           # session manager, listeners, thumbnail fetch
    palette.rs        # decode image, k-means, mood metrics
    window_tracker.rs # find Spotify HWND, WinEvent hooks, DWM bounds
    overlay.rs        # ex-styles, z-order, positioning
    settings.rs       # JSON persistence
    tray.rs
  src/
    main.ts           # receives events, drives renderer
    glow.ts           # canvas renderer + animation modes
    styles.css
  README.md
```

---

## 4. Key technical decisions

1. **SMTC session selection:** `GlobalSystemMediaTransportControlsSessionManager::RequestAsync()`, then iterate `GetSessions()` and pick the one whose `SourceAppUserModelId` contains "Spotify". Do not rely only on `GetCurrentSession()` (the user may be playing something else). Subscribe to `SessionsChanged`, and per session `MediaPropertiesChanged`, `PlaybackInfoChanged`, `TimelinePropertiesChanged`.
2. **Thumbnail:** `TryGetMediaPropertiesAsync()` → `Thumbnail()` → `OpenReadAsync()` → read bytes via `DataReader` → decode with `image` crate. The thumbnail can be null or stale right after a track change, so **retry up to ~5 times at 300 ms intervals**, and compare title/artist to avoid applying art for the previous track.
3. **Window bounds:** use `DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` instead of `GetWindowRect` (which includes invisible resize borders).
4. **Finding Spotify's window:** enumerate top-level windows, match process image name `Spotify.exe`, class `Chrome_WidgetWin_1`, visible, and non-empty title (Spotify spawns several helper windows).
5. **Z-order strategy (default = "halo behind"):** place the overlay *directly behind* Spotify using `SetWindowPos(overlay, spotifyHwnd, ..., SWP_NOACTIVATE)`. The glow bleeds outside Spotify's edges and Spotify naturally covers the overlay's middle. This avoids topmost hacks entirely. Optional "inner glow" mode (stretch): insert the overlay after `GetWindow(spotify, GW_HWNDPREV)` to sit just above Spotify.
6. **Click-through:** `WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW`; Tauri config `transparent: true`, `decorations: false`, `skipTaskbar: true`, `shadow: false`, `focus: false`.
7. **DPI:** per-monitor v2 awareness; position the overlay in physical pixels.
8. **Corner radius:** Windows 11 ≈ 8 px (scaled by DPI); Windows 10 = 0.
9. **Performance:** render at ≤ 30 fps by default, pause the render loop when hidden/paused, avoid full-screen blur layers (use a border-sized canvas with a blurred stroke).

---

## 5. Phases

### Phase 0: Scaffold
- Create the Tauri 2 project, module stubs, logging (`tracing`).
- **Done when:** `npm run tauri dev` opens an empty transparent window.

### Phase 1: SMTC spike (CLI-style, logs only)
- Implement `smtc.rs`: pick the Spotify session, print title/artist/status/timeline on every change, save the thumbnail to disk.
- **Done when:** changing songs and pause/play in Spotify prints correct events within ~1 s; thumbnail PNG matches the album art.

### Phase 2: Palette + mood
- Implement `palette.rs`: downscale to ~64×64, k-means (k=5), drop near-black/near-white/low-saturation clusters, rank by `population × saturation`, output `{primary, secondary, accent, avg_brightness, avg_saturation, mood}`.
- Unit-test with a handful of sample images (vivid, dark, monochrome, B&W).
- **Done when:** palettes for the samples look right and the function takes < 30 ms.

### Phase 3: Window tracking
- Implement `window_tracker.rs`: find the HWND, `SetWinEventHook` for `EVENT_OBJECT_LOCATIONCHANGE`, `EVENT_SYSTEM_MINIMIZESTART/END`, `EVENT_OBJECT_DESTROY`, `EVENT_SYSTEM_FOREGROUND`. Debounce, and re-scan when Spotify launches later (poll every 2 s while not found).
- Emit `{hwnd, rect, visible, minimized, maximized, monitor_dpi}`.
- **Done when:** logs follow dragging, resizing, minimizing, closing and relaunching Spotify.

### Phase 4: Overlay window
- Implement `overlay.rs`: apply ex-styles, size the overlay to `rect` + margin (e.g. 48 px), apply z-order behind Spotify, hide on minimize/close.
- Render a static colored rounded-rect glow.
- **Done when:** the glow hugs Spotify's edges while dragging/resizing without visible lag, stays click-through, and doesn't cover other windows.

### Phase 5: Glow renderer + animation
- `glow.ts` modes: **Breathe** (opacity/blur oscillation), **Flow** (conic gradient rotating around the border), **Progress sweep** (bright arc advancing around the border with song position; position extrapolated locally between SMTC updates).
- Mood → parameters (speed, intensity, blur). 800 ms color crossfade on track change. Paused → dim to ~15 % then fade out after 10 s (configurable).
- **Done when:** track changes crossfade smoothly; pause/resume behaves; CPU usage stays low (see targets).

### Phase 6: Tray + settings
- Tray menu: enable/disable, mode, intensity, thickness, hide-when-paused, start with Windows, quit. A small settings panel is optional. Persist to `%APPDATA%/SpotGlow/settings.json`.
- **Done when:** settings persist across restarts and apply live.

### Phase 7: Edge cases and hardening
- Maximized Spotify: glow the monitor's screen edges (option).
- Multi-monitor and mixed DPI; window moved across monitors.
- Spotify Microsoft Store build vs standalone installer (different process/AUMID); test both.
- SMTC unavailable or no thumbnail → fall back to the last palette or a default accent.
- Other media apps playing: ignore non-Spotify sessions.
- Crash safety: never leave a ghost overlay when Spotify exits.

### Phase 8: Packaging
- Build an installer (NSIS/MSI via Tauri bundler), optional autostart, README with screenshots/GIF, known limitations.

---

## 6. Test checklist (manual)

- [ ] Start app before Spotify; start Spotify after
- [ ] Play / pause / skip / seek; shuffle through very different albums
- [ ] Drag, resize, snap (Win+Arrows), minimize, maximize, close
- [ ] Move Spotify between monitors with different DPI
- [ ] Open other windows over Spotify (overlay must not show above them)
- [ ] Play music in a browser while Spotify is paused (must not react)
- [ ] Track with no/placeholder art (podcast, local file)
- [ ] 1 hour idle: memory stable, CPU low

**Targets:** < 3 % CPU while animating, < 150 MB RAM, color change within 1.5 s of a track change.

---

## 7. Known risks

| Risk | Mitigation |
|---|---|
| Thumbnail late/stale after track change | Retry loop + title/artist match check |
| Spotify has multiple windows | Filter by class + visible + title |
| Transparent WebView2 flicker | Fixed overlay size, no resizing per frame, hide/show instead of destroy/create |
| Z-order glitches | Default to behind-Spotify halo mode; re-apply on foreground events only |
| Win10 has no rounded corners | Detect OS build, set radius 0 |
