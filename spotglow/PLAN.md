# SpotGlow: Implementation Plan (SMTC-only)

A Windows 10/11 app that draws an animated glow around the Spotify desktop window, colored and styled by the currently playing song, with an extensible presentation system (Transparent Border Glow, Cover Art Focus Shroud, and Now Playing Card).

**Hard constraint:** The only data source and control interface is Windows SMTC (`SystemMediaTransportControls`). No Spotify Web API, no OAuth, no dev account, no audio capture.

---

## 1. Core Vision & Extensible Presentation Modes

SpotGlow operates in multiple selectable presentation modes:

| Mode | Visual Presentation | Z-Order / Placement | Cursor Behavior |
|---|---|---|---|
| **1. Ambient Border Halo** *(Current Default)* | Transparent center. Sleek thin neon ambient glow hugging Spotify's outer perimeter (or screen perimeter when maximized). Spotify's UI is fully visible. | Behind Spotify when windowed (+18px margin); in front of Spotify when maximized (inner edge). | 100% click-through (`set_ignore_cursor_events: true`). |
| **2. Cover Art Focus** *(The Shroud Mode)* | Completely covers and conceals the busy Spotify UI behind a frosted acrylic / darkened blurred album art backdrop. Features high-res album cover, track/artist typography, progress timeline, and music glow radiating from the artwork. | In front of Spotify (`find_window_above` / `HWND_TOP`), matching Spotify's exact window bounds. | Interactive on hover (Play/Pause, Skip, Seek controls via SMTC; flip-to-halo button); passthrough elsewhere. |
| **3. Compact Card / Mini Player** *(Optional Stretch)* | Floating minimalist Now Playing card widget that can dock to Spotify's position or float independently when Spotify is minimized. | Always-on-top or attached to Spotify. | Interactive on hover. |

---

## 2. What SMTC Provides (Two-Way Pipeline)

### Inbound (Read State):
| SMTC Data | Derived Feature |
|---|---|
| Title, artist, album | Track display, font sizing, crossfade trigger |
| Album art thumbnail stream | High-res album cover display, dominant palette extraction (k-means) |
| Playback status (Playing, Paused, Stopped) | Glow animation state, play/pause icon toggle, dimming |
| Timeline (Position, Start, End) | Live progress bar / perimeter sweep |
| Source app id (`Spotify.exe`) | Filtering out non-Spotify media |

### Outbound (Two-Way Control via WinRT SMTC):
Windows `GlobalSystemMediaTransportControlsSession` supports native playback commands without Spotify API tokens:
- `session.TryPlayAsync()` / `session.TryPauseAsync()` / `session.TryTogglePlayPauseAsync()`
- `session.TrySkipNextAsync()` / `session.TrySkipPreviousAsync()`
- `session.TryChangePlaybackPositionAsync(position_100ns)`

---

## 3. Extensible System Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     WINDOWS OPERATING SYSTEM                    │
│   ┌──────────────────────────┐    ┌─────────────────────────┐   │
│   │       Spotify.exe        │    │  WinRT SMTC Controller  │   │
│   │ (Chrome_WidgetWin_1 HWND)│    │ (Media Metadata & Cmds) │   │
│   └─────────────┬────────────┘    └────────────┬────────────┘   │
└─────────────────┼──────────────────────────────┼────────────────┘
                  │ WinEvent Hooks               │ Two-Way WinRT IPC
                  ▼                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     SPOTGLOW RUST BACKEND                       │
│                                                                 │
│  ┌───────────────────────┐         ┌─────────────────────────┐  │
│  │     WindowTracker     │         │       SmtcService       │  │
│  │ (DWM bounds, state,   │         │ (Events: track, time    │  │
│  │  z-order, maximize)   │         │  Commands: play, skip)  │  │
│  └───────────┬───────────┘         └────────────┬────────────┘  │
│              │                                  │               │
│              ▼                                  │               │
│  ┌───────────────────────┐                      │               │
│  │   OverlayController   │                      │               │
│  │ (Behind / Front mode, │                      │               │
│  │  hit-test, styles)    │                      │               │
│  └───────────┬───────────┘                      │               │
│              │                                  │               │
│              ▼                                  ▼               │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │            Tauri Event Bus & Command Handlers             │  │
│  │ (spotify_window, track_update, toggle_mode, media_ctrl)   │  │
│  └───────────────────────────┬───────────────────────────────┘  │
└──────────────────────────────┼──────────────────────────────────┘
                               │ IPC Events & Commands
                               ▼
┌─────────────────────────────────────────────────────────────────┐
│                    SPOTGLOW WEBVIEW2 FRONTEND                   │
│                                                                 │
│  ┌───────────────────────────────────────────────────────────┐  │
│  │                        App Root                           │  │
│  │   - Mode Switcher State ("border_halo" vs "cover_art")    │  │
│  └───────────────┬───────────────────────────┬───────────────┘  │
│                  │                           │                  │
│                  ▼                           ▼                  │
│  ┌───────────────────────────┐   ┌───────────────────────────┐  │
│  │     GlowCanvas Layer      │   │    CoverArtShroud Layer   │  │
│  │ (Perimeter glow, breathe, │   │ (Frosted backdrop, cover, │  │
│  │  flow, progress sweep)    │   │  track info, controls)    │  │
│  └───────────────────────────┘   └───────────────────────────┘  │
└─────────────────────────────────────────────────────────────────┘
```

---

## 4. Detailed Specification: Cover Art Focus Mode

### 4.1 Visual Design & Aesthetics
1. **Backdrop Shroud**:
   - Covers Spotify's client rectangle.
   - Glassmorphic dark acrylic (`rgba(15, 15, 20, 0.88)` with `backdrop-filter: blur(28px)`).
   - Subtle background radial gradient tinted with the current song's primary & secondary palette colors.
2. **Featured Album Artwork**:
   - Square artwork card centered (or left-aligned for wide layouts) with smooth 16px corner rounding, deep ambient shadow tinted with album accent color.
   - Smooth 600ms crossfade animation on track change.
   - Optional subtle floating/breathing scale animation ($1.00 \to 1.02$) synced to music playback.
3. **Track & Artist Typography**:
   - Title in clean modern sans-serif (e.g. Outfit/Inter, font-size responsive to window width).
   - Artist & Album subtitle with muted secondary tint.
4. **Interactive Media Bar (On Hover or Pinned)**:
   - Previous Track, Play/Pause, Next Track buttons.
   - Scannable progress bar showing elapsed/total time with hover seek.
   - Quick "Flip to Border Glow" icon button in the corner to toggle back to transparent halo mode.
5. **Perimeter Glow Integration**:
   - The outer ambient glow border remains active around the shroud edges, smoothly matching the album palette.

### 4.2 Window & Input Handling in Cover Art Mode
1. **Z-Order Placement**:
   - In Border Glow mode: Placed **Behind** Spotify (windowed) or **In Front** (maximized).
   - In Cover Art mode: Placed **In Front** of Spotify at all times (`find_window_above(spotify_hwnd)` or `HWND_TOP`), exactly matching Spotify's `rect`.
2. **Hit-Testing & Click Passthrough**:
   - When Cover Art mode is active:
     - By default, clicks inside the shroud are captured by the webview so the user can interact with Play/Pause, Next, Previous, and Mode Toggle.
     - Clicking the "Flip to Border Glow" button immediately sets `set_ignore_cursor_events(true)` and moves the window back behind Spotify.

---

## 5. Phase-by-Phase Roadmap

### Phase 5.5: Cover Art Focus UI & View Switcher (Completed)
- [x] Refine thin border glow renderer ($2\text{ px}$ core, $3\text{ px}$ blur, $18\text{ px}$ margin).
- [x] Fix maximized window support (inner edge mode with front Z-order).
- [x] Add Cover Art Focus Shroud layer in frontend:
  - Frosted acrylic backdrop (`backdrop-filter: blur(40px) saturate(180%)`) covering Spotify's UI.
  - High-res album artwork card with dynamic colored ambient drop-shadow.
  - Equalizer animation badge, song title, artist, album, live scrubber timeline.
- [x] Cover Art Focus Mode Enhancements:
  - Made the Spotify window completely invisible (`SW_HIDE` + `alpha = 0`) while preserving full SMTC and audio playback.
  - Full-bleed **Creative Color Aurora Engine**: dynamic morphing background mesh with multiple colored orbs (`--primary-rgb`, `--secondary-rgb`, `--accent-rgb`, `--tertiary-rgb`), ambient blur, film grain texture, and chromatic album aura glow.
  - Automatic restore of Spotify window on toggle back to Border Glow or app quit.

### Phase 6: Two-Way SMTC Media Controls & Settings Engine (Completed)
- [x] Expand `smtc.rs` to support outbound media commands via Windows WinRT:
  - `send_media_command("toggle")`, `send_media_command("next")`, `send_media_command("previous")`.
- [x] Expose Tauri IPC commands: `media_play_pause`, `media_next`, `media_previous`.
- [x] Connect interactive buttons in Cover Art UI and Spacebar shortcut to live playback commands.
- [x] Base64 album art thumbnail streaming (`thumbnail_data_url`).

### Phase 7: System Tray & Global Shortcuts (Completed)
- [x] Build Windows System Tray menu:
  - Left-click tray icon toggles mode (`Border Glow` $\leftrightarrow$ `Cover Art Focus`).
  - Right-click menu: Toggle Cover Focus Mode, Play/Pause, Next Track, Previous Track, Quit SpotGlow.
- [x] Global Windows Hotkey thread:
  - `Ctrl + Shift + G` or `F9` registered system-wide to instantly flip modes from anywhere.
  - `Esc` key or header button to flip back to Border Glow from Cover Art Focus.

### Phase 8: Multi-Animation Creative Glow Engine & Preset Selectors (Completed)
- [x] Designed and implemented 6 distinct, highly creative visual animation styles in `GlowRenderer` ([`glow.ts`](file:///c:/Users/user/Documents/contraband/spotglow/src/glow.ts)):
  1. **Aurora Flow (Default)**: Conic spectrum gradient rotating seamlessly with ambient breathing aura and multi-layer chromatic halo.
  2. **Cyber Pulse**: High-energy cardiac heartbeat rhythm with expanding dynamic neon shockwave and corner accent pings.
  3. **Comet Orbit**: Dual celestial particle comets with sparkling stardust tails orbiting in opposite directions and exploding in radiant flares when crossing.
  4. **Audio EQ**: Segmented 56-bar audio spectrum visualizer wrapping Spotify's perimeter with frequency dynamics (bass, mids, treble).
  5. **Plasma Storm**: Molten liquid plasma wave interference with shifting color hotspots and corner plasma flares.
  6. **Zen Progress**: Minimalist luxury hairline border with real-time song progress arc and an illuminated breathing playhead beacon.
- [x] Built multi-channel selection & switching mechanism:
  - **System Tray Submenu**: Right-click tray icon $\to$ `Glow Animation Style` submenu with instant checkable options.
  - **Global Hotkey Thread**: Pressing <kbd>F8</kbd> or <kbd>Ctrl+Shift+A</kbd> cycles through all presets from anywhere in Windows.
  - **HUD Notification Toast**: Elegant frosted-glass notification pill slides in at the top of the screen to give instant visual feedback when switching styles.
  - **Cover Art Screen (Mode 2) Pill Bar**: Interactive style buttons to select or preview animation presets directly.
  - **Number Keys Shortcut**: Pressing keys <kbd>1</kbd> through <kbd>6</kbd> when focused instantly switches to the corresponding style preset.

### Phase 9: Polish, Packaging & Performance
- [ ] DPI scaling verification across multi-monitor setups.
- [ ] Smooth spring/fade animations when flipping between modes.
- [ ] CPU and memory verification (< 3% CPU during playback).
- [x] NSIS installer packaging (standalone ~1.88 MiB setup bundle).

---

## 6. Verification & Quality Checklist

- [x] **Mode 1 (Border Glow)**: Spotify is fully visible, thin glow hugs edges, transparent center, 100% click-through.
- [x] **Mode 1 Creative Styles**: 6 distinct animation styles (Aurora Flow, Cyber Pulse, Comet Orbit, Audio EQ, Plasma Storm, Zen Progress).
- [x] **Mode 1 & Mode 2 Style Switching**: System tray submenu, global hotkey (<kbd>F8</kbd> / <kbd>Ctrl+Shift+A</kbd>), HUD toast pill, and Mode 2 style picker pills.
- [x] **Mode 2 (Cover Art Focus)**: Spotify UI is concealed behind frosted backdrop, album art & track info display crisply, media buttons control Spotify playback.
- [x] **Mode 2 Border Glow Animations**: Active animation preset (Aurora, Cyber, Comet, EQ, Plasma, Zen) renders along the Cover Art card perimeter in floating mode and along screen boundaries in maximized mode.
- [x] **Glow Luminance & Bloom Enhancement**: High dynamic range luminance scaling (target peak 235+), boosted layer opacities, radiant ambient drop-shadow bloom, and laser hot-core highlights across all 6 presets.
- [x] **Maximized Behavior**: Both modes cleanly hug full-screen display boundaries.
- [x] **Transition Smoothness**: Switching between Mode 1 and Mode 2 has zero flicker or window jumping.
- [x] **Zero API Dependency**: 100% local Windows SMTC data; no internet requests or Spotify account tokens required.
