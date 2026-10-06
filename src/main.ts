import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { GlowRenderer, GlowStyle } from "./glow";

export type DisplayMode = "border_glow" | "cover_art";

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
  thumbnail_data_url?: string;
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

function formatTime(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  return `${minutes}:${seconds.toString().padStart(2, "0")}`;
}

window.addEventListener("DOMContentLoaded", async () => {
  const canvas = document.getElementById("glow-canvas") as HTMLCanvasElement;
  let renderer: GlowRenderer | null = null;
  if (canvas) {
    renderer = new GlowRenderer(canvas);
    const recMargin = renderer.getRecommendedMargin();
    document.documentElement.style.setProperty("--overlay-margin", `${recMargin}px`);
    console.log("[SpotGlow] Overlay initialized, transparent canvas active.");
  }

  // Cover Art Focus UI Elements
  const shroud = document.getElementById("cover-shroud") as HTMLElement | null;
  const coverCardContainer = document.getElementById("cover-card-container") as HTMLElement | null;
  const coverImg = document.getElementById("cover-img") as HTMLImageElement | null;
  const coverPlaceholder = document.getElementById("cover-placeholder") as HTMLElement | null;
  const trackTitle = document.getElementById("track-title") as HTMLElement | null;
  const trackArtist = document.getElementById("track-artist") as HTMLElement | null;
  const trackAlbum = document.getElementById("track-album") as HTMLElement | null;
  const timeCurrent = document.getElementById("time-current") as HTMLElement | null;
  const timeTotal = document.getElementById("time-total") as HTMLElement | null;
  const timelineFill = document.getElementById("timeline-fill") as HTMLElement | null;
  const badgeText = document.getElementById("badge-text") as HTMLElement | null;
  const badgeEq = document.querySelector(".badge-eq") as HTMLElement | null;
  const btnFlipGlow = document.getElementById("btn-flip-glow") as HTMLButtonElement | null;
  const btnPlayPause = document.getElementById("btn-play-pause") as HTMLButtonElement | null;
  const iconPlay = document.getElementById("icon-play") as SVGElement | null;
  const iconPause = document.getElementById("icon-pause") as SVGElement | null;
  const btnPrev = document.getElementById("btn-prev") as HTMLButtonElement | null;
  const btnNext = document.getElementById("btn-next") as HTMLButtonElement | null;

  // Global HUD Toast Notification Elements
  const hudToast = document.getElementById("hud-toast") as HTMLElement | null;
  const hudText = document.getElementById("hud-text") as HTMLElement | null;
  let hudTimeout: number | null = null;

  const styleLabels: Record<GlowStyle, string> = {
    aurora_flow: "Aurora Flow (Fluid Wave)",
    cyber_pulse: "Cyber Pulse (Neon Beat)",
    comet_orbit: "Comet Orbit (Dual Celestial)",
    audio_eq: "Audio EQ (Soundwave Bars)",
    plasma_storm: "Plasma Storm (Liquid Collision)",
    zen_progress: "Zen Progress (Minimalist Tracer)",
  };

  const showHudToast = (style: GlowStyle) => {
    if (!hudToast || !hudText) return;
    const label = styleLabels[style] || style;
    hudText.textContent = label;
    hudToast.classList.remove("hidden");
    if (hudTimeout) {
      clearTimeout(hudTimeout);
    }
    hudTimeout = window.setTimeout(() => {
      hudToast.classList.add("hidden");
      hudTimeout = null;
    }, 1800);
  };

  let currentDisplayMode: DisplayMode = "border_glow";
  let currentGlowStyle: GlowStyle = "aurora_flow";

  const applyGlowStyle = (style: GlowStyle, showToast: boolean = true) => {
    currentGlowStyle = style;
    console.log("[SpotGlow] Active glow style:", currentGlowStyle);
    if (renderer) {
      renderer.setStyle(style);
    }
    const pills = document.querySelectorAll<HTMLButtonElement>(".style-pill");
    pills.forEach((pill) => {
      if (pill.dataset.style === style) {
        pill.classList.add("active");
      } else {
        pill.classList.remove("active");
      }
    });
    if (showToast) {
      showHudToast(style);
    }
  };

  const setGlowStyle = async (style: GlowStyle) => {
    applyGlowStyle(style, true);
    try {
      await invoke("set_glow_style", { style });
    } catch (err) {
      console.error("[SpotGlow] Failed to invoke set_glow_style:", err);
    }
  };

  let timelineState = {
    position_ms: 0,
    end_time_ms: 1,
    last_ts: performance.now(),
    is_playing: false,
  };

  // Live timeline playhead tick (200ms)
  setInterval(() => {
    if (timelineState.is_playing && timelineState.end_time_ms > 0) {
      const elapsed = performance.now() - timelineState.last_ts;
      const estPos = Math.min(timelineState.end_time_ms, timelineState.position_ms + elapsed);
      if (timeCurrent) {
        timeCurrent.textContent = formatTime(estPos);
      }
      if (timelineFill) {
        const pct = (estPos / timelineState.end_time_ms) * 100;
        timelineFill.style.width = `${Math.min(100, Math.max(0, pct))}%`;
      }
    }
  }, 200);

  const applyDisplayMode = (mode: DisplayMode) => {
    currentDisplayMode = mode;
    if (renderer) {
      renderer.setDisplayMode(mode);
    }
    if (shroud) {
      if (mode === "cover_art") {
        shroud.classList.remove("hidden");
      } else {
        shroud.classList.add("hidden");
      }
    }
  };

  const setDisplayMode = async (mode: DisplayMode) => {
    applyDisplayMode(mode);
    try {
      await invoke("set_display_mode", { mode });
    } catch (err) {
      console.error("[SpotGlow] Failed to invoke set_display_mode:", err);
    }
  };

  const updateTrackDisplay = (track: TrackUpdatePayload) => {
    // Dynamic palette color injection for CSS ambient variables
    if (track.palette) {
      const p = track.palette.primary;
      const s = track.palette.secondary;
      const a = track.palette.accent;
      // Creative tertiary color derived by blending and hue shift
      const tR = Math.min(255, Math.max(0, Math.round((s.g * 1.2 + a.r) / 2)));
      const tG = Math.min(255, Math.max(0, Math.round((s.b * 1.2 + a.g) / 2)));
      const tB = Math.min(255, Math.max(0, Math.round((p.r * 1.2 + a.b) / 2)));

      document.documentElement.style.setProperty("--primary-rgb", `${p.r}, ${p.g}, ${p.b}`);
      document.documentElement.style.setProperty("--secondary-rgb", `${s.r}, ${s.g}, ${s.b}`);
      document.documentElement.style.setProperty("--accent-rgb", `${a.r}, ${a.g}, ${a.b}`);
      document.documentElement.style.setProperty("--tertiary-rgb", `${tR}, ${tG}, ${tB}`);
    }

    // Text metadata
    if (trackTitle) trackTitle.textContent = track.title || "Waiting for Spotify...";
    if (trackArtist) trackArtist.textContent = track.artist || "—";
    if (trackAlbum) trackAlbum.textContent = track.album_title || "—";

    // Album Artwork Image
    if (coverImg) {
      if (track.thumbnail_data_url) {
        coverImg.src = track.thumbnail_data_url;
        coverImg.classList.add("loaded");
        if (coverPlaceholder) coverPlaceholder.classList.add("hidden");
      } else {
        coverImg.src = "";
        coverImg.classList.remove("loaded");
        if (coverPlaceholder) coverPlaceholder.classList.remove("hidden");
      }
    }

    // Playback state & icons
    const isPlaying = track.status === "playing";
    timelineState = {
      position_ms: track.timeline.position_ms,
      end_time_ms: Math.max(1, track.timeline.end_time_ms),
      last_ts: performance.now(),
      is_playing: isPlaying,
    };

    if (timeCurrent) timeCurrent.textContent = formatTime(track.timeline.position_ms);
    if (timeTotal) timeTotal.textContent = formatTime(track.timeline.end_time_ms);
    if (timelineFill) {
      const pct = (track.timeline.position_ms / Math.max(1, track.timeline.end_time_ms)) * 100;
      timelineFill.style.width = `${Math.min(100, Math.max(0, pct))}%`;
    }

    if (badgeText) badgeText.textContent = isPlaying ? "PLAYING" : "PAUSED";
    if (badgeEq) {
      if (isPlaying) {
        badgeEq.classList.add("playing");
      } else {
        badgeEq.classList.remove("playing");
      }
    }

    if (coverCardContainer) {
      if (isPlaying) {
        coverCardContainer.classList.add("is-playing");
        coverCardContainer.classList.remove("is-paused");
      } else {
        coverCardContainer.classList.remove("is-playing");
        coverCardContainer.classList.add("is-paused");
      }
    }

    if (iconPlay && iconPause) {
      if (isPlaying) {
        iconPlay.classList.add("hidden");
        iconPause.classList.remove("hidden");
      } else {
        iconPlay.classList.remove("hidden");
        iconPause.classList.add("hidden");
      }
    }
  };

  // Wire User Interactions
  btnFlipGlow?.addEventListener("click", () => {
    setDisplayMode("border_glow");
  });

  // Wire Style Preset Pills
  const pillButtons = document.querySelectorAll<HTMLButtonElement>(".style-pill");
  pillButtons.forEach((btn) => {
    btn.addEventListener("click", () => {
      const targetStyle = btn.dataset.style as GlowStyle;
      if (targetStyle) {
        setGlowStyle(targetStyle);
      }
    });
  });

  btnPlayPause?.addEventListener("click", async () => {
    try {
      await invoke("media_play_pause");
    } catch (err) {
      console.error("[SpotGlow] Failed to trigger media_play_pause:", err);
    }
  });

  btnPrev?.addEventListener("click", async () => {
    try {
      await invoke("media_previous");
    } catch (err) {
      console.error("[SpotGlow] Failed to trigger media_previous:", err);
    }
  });

  btnNext?.addEventListener("click", async () => {
    try {
      await invoke("media_next");
    } catch (err) {
      console.error("[SpotGlow] Failed to trigger media_next:", err);
    }
  });

  // Global Keyboard Shortcuts (when webview has focus / shroud is open)
  window.addEventListener("keydown", async (e) => {
    if (e.key === "Escape" || (e.ctrlKey && e.key.toLowerCase() === "g")) {
      e.preventDefault();
      const targetMode: DisplayMode = currentDisplayMode === "cover_art" ? "border_glow" : "cover_art";
      await setDisplayMode(targetMode);
    } else if (e.code === "Space" && currentDisplayMode === "cover_art") {
      e.preventDefault();
      try {
        await invoke("media_play_pause");
      } catch (err) {
        console.error("[SpotGlow] Media command error:", err);
      }
    } else if (e.key === "F8" || (e.ctrlKey && e.shiftKey && e.key.toLowerCase() === "a")) {
      e.preventDefault();
      try {
        const nextStyle = await invoke<GlowStyle>("cycle_glow_style");
        if (nextStyle) {
          applyGlowStyle(nextStyle, true);
        }
      } catch (err) {
        console.error("[SpotGlow] Failed to cycle glow style:", err);
      }
    } else if (currentDisplayMode === "cover_art" && ["1", "2", "3", "4", "5", "6"].includes(e.key)) {
      e.preventDefault();
      const styles: GlowStyle[] = [
        "aurora_flow",
        "cyber_pulse",
        "comet_orbit",
        "audio_eq",
        "plasma_storm",
        "zen_progress",
      ];
      const idx = parseInt(e.key, 10) - 1;
      if (styles[idx]) {
        await setGlowStyle(styles[idx]);
      }
    }
  });

  // 1. Fetch initial states from Rust immediately on mount
  try {
    const initialMode = await invoke<DisplayMode>("get_display_mode");
    if (initialMode) {
      applyDisplayMode(initialMode);
    }
  } catch (err) {
    console.warn("[SpotGlow] Could not fetch initial display mode:", err);
  }

  try {
    const initialStyle = await invoke<GlowStyle>("get_glow_style");
    if (initialStyle) {
      applyGlowStyle(initialStyle, false);
    }
  } catch (err) {
    console.warn("[SpotGlow] Could not fetch initial glow style:", err);
  }

  try {
    const initialTrack = await invoke<TrackUpdatePayload | null>("get_current_track");
    if (initialTrack) {
      console.log("[SpotGlow] Initial track loaded via invoke:", initialTrack.title, initialTrack.artist);
      updateTrackDisplay(initialTrack);
      if (renderer) {
        renderer.updateTrack(initialTrack);
      }
    }
  } catch (err) {
    console.warn("[SpotGlow] Could not fetch initial track via invoke:", err);
  }

  try {
    const initialWin = await invoke<SpotifyWindowState>("get_spotify_window");
    if (initialWin && initialWin.hwnd !== 0) {
      console.log("[SpotGlow] Initial window state loaded via invoke:", initialWin);
      if (shroud) {
        if (initialWin.maximized) {
          shroud.classList.add("maximized");
        } else {
          shroud.classList.remove("maximized");
        }
      }
      if (renderer) {
        renderer.updateWindow(initialWin.rect, initialWin.visible, initialWin.minimized, initialWin.maximized);
      }
    }
  } catch (err) {
    console.warn("[SpotGlow] Could not fetch initial window via invoke:", err);
  }

  // 2. Listen for continuous real-time updates from Rust
  try {
    await listen<DisplayMode>("display_mode_changed", (event) => {
      console.log("[SpotGlow] display_mode_changed received:", event.payload);
      applyDisplayMode(event.payload);
    });
  } catch (err) {
    console.error("[SpotGlow] Failed to listen for display_mode_changed:", err);
  }

  try {
    await listen<GlowStyle>("glow_style_changed", (event) => {
      console.log("[SpotGlow] glow_style_changed received:", event.payload);
      applyGlowStyle(event.payload, true);
    });
  } catch (err) {
    console.error("[SpotGlow] Failed to listen for glow_style_changed:", err);
  }

  try {
    await listen<TrackUpdatePayload>("track_update", (event) => {
      const payload = event.payload;
      updateTrackDisplay(payload);
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
      if (shroud) {
        if (win.maximized) {
          shroud.classList.add("maximized");
        } else {
          shroud.classList.remove("maximized");
        }
      }
      if (renderer) {
        renderer.updateWindow(win.rect, win.visible, win.minimized, win.maximized);
      }
    });
    console.log("[SpotGlow] Subscribed to spotify_window event.");
  } catch (err) {
    console.error("[SpotGlow] Failed to listen for spotify_window event:", err);
  }
});
