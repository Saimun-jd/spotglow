import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { GlowRenderer, GlowStyle, AudioBeatPayload } from "./glow";
import { fetchLyrics, findActiveLyricIndex, LrcLyrics } from "./lyrics";

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

export function ensureLuminousPalette(palette?: PaletteInfo): {
  primary: ColorRgb;
  secondary: ColorRgb;
  accent: ColorRgb;
  tertiary: ColorRgb;
} {
  const fallback = {
    primary: { r: 30, g: 215, b: 96 },
    secondary: { r: 160, g: 32, b: 240 },
    accent: { r: 255, g: 255, b: 255 },
    tertiary: { r: 0, g: 210, b: 255 },
  };

  if (!palette) return fallback;

  let p = { ...palette.primary };
  let s = { ...palette.secondary };
  let a = { ...palette.accent };

  const getLuma = (c: ColorRgb) => 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
  const getChroma = (c: ColorRgb) => Math.max(c.r, c.g, c.b) - Math.min(c.r, c.g, c.b);

  const pLuma = getLuma(p);
  const pChroma = getChroma(p);
  const sChroma = getChroma(s);

  // 1. Detect Monochrome / Grayscale / Low-luminance dark art (e.g. night photography like Bujhe Na Bujhe)
  if ((pChroma < 28 && sChroma < 28) || pLuma < 58) {
    // Check if accent has a warm fairy-light / amber hue (e.g. string lights in night scene)
    const aChroma = getChroma(a);
    if (aChroma > 25 && a.r > 130 && a.r > a.b) {
      // Warm golden fairy-light incandescent radiance
      return {
        primary: { r: 255, g: 210, b: 145 },
        secondary: { r: 255, g: 150, b: 80 },
        accent: { r: 255, g: 248, b: 225 },
        tertiary: { r: 245, g: 125, b: 65 },
      };
    }

    // Celestial moonlight platinum & icy electric cyan aura
    return {
      primary: { r: 220, g: 236, b: 255 },
      secondary: { r: 145, g: 200, b: 255 },
      accent: { r: 255, g: 255, b: 255 },
      tertiary: { r: 195, g: 185, b: 255 },
    };
  }

  // 2. For all colored albums, guarantee peak radiance (min channel peak 225) so glow shines brightly
  const boost = (c: ColorRgb): ColorRgb => {
    const maxVal = Math.max(c.r, c.g, c.b);
    let r = c.r;
    let g = c.g;
    let b = c.b;

    if (maxVal < 225) {
      const factor = 225 / Math.max(1, maxVal);
      r = Math.min(255, Math.round(r * factor));
      g = Math.min(255, Math.round(g * factor));
      b = Math.min(255, Math.round(b * factor));
    }

    // Boost saturation if muted
    const chroma = Math.max(r, g, b) - Math.min(r, g, b);
    if (chroma < 65) {
      const avg = (r + g + b) / 3;
      r = Math.min(255, Math.max(0, Math.round(avg + (r - avg) * 1.45)));
      g = Math.min(255, Math.max(0, Math.round(avg + (g - avg) * 1.45)));
      b = Math.min(255, Math.max(0, Math.round(avg + (b - avg) * 1.45)));
    }
    return { r, g, b };
  };

  const boostedP = boost(p);
  const boostedS = boost(s);
  const boostedA = boost(a);

  const tR = Math.min(255, Math.max(0, Math.round((boostedS.g * 1.2 + boostedA.r) / 2)));
  const tG = Math.min(255, Math.max(0, Math.round((boostedS.b * 1.2 + boostedA.g) / 2)));
  const tB = Math.min(255, Math.max(0, Math.round((boostedP.r * 1.2 + boostedA.b) / 2)));

  return {
    primary: boostedP,
    secondary: boostedS,
    accent: boostedA,
    tertiary: { r: tR, g: tG, b: tB },
  };
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
  const coverCard = document.getElementById("cover-card") as HTMLElement | null;
  const coverPulseHalo = document.querySelector(".cover-pulse-halo") as HTMLElement | null;
  const coverPulseBloom = document.querySelector(".cover-pulse-bloom") as HTMLElement | null;
  const coverPulseRim = document.querySelector(".cover-pulse-rim") as HTMLElement | null;
  const coverPulseWaves = Array.from(document.querySelectorAll<HTMLElement>(".cover-pulse-wave"));
  coverPulseWaves.forEach((wave) => {
    wave.addEventListener("animationend", () => {
      wave.classList.remove("trigger-shockwave");
    });
  });
  const albumAuraGlow = document.querySelector(".album-aura-glow") as HTMLElement | null;
  const ambientStars = Array.from(document.querySelectorAll<HTMLElement>(".ambient-star"));
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
  const btnToggleLyrics = document.getElementById("btn-toggle-lyrics") as HTMLButtonElement | null;
  const btnLyricsText = document.getElementById("btn-lyrics-text") as HTMLElement | null;
  const brandModeSub = document.getElementById("brand-mode-sub") as HTMLElement | null;
  const lyricsStage = document.getElementById("lyrics-stage") as HTMLElement | null;
  const lyricsScrollContainer = document.getElementById("lyrics-scroll-container") as HTMLElement | null;
  const lyricsLinesContainer = document.getElementById("lyrics-lines") as HTMLElement | null;
  const lyricsStatus = document.getElementById("lyrics-status") as HTMLElement | null;
  const lyricsStatusText = document.getElementById("lyrics-status-text") as HTMLElement | null;

  // Real-Time Synchronized Lyrics State
  let isLyricsActive = false;
  let currentLyrics: LrcLyrics | null = null;
  let currentActiveLineIndex = -1;
  let userScrolledUntil = 0;
  let lastLoadedTrackKey = "";
  let currentTrackData: TrackUpdatePayload | null = null;
  let activeFetchId = 0;

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

  // Fluid Spotify-style lyrics scrolling and highlight engine
  const centerLyricLine = (idx: number, immediate: boolean = false) => {
    if (!lyricsLinesContainer || !lyricsScrollContainer) return;
    if (idx < 0 || idx >= lyricsLinesContainer.children.length) return;

    const activeEl = lyricsLinesContainer.children[idx] as HTMLElement;
    if (!activeEl) return;

    const lineRect = activeEl.getBoundingClientRect();
    const containerRect = lyricsScrollContainer.getBoundingClientRect();
    if (containerRect.height === 0) return;

    // Relative offset from scroll container top to lyric line top
    const currentOffset = lineRect.top - containerRect.top;
    // Desired vertical center position
    const desiredOffset = (containerRect.height / 2) - (lineRect.height / 2);
    const delta = currentOffset - desiredOffset;

    if (Math.abs(delta) < 1.5) return;

    const targetScrollTop = Math.max(0, lyricsScrollContainer.scrollTop + delta);

    lyricsScrollContainer.scrollTo({
      top: targetScrollTop,
      behavior: immediate ? "auto" : "smooth",
    });
  };

  const updateLyricHighlight = (newIdx: number, smooth: boolean = true) => {
    if (!lyricsLinesContainer || !currentLyrics?.synced) return;
    const children = lyricsLinesContainer.children;
    const total = children.length;
    if (total === 0) return;

    if (newIdx !== currentActiveLineIndex) {
      if (currentActiveLineIndex < 0 || Math.abs(newIdx - currentActiveLineIndex) > 1) {
        for (let i = 0; i < total; i++) {
          const el = children[i] as HTMLElement;
          if (i === newIdx) {
            el.className = "lyric-line active";
          } else if (i < newIdx) {
            el.className = "lyric-line past";
          } else {
            el.className = "lyric-line upcoming";
          }
        }
      } else {
        if (currentActiveLineIndex >= 0 && currentActiveLineIndex < total) {
          const prevEl = children[currentActiveLineIndex] as HTMLElement;
          prevEl.className = currentActiveLineIndex < newIdx ? "lyric-line past" : "lyric-line upcoming";
        }
        if (newIdx >= 0 && newIdx < total) {
          const nextEl = children[newIdx] as HTMLElement;
          nextEl.className = "lyric-line active";
        }
      }

      currentActiveLineIndex = newIdx;

      if (newIdx >= 0 && newIdx < total && performance.now() > userScrolledUntil) {
        centerLyricLine(newIdx, !smooth);
      }
    }
  };

  const renderLyrics = (lyrics: LrcLyrics | null, loading: boolean = false, statusMsg?: string) => {
    if (!lyricsLinesContainer || !lyricsStatus || !lyricsStatusText) return;

    if (loading) {
      lyricsLinesContainer.innerHTML = "";
      lyricsStatus.classList.remove("hidden");
      lyricsStatusText.textContent = statusMsg || "Loading lyrics from LRCLIB...";
      return;
    }

    if (!lyrics || lyrics.lines.length === 0) {
      lyricsLinesContainer.innerHTML = "";
      lyricsStatus.classList.remove("hidden");
      lyricsStatusText.textContent = lyrics?.instrumental
        ? "♪ Instrumental Track ♪"
        : "No synced lyrics available";
      return;
    }

    // Populate lyrics lines
    lyricsStatus.classList.add("hidden");
    lyricsLinesContainer.innerHTML = "";

    const fragment = document.createDocumentFragment();
    lyrics.lines.forEach((line, idx) => {
      const div = document.createElement("div");
      div.className = `lyric-line ${lyrics.synced ? "upcoming" : "unsynced"}`;
      div.dataset.index = idx.toString();
      div.dataset.timeMs = line.timeMs.toString();
      div.textContent = line.text;

      div.addEventListener("click", () => {
        userScrolledUntil = performance.now() + 3500;
        updateLyricHighlight(idx, true);
      });

      fragment.appendChild(div);
    });

    lyricsLinesContainer.appendChild(fragment);
    currentActiveLineIndex = -1;
  };

  const loadLyricsForTrack = async (track: TrackUpdatePayload, forceFetch: boolean = false) => {
    currentTrackData = track;
    if (!track.title || track.title === "Waiting for Spotify...") return;

    const trackKey = `${track.title.toLowerCase().trim()}::${(track.artist || "").toLowerCase().trim()}`;
    if (!forceFetch && lastLoadedTrackKey === trackKey && currentLyrics !== null) {
      if (isLyricsActive) {
        renderLyrics(currentLyrics, false);
      }
      return;
    }

    lastLoadedTrackKey = trackKey;
    currentLyrics = null;
    currentActiveLineIndex = -1;

    if (isLyricsActive) {
      renderLyrics(null, true, "Finding lyrics on LRCLIB...");
    }

    const fetchId = ++activeFetchId;
    const durationSec =
      track.timeline.end_time_ms > 0 ? Math.round(track.timeline.end_time_ms / 1000) : undefined;
    const lyrics = await fetchLyrics(track.title, track.artist, track.album_title, durationSec);

    // Guard against race conditions if another track changed while fetching
    if (fetchId !== activeFetchId) return;

    currentLyrics = lyrics;
    if (isLyricsActive) {
      renderLyrics(lyrics, false);
      if (lyrics?.synced && lyrics.lines.length > 0) {
        const elapsed = performance.now() - timelineState.last_ts;
        const estPos = Math.min(timelineState.end_time_ms, timelineState.position_ms + elapsed);
        const idx = findActiveLyricIndex(lyrics.lines, estPos);
        if (idx >= 0) {
          requestAnimationFrame(() => {
            updateLyricHighlight(idx, false);
          });
        }
      }
    }
  };

  const toggleLyrics = (force?: boolean) => {
    isLyricsActive = typeof force === "boolean" ? force : !isLyricsActive;

    if (shroud) {
      if (isLyricsActive) {
        shroud.classList.add("lyrics-active");
      } else {
        shroud.classList.remove("lyrics-active");
      }
    }

    if (btnToggleLyrics) {
      if (isLyricsActive) {
        btnToggleLyrics.classList.add("active");
      } else {
        btnToggleLyrics.classList.remove("active");
      }
    }

    if (btnLyricsText) {
      btnLyricsText.textContent = isLyricsActive ? "Cover" : "Lyrics";
    }

    if (brandModeSub) {
      brandModeSub.textContent = isLyricsActive ? "Live Lyrics" : "Cover Focus";
    }

    if (lyricsStage) {
      if (isLyricsActive) {
        lyricsStage.classList.remove("hidden");
      } else {
        lyricsStage.classList.add("hidden");
      }
    }

    if (isLyricsActive) {
      if (currentLyrics) {
        renderLyrics(currentLyrics, false);
        if (currentLyrics.synced && currentLyrics.lines.length > 0) {
          const elapsed = performance.now() - timelineState.last_ts;
          const estPos = Math.min(timelineState.end_time_ms, timelineState.position_ms + elapsed);
          const idx = findActiveLyricIndex(currentLyrics.lines, estPos);
          if (idx >= 0) {
            requestAnimationFrame(() => {
              updateLyricHighlight(idx, false);
            });
          }
        }
      } else if (currentTrackData) {
        loadLyricsForTrack(currentTrackData, true);
      }
    }
  };

  // Pause auto-scroll when user manually wheels or drags inside lyrics container
  lyricsScrollContainer?.addEventListener(
    "wheel",
    () => {
      userScrolledUntil = performance.now() + 4000;
    },
    { passive: true }
  );

  lyricsScrollContainer?.addEventListener(
    "touchmove",
    () => {
      userScrolledUntil = performance.now() + 4000;
    },
    { passive: true }
  );

  // Live timeline playhead & lyrics synchronizer tick (100ms)
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

      // Real-time lyrics line synchronization with fluid scroll
      if (
        isLyricsActive &&
        currentLyrics?.synced &&
        currentLyrics.lines.length > 0 &&
        lyricsLinesContainer
      ) {
        const newIdx = findActiveLyricIndex(currentLyrics.lines, estPos);
        if (newIdx !== currentActiveLineIndex) {
          updateLyricHighlight(newIdx, true);
        }
      }
    }
  }, 25);

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
    // Dynamic luminous palette injection ensuring brilliant glows even for monochrome & dark art
    if (track.palette) {
      const lum = ensureLuminousPalette(track.palette);
      document.documentElement.style.setProperty("--primary-rgb", `${lum.primary.r}, ${lum.primary.g}, ${lum.primary.b}`);
      document.documentElement.style.setProperty("--secondary-rgb", `${lum.secondary.r}, ${lum.secondary.g}, ${lum.secondary.b}`);
      document.documentElement.style.setProperty("--accent-rgb", `${lum.accent.r}, ${lum.accent.g}, ${lum.accent.b}`);
      document.documentElement.style.setProperty("--tertiary-rgb", `${lum.tertiary.r}, ${lum.tertiary.g}, ${lum.tertiary.b}`);
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

    // Prefetch / update synchronized lyrics
    loadLyricsForTrack(track);
  };

  // Wire User Interactions
  btnFlipGlow?.addEventListener("click", () => {
    setDisplayMode("border_glow");
  });

  btnToggleLyrics?.addEventListener("click", () => {
    toggleLyrics();
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
      if (currentDisplayMode === "cover_art" && isLyricsActive) {
        toggleLyrics(false);
      } else {
        const targetMode: DisplayMode = currentDisplayMode === "cover_art" ? "border_glow" : "cover_art";
        await setDisplayMode(targetMode);
      }
    } else if (
      currentDisplayMode === "cover_art" &&
      (e.key.toLowerCase() === "l" || (e.ctrlKey && e.key.toLowerCase() === "l"))
    ) {
      e.preventDefault();
      toggleLyrics();
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

  // Re-center active lyric line when window resizes
  window.addEventListener("resize", () => {
    if (isLyricsActive && currentActiveLineIndex >= 0) {
      centerLyricLine(currentActiveLineIndex, true);
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

  // 3. Listen for real-time STFT beat and spectral energy events from Rust WASAPI loopback
  try {
    const eqBar1 = document.querySelector(".badge-eq .bar-1") as HTMLElement | null;
    const eqBar2 = document.querySelector(".badge-eq .bar-2") as HTMLElement | null;
    const eqBar3 = document.querySelector(".badge-eq .bar-3") as HTMLElement | null;

    let waveIndex = 0;
    let lastBeatTriggerTime = 0;
    let smoothedBeatStrength = 0;

    await listen<AudioBeatPayload>("audio_beat", (event) => {
      const beat = event.payload;

      // Update CSS variables for hardware-accelerated style interpolations
      document.documentElement.style.setProperty("--audio-beat", beat.beat.toFixed(3));
      document.documentElement.style.setProperty("--audio-bass", beat.bass.toFixed(3));
      document.documentElement.style.setProperty("--audio-mid", beat.mid.toFixed(3));
      document.documentElement.style.setProperty("--audio-treble", beat.treble.toFixed(3));
      document.documentElement.style.setProperty("--audio-volume", beat.volume.toFixed(3));

      // Calculate organic target beat energy from real-time audio
      const isTrackActive = timelineState.is_playing || beat.volume > 0.002 || beat.bass > 0.005 || beat.beat > 0.005;
      let targetBeatStrength = 0;

      if (isTrackActive) {
        const beatOnset = beat.beat;
        const bassEnergy = beat.bass;
        const volWeight = Math.min(1.0, Math.max(0.35, beat.volume * 2.2));
        targetBeatStrength = Math.min(1.0, Math.max(0.0, (beatOnset * 0.60 + bassEnergy * 0.40) * volWeight));
      }

      // Asymmetric dual-rate easing: responsive organic attack, velvety smooth musical release
      if (targetBeatStrength > smoothedBeatStrength) {
        smoothedBeatStrength += (targetBeatStrength - smoothedBeatStrength) * 0.35;
      } else {
        smoothedBeatStrength += (targetBeatStrength - smoothedBeatStrength) * 0.12;
      }

      // Trigger outward beat ripple shockwave on musical downbeats (spaced out, never strobe)
      if (beat.is_beat && smoothedBeatStrength > 0.28 && beat.volume > 0.008) {
        const now = performance.now();
        if (now - lastBeatTriggerTime > 340) {
          lastBeatTriggerTime = now;
          if (coverPulseWaves.length > 0) {
            let targetWave = coverPulseWaves.find((w) => !w.classList.contains("trigger-shockwave"));
            if (!targetWave) {
              targetWave = coverPulseWaves[waveIndex % coverPulseWaves.length];
              waveIndex++;
            }
            if (targetWave) {
              const instantImpact = Math.min(1.0, Math.max(0.60, 0.45 + smoothedBeatStrength * 0.55));
              targetWave.style.setProperty("--wave-strength", instantImpact.toFixed(3));
              targetWave.classList.remove("trigger-shockwave");
              void targetWave.offsetWidth; // Force reflow to restart CSS animation
              targetWave.classList.add("trigger-shockwave");
            }
          }
        }
      }

      // Real-time album cover glow & pulse sync with gentle ease and radial distance falloff
      if (coverCard && currentDisplayMode === "cover_art") {
        if (isTrackActive) {
          // Beat strength decides total opacity / glow strength:
          // Stable resting floor at 0.46, tastefully modulating up to 0.68 on heavy kicks (restrained, "not too much")
          const totalGlowStrength = Math.min(0.70, Math.max(0.44, 0.46 + smoothedBeatStrength * 0.22));

          // Tasteful musical bounce on the album cover card (responsive ~2.8% scale bounce, punchy without violence)
          const cardScale = 1.0 + smoothedBeatStrength * 0.028;
          coverCard.style.transform = `scale(${cardScale.toFixed(4)})`;

          // Radial falloff on box-shadow (opacity strictly decreases with distance from perimeter):
          // Radius 1 (~18-24px): high density core (highest opacity)
          const s1 = Math.round(18 + smoothedBeatStrength * 6);
          const o1 = (totalGlowStrength * 0.82).toFixed(3);
          // Radius 2 (~36-48px): mid density near halo (~52% of core)
          const s2 = Math.round(36 + smoothedBeatStrength * 12);
          const o2 = (totalGlowStrength * 0.44).toFixed(3);
          // Radius 3 (~72-90px): soft diffuse radiance bloom (~25% of core)
          const s3 = Math.round(72 + smoothedBeatStrength * 18);
          const o3 = (totalGlowStrength * 0.20).toFixed(3);
          // Radius 4 (~122-146px): outer whisper corona (~8% of core)
          const s4 = Math.round(122 + smoothedBeatStrength * 24);
          const o4 = (totalGlowStrength * 0.07).toFixed(3);

          coverCard.style.boxShadow = `
            0 0 ${s1}px rgba(var(--primary-rgb), ${o1}),
            0 0 ${s2}px rgba(var(--primary-rgb), ${o2}),
            0 0 ${s3}px rgba(var(--secondary-rgb), ${o3}),
            0 0 ${s4}px rgba(var(--accent-rgb), ${o4}),
            0 20px 50px rgba(0, 0, 0, 0.95)
          `;

          // Layer 1: Immediate Contour Rim (Distance ~0-6px from card edge)
          // Opacity modulates gently: ~0.39 resting up to ~0.58 on peak beat
          if (coverPulseRim) {
            const rScale = 1.0 + smoothedBeatStrength * 0.022;
            const rOpacity = totalGlowStrength * 0.85;
            coverPulseRim.style.transform = `scale(${rScale.toFixed(4)})`;
            coverPulseRim.style.opacity = rOpacity.toFixed(3);
          }

          // Layer 2: Concentric Breathing Halo (Distance ~15-40px from card edge)
          // Opacity modulates gently: ~0.22 resting up to ~0.33 on peak beat
          if (coverPulseHalo) {
            const hScale = 1.0 + smoothedBeatStrength * 0.032;
            const hOpacity = totalGlowStrength * 0.48;
            const hBlur = 20 + smoothedBeatStrength * 8;
            coverPulseHalo.style.transform = `scale(${hScale.toFixed(4)})`;
            coverPulseHalo.style.opacity = hOpacity.toFixed(3);
            coverPulseHalo.style.filter = `blur(${hBlur.toFixed(1)}px)`;
          }

          // Layer 3: Volumetric Ambient Bloom (Distance ~45-95px from card edge)
          // Opacity modulates gently: ~0.11 resting up to ~0.16 on peak beat
          if (coverPulseBloom) {
            const bScale = 0.99 + smoothedBeatStrength * 0.038;
            const bOpacity = totalGlowStrength * 0.24;
            const bBlur = 44 + smoothedBeatStrength * 12;
            coverPulseBloom.style.transform = `scale(${bScale.toFixed(4)})`;
            coverPulseBloom.style.opacity = bOpacity.toFixed(3);
            coverPulseBloom.style.filter = `blur(${bBlur.toFixed(1)}px)`;
          }

          // Layer 4: Distant Background Wall Aura (Distance 100px+)
          // Opacity modulates gently: ~0.045 resting up to ~0.07 on peak beat
          if (albumAuraGlow) {
            const aScale = 0.98 + smoothedBeatStrength * 0.04;
            const aOpacity = totalGlowStrength * 0.10;
            albumAuraGlow.style.transform = `translate(-50%, -50%) scale(${aScale.toFixed(4)})`;
            albumAuraGlow.style.opacity = aOpacity.toFixed(3);
          }
        } else {
          coverCard.style.transform = "scale(1)";
          coverCard.style.boxShadow = `
            0 0 18px rgba(var(--primary-rgb), 0.32),
            0 0 36px rgba(var(--primary-rgb), 0.18),
            0 0 72px rgba(var(--secondary-rgb), 0.09),
            0 20px 50px rgba(0, 0, 0, 0.95)
          `;
          if (coverPulseRim) {
            coverPulseRim.style.transform = "scale(1)";
            coverPulseRim.style.opacity = "0.35";
          }
          if (coverPulseHalo) {
            coverPulseHalo.style.transform = "scale(1)";
            coverPulseHalo.style.opacity = "0.20";
            coverPulseHalo.style.filter = "blur(20px)";
          }
          if (coverPulseBloom) {
            coverPulseBloom.style.transform = "scale(1)";
            coverPulseBloom.style.opacity = "0.10";
            coverPulseBloom.style.filter = "blur(44px)";
          }
          if (albumAuraGlow) {
            albumAuraGlow.style.transform = "translate(-50%, -50%) scale(1)";
            albumAuraGlow.style.opacity = "0.05";
          }
        }
      }

      // Real-time celestial ambient stars: illuminate and dim smoothly with musical beat
      if (ambientStars.length > 0) {
        if (isTrackActive) {
          // Distinct frequency sensitivities per star [bass, mid, treble, beatOnset, baseScale, maxScale]
          const starProfiles = [
            { bass: 0.55, mid: 0.20, treb: 0.10, beat: 0.40, baseScale: 1.0, maxScale: 1.35 },
            { bass: 0.15, mid: 0.50, treb: 0.30, beat: 0.30, baseScale: 0.95, maxScale: 1.30 },
            { bass: 0.10, mid: 0.25, treb: 0.60, beat: 0.20, baseScale: 0.90, maxScale: 1.25 },
            { bass: 0.65, mid: 0.15, treb: 0.10, beat: 0.45, baseScale: 1.0, maxScale: 1.40 },
            { bass: 0.20, mid: 0.55, treb: 0.25, beat: 0.35, baseScale: 1.0, maxScale: 1.35 },
            { bass: 0.45, mid: 0.35, treb: 0.15, beat: 0.50, baseScale: 0.95, maxScale: 1.32 },
            { bass: 0.15, mid: 0.25, treb: 0.55, beat: 0.25, baseScale: 0.90, maxScale: 1.25 },
          ];

          ambientStars.forEach((starEl, i) => {
            const p = starProfiles[i] || starProfiles[0];
            const rawEnergy = (
              beat.bass * p.bass +
              beat.mid * p.mid +
              beat.treble * p.treb +
              beat.beat * p.beat
            );
            // Blended with smoothedBeatStrength for fluid, non-abrupt breathing
            const starStrength = Math.min(1.0, Math.max(0.0, rawEnergy * 0.45 + smoothedBeatStrength * 0.55));

            // Opacity: resting floor 0.18 -> peak 0.78
            const op = (0.18 + starStrength * 0.60).toFixed(3);
            const sc = (p.baseScale + starStrength * (p.maxScale - p.baseScale)).toFixed(3);
            const haloBlur = Math.round(6 + starStrength * 14);

            starEl.style.opacity = op;
            starEl.style.transform = `translate(-50%, -50%) scale(${sc})`;
            starEl.style.filter = `drop-shadow(0 0 ${haloBlur}px rgba(var(--primary-rgb), ${(starStrength * 0.70).toFixed(2)}))`;
          });
        } else {
          ambientStars.forEach((starEl) => {
            starEl.style.opacity = "0.14";
            starEl.style.transform = "translate(-50%, -50%) scale(1)";
            starEl.style.filter = "none";
          });
        }
      }

      // Real-time equalizer visualizer on playback badge
      if (eqBar1 && eqBar2 && eqBar3 && beat.volume > 0.01) {
        eqBar1.style.height = `${Math.max(3, Math.min(12, 3 + beat.bass * 9))}px`;
        eqBar2.style.height = `${Math.max(3, Math.min(12, 3 + beat.mid * 9))}px`;
        eqBar3.style.height = `${Math.max(3, Math.min(12, 3 + beat.treble * 9))}px`;
      }

      // Pass live beat & frequency metrics to border glow renderer
      if (renderer) {
        renderer.updateAudioBeat(beat);
      }
    });
    console.log("[SpotGlow] Subscribed to real-time audio_beat event.");
  } catch (err) {
    console.error("[SpotGlow] Failed to listen for audio_beat event:", err);
  }
});
