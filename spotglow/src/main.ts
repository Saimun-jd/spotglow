import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { GlowRenderer } from "./glow";

export interface ColorRgb {
  r: number;
  g: number;
  b: number;
}

export type Mood = "vivid" | "mellow" | "monochrome" | "dual_tone" | "balanced";

export interface PaletteInfo {
  primary: ColorRgb;
  secondary: ColorRgb;
  accent: ColorRgb;
  avg_brightness: number;
  avg_saturation: number;
  mood: Mood;
}

export interface TrackTimeline {
  position_ms: number;
  start_time_ms: number;
  end_time_ms: number;
}

export type PlaybackStatus =
  | "playing"
  | "paused"
  | "stopped"
  | "changing"
  | "closed"
  | "opened"
  | "unknown";

export interface TrackUpdatePayload {
  title: string;
  artist: string;
  album_title: string;
  status: PlaybackStatus;
  timeline: TrackTimeline;
  palette: PaletteInfo;
  mood: Mood;
  thumbnail_path?: string;
}

export interface WindowRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
  width: number;
  height: number;
}

export interface SpotifyWindowState {
  hwnd: number;
  rect: WindowRect;
  visible: boolean;
  minimized: boolean;
  maximized: boolean;
  dpi: number;
}

window.addEventListener("DOMContentLoaded", async () => {
  const canvas = document.getElementById("glow-canvas") as HTMLCanvasElement;
  let renderer: GlowRenderer | null = null;
  if (canvas) {
    renderer = new GlowRenderer(canvas);
    // renderer.setThickness(0.2);
    console.log("[SpotGlow] Overlay initialized, transparent canvas active.");
  }

  // 1. Fetch initial states from Rust immediately on mount
  try {
    const initialTrack = await invoke<TrackUpdatePayload | null>("get_current_track");
    if (initialTrack && renderer) {
      console.log("[SpotGlow] Initial track loaded via invoke:", initialTrack.title, initialTrack.artist);
      renderer.updateTrack(initialTrack);
    }
  } catch (err) {
    console.warn("[SpotGlow] Could not fetch initial track via invoke:", err);
  }

  try {
    const initialWin = await invoke<SpotifyWindowState>("get_spotify_window");
    if (initialWin && initialWin.hwnd !== 0 && renderer) {
      console.log("[SpotGlow] Initial window state loaded via invoke:", initialWin);
      renderer.updateWindow(initialWin.rect, initialWin.visible, initialWin.minimized, initialWin.maximized);
    }
  } catch (err) {
    console.warn("[SpotGlow] Could not fetch initial window via invoke:", err);
  }

  // 2. Listen for continuous real-time updates from Rust
  try {
    await listen<TrackUpdatePayload>("track_update", (event) => {
      const payload = event.payload;
      console.log("[SpotGlow] track_update received:", {
        title: payload.title,
        artist: payload.artist,
        status: payload.status,
        mood: payload.mood,
        palette: {
          primary: `rgb(${payload.palette.primary.r}, ${payload.palette.primary.g}, ${payload.palette.primary.b})`,
          secondary: `rgb(${payload.palette.secondary.r}, ${payload.palette.secondary.g}, ${payload.palette.secondary.b})`,
          accent: `rgb(${payload.palette.accent.r}, ${payload.palette.accent.g}, ${payload.palette.accent.b})`,
        },
      });

      if (renderer) {
        renderer.updateTrack(payload);
      }
    });
    console.log("[SpotGlow] Subscribed to track_update event.");
  } catch (err) {
    console.error("[SpotGlow] Failed to listen for track_update event:", err);
  }

  try {
    await listen<SpotifyWindowState>("spotify_window", (event) => {
      const win = event.payload;
      console.log("[SpotGlow] spotify_window received:", {
        hwnd: `0x${win.hwnd.toString(16)}`,
        bounds: `${win.rect.width}x${win.rect.height} at (${win.rect.left}, ${win.rect.top})`,
        minimized: win.minimized,
        maximized: win.maximized,
        dpi: win.dpi,
      });

      console.log("DPI check", {
      stateDpi: win.dpi,
      dprFromState: win.dpi / 96,
      devicePixelRatio: window.devicePixelRatio,
      innerW: window.innerWidth,
      rectW: win.rect.width,
      physicalMarginApplied: (window.innerWidth * window.devicePixelRatio - win.rect.width) / 2,
      physicalMarginJsDraws: 18 * window.devicePixelRatio,
    });

      if (renderer) {
        renderer.updateWindow(win.rect, win.visible, win.minimized, win.maximized);
      }
    });
    console.log("[SpotGlow] Subscribed to spotify_window event.");
  } catch (err) {
    console.error("[SpotGlow] Failed to listen for spotify_window event:", err);
  }
});
