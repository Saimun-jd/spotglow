import { ColorRgb, DisplayMode, Mood, PlaybackStatus, TrackUpdatePayload, WindowRect } from "./main";

export type GlowStyle =
  | "aurora_flow"
  | "cyber_pulse"
  | "comet_orbit"
  | "audio_eq"
  | "plasma_storm"
  | "zen_progress";

export type GlowMode = GlowStyle | "flow" | "breathe" | "progress" | "comet" | "all";

interface ColorLerp {
  r: number;
  g: number;
  b: number;
}

/** Worst-case outward reach of the glow, per 1.0 of thickness, in logical px.
 *  (path offset + half of widest layer + blur). Used to size the overlay margin. */
const REACH_PER_THICKNESS = 16;

export class GlowRenderer {
  private canvas: HTMLCanvasElement;
  private ctx: CanvasRenderingContext2D | null;

  // Track & Palette State
  private currentPrimary: ColorLerp = { r: 30, g: 215, b: 96 };
  private currentSecondary: ColorLerp = { r: 160, g: 32, b: 240 };
  private currentAccent: ColorLerp = { r: 255, g: 255, b: 255 };

  private prevPrimary: ColorLerp = { r: 30, g: 215, b: 96 };
  private prevSecondary: ColorLerp = { r: 160, g: 32, b: 240 };
  private prevAccent: ColorLerp = { r: 255, g: 255, b: 255 };

  private targetPrimary: ColorLerp = { r: 30, g: 215, b: 96 };
  private targetSecondary: ColorLerp = { r: 160, g: 32, b: 240 };
  private targetAccent: ColorLerp = { r: 255, g: 255, b: 255 };

  private crossfadeStartTime: number = 0;
  private crossfadeDuration: number = 800;

  private status: PlaybackStatus = "playing";
  private mood: Mood = "balanced";
  private style: GlowStyle = "aurora_flow";
  private displayMode: DisplayMode = "border_glow";

  // Timeline extrapolation
  private timelinePositionMs: number = 0;
  private timelineEndMs: number = 1;
  private lastTimelineTimestamp: number = 0;

  // Window & Render State
  private isVisible: boolean = false;
  private isMaximized: boolean = false;
  private margin: number = 18; // logical px, must match the Rust overlay margin
  private cornerRadius: number = 12; // Windows 11 rounded corner radius (matches CSS var(--radius))
  private thickness: number = 1.0; // 0.5 (hairline) .. 2 (bold)

  // Intensity / paused state
  private pausedTimestamp: number = 0;
  private currentIntensity: number = 1.0;
  private desaturation: number = 0; // 0 = full color, 1 = grey (used while paused)

  // Track-change burst
  private burst: number = 0;
  private lastTitle: string = "";

  // Animation Loop
  private animFrameId: number | null = null;
  private isRunning: boolean = false;
  private lastRenderTimestamp: number = 0;
  private rotationAngle: number = 0;
  private readonly frameInterval: number = 1000 / 30; // 30 fps cap

  constructor(canvas: HTMLCanvasElement, margin: number = 18) {
    this.canvas = canvas;
    this.ctx = canvas.getContext("2d");
    this.margin = margin;

    this.resize();
    window.addEventListener("resize", () => {
      this.resize();
    });

    this.startAnimation();
  }

  // ───────────────────────── Public API ─────────────────────────

  public setMargin(margin: number) {
    this.margin = margin;
  }

  /** Thickness multiplier. 1.0 is the default thin look. */
  public setThickness(thickness: number) {
    this.thickness = Math.max(0.4, Math.min(2.5, thickness));
  }

  public getThickness(): number {
    return this.thickness;
  }

  /** Overlay margin (logical px) needed so the glow is never clipped at this thickness.
   *  Send this to Rust (set_margin) whenever thickness changes. */
  public getRecommendedMargin(): number {
    return Math.ceil(REACH_PER_THICKNESS * this.thickness) + 2;
  }

  public setStyle(style: GlowStyle | string) {
    if (
      style === "aurora_flow" ||
      style === "cyber_pulse" ||
      style === "comet_orbit" ||
      style === "audio_eq" ||
      style === "plasma_storm" ||
      style === "zen_progress"
    ) {
      this.style = style;
    } else if (style === "flow" || style === "all") {
      this.style = "aurora_flow";
    } else if (style === "breathe") {
      this.style = "cyber_pulse";
    } else if (style === "comet") {
      this.style = "comet_orbit";
    } else if (style === "progress") {
      this.style = "zen_progress";
    }
    this.startAnimation();
  }

  public getStyle(): GlowStyle {
    return this.style;
  }

  public setMode(mode: GlowMode) {
    this.setStyle(mode);
  }

  public getMode(): GlowMode {
    return this.style;
  }

  public setDisplayMode(mode: DisplayMode) {
    this.displayMode = mode;
    this.startAnimation();
  }

  public getDisplayMode(): DisplayMode {
    return this.displayMode;
  }

  public updateTrack(payload: TrackUpdatePayload) {
    const rawPrimary = payload.palette?.primary || { r: 30, g: 215, b: 96 };
    const rawSecondary = payload.palette?.secondary || { r: 160, g: 32, b: 240 };
    const rawAccent = payload.palette?.accent || { r: 255, g: 255, b: 255 };

    // Set up color crossfade from whatever is currently on screen
    this.prevPrimary = { ...this.currentPrimary };
    this.prevSecondary = { ...this.currentSecondary };
    this.prevAccent = { ...this.currentAccent };

    this.targetPrimary = this.ensureVibrant(rawPrimary);
    this.targetSecondary = this.ensureVibrant(rawSecondary);
    this.targetAccent = this.ensureVibrant(rawAccent);

    this.crossfadeStartTime = performance.now();

    // Track change → flare
    const title = payload.title ?? "";
    if (title && title !== this.lastTitle) {
      this.burst = 1;
      this.lastTitle = title;
    }

    // Timeline extrapolation sync
    if (payload.timeline) {
      this.timelinePositionMs = payload.timeline.position_ms;
      this.timelineEndMs = Math.max(1, payload.timeline.end_time_ms);
      this.lastTimelineTimestamp = performance.now();
    }

    // Playback status
    if (payload.status === "paused" && this.status !== "paused") {
      this.pausedTimestamp = performance.now();
    }
    this.status = payload.status;
    this.mood = payload.mood || "balanced";

    this.startAnimation();
  }

  private lastRect: WindowRect | null = null;

  public updateWindow(rect: WindowRect, visible: boolean, minimized: boolean, maximized: boolean = false) {
    this.isVisible = visible && !minimized && rect.width > 0 && rect.height > 0;
    this.isMaximized = maximized;
    this.lastRect = rect;
    this.resize();
    if (this.isVisible) {
      this.startAnimation();
    } else {
      this.clear();
    }
  }

  public resize() {
    const dpr = window.devicePixelRatio || 1;
    const width = window.innerWidth;
    const height = window.innerHeight;

    if (
      this.canvas.width !== Math.round(width * dpr) ||
      this.canvas.height !== Math.round(height * dpr)
    ) {
      this.canvas.width = Math.round(width * dpr);
      this.canvas.height = Math.round(height * dpr);
    }
  }

  public clear() {
    this.stopAnimation();
    if (!this.ctx) return;
    this.ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);
  }

  // ───────────────────────── Color helpers ─────────────────────────

  private ensureVibrant(c: ColorRgb): ColorLerp {
    const maxVal = Math.max(c.r, c.g, c.b);
    let r = c.r;
    let g = c.g;
    let b = c.b;

    // Guarantee intense luminous peak (scaled up to at least 235 out of 255)
    if (maxVal < 235) {
      const factor = 235 / Math.max(1, maxVal);
      r = Math.min(255, Math.round(r * factor));
      g = Math.min(255, Math.round(g * factor));
      b = Math.min(255, Math.round(b * factor));
    }

    // Boost saturation: widen separation between dominant and recessive channels
    const newMax = Math.max(r, g, b);
    const newMin = Math.min(r, g, b);
    if (newMax - newMin < 70 && newMax > 0) {
      const avg = (r + g + b) / 3;
      r = Math.min(255, Math.max(0, Math.round(avg + (r - avg) * 1.45)));
      g = Math.min(255, Math.max(0, Math.round(avg + (g - avg) * 1.45)));
      b = Math.min(255, Math.max(0, Math.round(avg + (b - avg) * 1.45)));
    }

    return { r, g, b };
  }

  private lerpColor(a: ColorLerp, b: ColorLerp, t: number): ColorLerp {
    return {
      r: Math.round(a.r + (b.r - a.r) * t),
      g: Math.round(a.g + (b.g - a.g) * t),
      b: Math.round(a.b + (b.b - a.b) * t),
    };
  }

  /** Mix a color toward its own luminance grey. amount 0..1 */
  private desaturate(c: ColorLerp, amount: number): ColorLerp {
    if (amount <= 0.001) return c;
    const lum = 0.299 * c.r + 0.587 * c.g + 0.114 * c.b;
    return this.lerpColor(c, { r: lum, g: lum, b: lum }, amount);
  }

  private rgbString(c: ColorLerp, alpha: number = 1): string {
    return `rgba(${c.r}, ${c.g}, ${c.b}, ${Math.max(0, Math.min(1, alpha))})`;
  }

  // ───────────────────────── Animation loop ─────────────────────────

  private startAnimation() {
    if (this.isRunning) return;
    this.isRunning = true;
    this.lastRenderTimestamp = performance.now() - this.frameInterval;
    this.loop(performance.now());
  }

  private stopAnimation() {
    this.isRunning = false;
    if (this.animFrameId !== null) {
      cancelAnimationFrame(this.animFrameId);
      this.animFrameId = null;
    }
  }

  private loop = (timestamp: number) => {
    if (!this.isRunning) return;

    // 30 fps cap: skip this display frame if we rendered too recently
    if (timestamp - this.lastRenderTimestamp < this.frameInterval - 1) {
      this.animFrameId = requestAnimationFrame(this.loop);
      return;
    }

    const delta = Math.min(100, timestamp - this.lastRenderTimestamp);
    this.lastRenderTimestamp = timestamp;

    this.render(timestamp, delta);

    if (this.isVisible && (this.status === "playing" || this.currentIntensity > 0.01)) {
      this.animFrameId = requestAnimationFrame(this.loop);
    } else {
      this.isRunning = false;
      this.animFrameId = null;
    }
  };

  // ───────────────────────── Geometry helpers ─────────────────────────

  private roundedRectPath(
    ctx: CanvasRenderingContext2D,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number
  ) {
    const radius = Math.max(0, Math.min(r, w / 2, h / 2));
    if (typeof ctx.roundRect === "function") {
      ctx.beginPath();
      ctx.roundRect(x, y, w, h, radius);
      return;
    }

    ctx.beginPath();
    ctx.moveTo(x + radius, y);
    ctx.lineTo(x + w - radius, y);
    ctx.arcTo(x + w, y, x + w, y + radius, radius);
    ctx.lineTo(x + w, y + h - radius);
    ctx.arcTo(x + w, y + h, x + w - radius, y + h, radius);
    ctx.lineTo(x + radius, y + h);
    ctx.arcTo(x, y + h, x, y + h - radius, radius);
    ctx.lineTo(x, y + radius);
    ctx.arcTo(x, y, x + radius, y, radius);
    ctx.closePath();
  }

  /** Point at distance `d` along the rounded-rect path (clockwise, starting at (x+r, y)),
   *  matching how roundRect / the fallback path is drawn, so dashes and dots line up. */
  private pointOnPath(
    d: number,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number
  ): { x: number; y: number } {
    const radius = Math.max(0, Math.min(r, w / 2, h / 2));
    const sw = Math.max(0, w - 2 * radius);
    const sh = Math.max(0, h - 2 * radius);
    const arc = (Math.PI * radius) / 2;
    const quarter = Math.PI / 2;
    let t = d;

    if (t <= sw) return { x: x + radius + t, y };
    t -= sw;

    if (arc > 0 && t <= arc) {
      const a = (t / arc) * quarter;
      return { x: x + w - radius + Math.sin(a) * radius, y: y + radius - Math.cos(a) * radius };
    }
    t -= arc;

    if (t <= sh) return { x: x + w, y: y + radius + t };
    t -= sh;

    if (arc > 0 && t <= arc) {
      const a = (t / arc) * quarter;
      return { x: x + w - radius + Math.cos(a) * radius, y: y + h - radius + Math.sin(a) * radius };
    }
    t -= arc;

    if (t <= sw) return { x: x + w - radius - t, y: y + h };
    t -= sw;

    if (arc > 0 && t <= arc) {
      const a = (t / arc) * quarter;
      return { x: x + radius - Math.sin(a) * radius, y: y + h - radius + Math.cos(a) * radius };
    }
    t -= arc;

    if (t <= sh) return { x, y: y + h - radius - t };
    t -= sh;

    const a = arc > 0 ? (Math.min(t, arc) / arc) * quarter : 0;
    return { x: x + radius - Math.cos(a) * radius, y: y + radius - Math.sin(a) * radius };
  }

  // ───────────────────────── Render ─────────────────────────

  private render(timestamp: number, delta: number) {
    if (!this.ctx) return;

    const ctx = this.ctx;
    const dpr = window.devicePixelRatio || 1;
    const width = window.innerWidth;
    const height = window.innerHeight;

    // Clear without stopping the loop
    ctx.clearRect(0, 0, this.canvas.width, this.canvas.height);

    if (!this.isVisible || this.status === "closed" || width <= 0 || height <= 0) {
      return;
    }

    // 1. Color crossfade (smoothstep)
    const crossfadeT = Math.min(
      1,
      Math.max(0, (timestamp - this.crossfadeStartTime) / this.crossfadeDuration)
    );
    const smoothT = crossfadeT * crossfadeT * (3 - 2 * crossfadeT);

    this.currentPrimary = this.lerpColor(this.prevPrimary, this.targetPrimary, smoothT);
    this.currentSecondary = this.lerpColor(this.prevSecondary, this.targetSecondary, smoothT);
    this.currentAccent = this.lerpColor(this.prevAccent, this.targetAccent, smoothT);

    // 2. Intensity + paused fade-out + desaturation
    if (this.status === "playing") {
      this.currentIntensity = Math.min(1.0, this.currentIntensity + delta * 0.003);
      this.desaturation = Math.max(0, this.desaturation - delta * 0.002);
    } else if (this.status === "paused") {
      const pausedElapsed = (timestamp - this.pausedTimestamp) / 1000;
      this.desaturation = Math.min(0.75, this.desaturation + delta * 0.0015);
      if (pausedElapsed < 10) {
        const targetDim = 0.2;
        this.currentIntensity += (targetDim - this.currentIntensity) * Math.min(1, delta * 0.003);
      } else {
        const fadeElapsed = pausedElapsed - 10;
        this.currentIntensity = Math.max(0, 0.2 * (1 - Math.min(1, fadeElapsed / 2.0)));
      }
    } else {
      this.currentIntensity = 0;
    }

    if (this.currentIntensity <= 0.001) {
      return;
    }

    // Colors actually drawn this frame
    const cPrimary = this.desaturate(this.currentPrimary, this.desaturation);
    const cSecondary = this.desaturate(this.currentSecondary, this.desaturation);
    const cAccent = this.desaturate(this.currentAccent, this.desaturation);

    // 3. Mood tuning
    let speedMult = 1.0;
    let blurMult = 1.0;
    switch (this.mood) {
      case "vivid":
        speedMult = 1.35;
        blurMult = 1.15;
        break;
      case "mellow":
        speedMult = 0.65;
        blurMult = 0.9;
        break;
      case "monochrome":
        speedMult = 0.55;
        blurMult = 0.85;
        break;
      case "dual_tone":
        speedMult = 1.1;
        blurMult = 1.05;
        break;
      case "balanced":
      default:
        break;
    }

    // 4. Burst decay (track-change flare)
    this.burst = Math.max(0, this.burst - delta * 0.0012);
    const burstBoost = 1 + this.burst * 0.6;
    const activeIntensity = Math.min(1, this.currentIntensity * burstBoost);

    // 5. Geometry
    const T = this.thickness * (this.isMaximized ? 1.3 : 1.0);
    const core = 1.2 * T;
    let m = 0;
    if (!this.isMaximized) {
      m = this.margin;
      if (this.lastRect && this.lastRect.width > 0) {
        const derived = (width - this.lastRect.width / dpr) / 2;
        if (derived > 0 && derived < 100) m = derived;
      }
    }
    const isCoverArt = this.displayMode === "cover_art";
    const off = this.isMaximized ? core / 2 : (isCoverArt ? 0 : -core / 2);
    const x = m + off;
    const y = m + off;
    const w = Math.max(0, width - (m + off) * 2);
    const h = Math.max(0, height - (m + off) * 2);
    const r = this.isMaximized ? 0 : (isCoverArt ? this.cornerRadius : this.cornerRadius + core / 2);
    const cx = x + w / 2;
    const cy = y + h / 2;
    const perimeter = Math.max(
      1,
      2 * (w - 2 * Math.min(r, w / 2, h / 2)) +
        2 * (h - 2 * Math.min(r, w / 2, h / 2)) +
        2 * Math.PI * Math.min(r, w / 2, h / 2)
    );

    ctx.save();
    ctx.scale(dpr, dpr);
    ctx.lineCap = "round";
    ctx.lineJoin = "round";

    // Dispatch to selected style renderer
    switch (this.style) {
      case "cyber_pulse":
        this.renderCyberPulse(ctx, dpr, timestamp, delta, speedMult, blurMult, activeIntensity, T, core, x, y, w, h, r, cx, cy, perimeter, cPrimary, cSecondary, cAccent);
        break;
      case "comet_orbit":
        this.renderCometOrbit(ctx, dpr, timestamp, delta, speedMult, blurMult, activeIntensity, T, core, x, y, w, h, r, cx, cy, perimeter, cPrimary, cSecondary, cAccent);
        break;
      case "audio_eq":
        this.renderAudioEq(ctx, dpr, timestamp, delta, speedMult, blurMult, activeIntensity, T, core, x, y, w, h, r, cx, cy, perimeter, cPrimary, cSecondary, cAccent);
        break;
      case "plasma_storm":
        this.renderPlasmaStorm(ctx, dpr, timestamp, delta, speedMult, blurMult, activeIntensity, T, core, x, y, w, h, r, cx, cy, perimeter, cPrimary, cSecondary, cAccent);
        break;
      case "zen_progress":
        this.renderZenProgress(ctx, dpr, timestamp, delta, speedMult, blurMult, activeIntensity, T, core, x, y, w, h, r, cx, cy, perimeter, cPrimary, cSecondary, cAccent);
        break;
      case "aurora_flow":
      default:
        this.renderAuroraFlow(ctx, dpr, timestamp, delta, speedMult, blurMult, activeIntensity, T, core, x, y, w, h, r, cx, cy, perimeter, cPrimary, cSecondary, cAccent);
        break;
    }

    ctx.restore();
  }

  // ───────────────────────── Style 1: Aurora Flow ─────────────────────────

  private renderAuroraFlow(
    ctx: CanvasRenderingContext2D,
    dpr: number,
    timestamp: number,
    delta: number,
    speedMult: number,
    blurMult: number,
    activeIntensity: number,
    T: number,
    core: number,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
    cx: number,
    cy: number,
    _perimeter: number,
    cPrimary: ColorLerp,
    cSecondary: ColorLerp,
    cAccent: ColorLerp
  ) {
    this.rotationAngle = (this.rotationAngle + delta * 0.00065 * speedMult) % (Math.PI * 2);
    const breatheFactor = 0.85 + 0.15 * Math.sin(timestamp * 0.0018 * speedMult);

    let grad: CanvasGradient;
    if (typeof ctx.createConicGradient === "function") {
      grad = ctx.createConicGradient(this.rotationAngle, cx, cy);
      grad.addColorStop(0.0, this.rgbString(cPrimary));
      grad.addColorStop(0.33, this.rgbString(cSecondary));
      grad.addColorStop(0.66, this.rgbString(cAccent));
      grad.addColorStop(1.0, this.rgbString(cPrimary));
    } else {
      const cos = Math.cos(this.rotationAngle);
      const sin = Math.sin(this.rotationAngle);
      const gradLen = Math.max(w, h);
      grad = ctx.createLinearGradient(
        cx - cos * gradLen * 0.5,
        cy - sin * gradLen * 0.5,
        cx + cos * gradLen * 0.5,
        cy + sin * gradLen * 0.5
      );
      grad.addColorStop(0.0, this.rgbString(cPrimary));
      grad.addColorStop(0.5, this.rgbString(cSecondary));
      grad.addColorStop(1.0, this.rgbString(cAccent));
    }

    ctx.strokeStyle = grad;

    const layers = [
      {
        lw: 2.8 * T,
        blur: 5.0 * T * blurMult * breatheFactor,
        alpha: 0.70,
        color: cPrimary,
      },
      {
        lw: 1.8 * T,
        blur: 2.8 * T * blurMult,
        alpha: 0.85,
        color: cSecondary,
      },
      {
        lw: core * 1.15,
        blur: 1.4 * T,
        alpha: 1.0,
        color: cAccent,
      },
    ];

    for (const layer of layers) {
      ctx.save();
      ctx.globalAlpha = layer.alpha * activeIntensity;
      ctx.lineWidth = layer.lw;
      ctx.shadowColor = this.rgbString(layer.color, 1.0);
      ctx.shadowBlur = layer.blur * dpr;
      this.roundedRectPath(ctx, x, y, w, h, r);
      ctx.stroke();
      ctx.restore();
    }

    // Incandescent hot-core laser line for high-energy brilliance
    ctx.save();
    ctx.globalAlpha = 0.92 * activeIntensity;
    ctx.lineWidth = core * 0.55;
    ctx.strokeStyle = "#ffffff";
    ctx.shadowColor = this.rgbString(cAccent, 1.0);
    ctx.shadowBlur = 2.5 * T * dpr;
    this.roundedRectPath(ctx, x, y, w, h, r);
    ctx.stroke();
    ctx.restore();
  }

  // ───────────────────────── Style 2: Cyber Pulse ─────────────────────────

  private renderCyberPulse(
    ctx: CanvasRenderingContext2D,
    dpr: number,
    timestamp: number,
    _delta: number,
    speedMult: number,
    blurMult: number,
    activeIntensity: number,
    T: number,
    core: number,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
    _cx: number,
    _cy: number,
    _perimeter: number,
    cPrimary: ColorLerp,
    _cSecondary: ColorLerp,
    cAccent: ColorLerp
  ) {
    // Cardiac rhythmic waveform: primary thump + secondary reverberation
    const omega = (timestamp * 0.0035 * speedMult) % (Math.PI * 2);
    const s1 = Math.max(0, Math.sin(omega));
    const beat1 = Math.pow(s1, 1.8);
    const s2 = Math.max(0, Math.sin(omega * 2));
    const beat2 = 0.32 * Math.pow(s2, 2.5);
    const pulse = Math.min(1.0, beat1 + beat2);

    const outerLw = (1.8 + 2.2 * pulse) * T;
    const outerBlur = (3.5 + 5.5 * pulse) * T * blurMult;

    // 1. Outward pulsating diffuse aura
    ctx.save();
    ctx.globalAlpha = (0.55 + 0.45 * pulse) * activeIntensity;
    ctx.lineWidth = outerLw;
    ctx.strokeStyle = this.rgbString(cPrimary);
    ctx.shadowColor = this.rgbString(cPrimary, 1.0);
    ctx.shadowBlur = outerBlur * dpr;
    this.roundedRectPath(ctx, x, y, w, h, r);
    ctx.stroke();
    ctx.restore();

    // 2. High-contrast neon laser core
    ctx.save();
    ctx.globalAlpha = 1.0 * activeIntensity;
    ctx.lineWidth = core * 1.15;
    ctx.strokeStyle = this.rgbString(pulse > 0.55 ? { r: 255, g: 255, b: 255 } : cAccent);
    ctx.shadowColor = this.rgbString(cAccent, 1.0);
    ctx.shadowBlur = (2.0 + 4.0 * pulse) * T * dpr;
    this.roundedRectPath(ctx, x, y, w, h, r);
    ctx.stroke();
    ctx.restore();

    // 3. Corner neon brackets (accentuate corners on beat pulses)
    if (pulse > 0.25 && !this.isMaximized) {
      const pingAlpha = Math.min(1.0, (pulse - 0.25) / 0.75 * 1.2 * activeIntensity);
      const bracketLen = 24 * T;
      ctx.save();
      ctx.globalAlpha = pingAlpha;
      ctx.lineWidth = core * 1.6;
      ctx.strokeStyle = "#ffffff";
      ctx.shadowColor = this.rgbString(cAccent, 1.0);
      ctx.shadowBlur = 12 * T * dpr;

      // Top-Left corner
      ctx.beginPath();
      ctx.moveTo(x, y + bracketLen);
      ctx.lineTo(x, y + r);
      ctx.arcTo(x, y, x + r, y, r);
      ctx.lineTo(x + bracketLen, y);
      ctx.stroke();

      // Top-Right corner
      ctx.beginPath();
      ctx.moveTo(x + w - bracketLen, y);
      ctx.lineTo(x + w - r, y);
      ctx.arcTo(x + w, y, x + w, y + r, r);
      ctx.lineTo(x + w, y + bracketLen);
      ctx.stroke();

      // Bottom-Right corner
      ctx.beginPath();
      ctx.moveTo(x + w, y + h - bracketLen);
      ctx.lineTo(x + w, y + h - r);
      ctx.arcTo(x + w, y + h, x + w - r, y + h, r);
      ctx.lineTo(x + w - bracketLen, y + h);
      ctx.stroke();

      // Bottom-Left corner
      ctx.beginPath();
      ctx.moveTo(x + bracketLen, y + h);
      ctx.lineTo(x + r, y + h);
      ctx.arcTo(x, y + h, x, y + h - r, r);
      ctx.lineTo(x, y + h - bracketLen);
      ctx.stroke();

      ctx.restore();
    }
  }

  // ───────────────────────── Style 3: Comet Orbit ─────────────────────────

  private renderCometOrbit(
    ctx: CanvasRenderingContext2D,
    dpr: number,
    timestamp: number,
    _delta: number,
    speedMult: number,
    _blurMult: number,
    activeIntensity: number,
    T: number,
    core: number,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
    _cx: number,
    _cy: number,
    perimeter: number,
    cPrimary: ColorLerp,
    cSecondary: ColorLerp,
    cAccent: ColorLerp
  ) {
    // 1. Radiant orbital base frame
    ctx.save();
    ctx.globalAlpha = 0.30 * activeIntensity;
    ctx.lineWidth = core * 1.1;
    ctx.strokeStyle = this.rgbString(cPrimary);
    ctx.shadowColor = this.rgbString(cPrimary, 1.0);
    ctx.shadowBlur = 3.0 * T * dpr;
    this.roundedRectPath(ctx, x, y, w, h, r);
    ctx.stroke();
    ctx.restore();

    // 2. Comet Alpha (Primary color with incandescent white head, sweeps clockwise)
    const cometLen1 = perimeter * 0.28;
    const steps1 = 9;
    const seg1 = cometLen1 / steps1;
    const headPos1 = ((timestamp * 0.00010 * speedMult) % 1) * perimeter;

    ctx.save();
    ctx.lineCap = "butt";
    ctx.lineWidth = core * 1.5;
    ctx.shadowColor = this.rgbString(cPrimary, 1.0);
    ctx.shadowBlur = 6.0 * T * dpr;

    for (let k = 0; k < steps1; k++) {
      const segStart = headPos1 - cometLen1 + k * seg1;
      const wrapped = ((segStart % perimeter) + perimeter) % perimeter;
      const norm = (k + 1) / steps1;
      const a = norm * norm;
      ctx.globalAlpha = a * activeIntensity;
      ctx.strokeStyle = this.rgbString(norm > 0.85 ? { r: 255, g: 255, b: 255 } : cPrimary);
      ctx.setLineDash([seg1 + 0.5, perimeter - seg1]);
      ctx.lineDashOffset = -wrapped;
      this.roundedRectPath(ctx, x, y, w, h, r);
      ctx.stroke();
    }
    ctx.restore();

    // Comet Alpha Head Particle Orb
    const pt1 = this.pointOnPath(headPos1, x, y, w, h, r);
    ctx.save();
    ctx.globalAlpha = activeIntensity;
    ctx.fillStyle = "#ffffff";
    ctx.shadowColor = this.rgbString(cPrimary, 1.0);
    ctx.shadowBlur = 14 * T * dpr;
    ctx.beginPath();
    ctx.arc(pt1.x, pt1.y, 3.4 * T, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    // 3. Comet Beta (Secondary & Accent colors, sweeps counter-direction/offset)
    const cometLen2 = perimeter * 0.20;
    const steps2 = 7;
    const seg2 = cometLen2 / steps2;
    const headPos2 = ((1 - ((timestamp * 0.000085 * speedMult + 0.5) % 1)) % 1) * perimeter;

    ctx.save();
    ctx.lineCap = "butt";
    ctx.lineWidth = core * 1.3;
    ctx.shadowColor = this.rgbString(cSecondary, 1.0);
    ctx.shadowBlur = 5.0 * T * dpr;

    for (let k = 0; k < steps2; k++) {
      const segStart = headPos2 - cometLen2 + k * seg2;
      const wrapped = ((segStart % perimeter) + perimeter) % perimeter;
      const norm = (k + 1) / steps2;
      const a = norm * norm;
      ctx.globalAlpha = a * 0.95 * activeIntensity;
      ctx.strokeStyle = this.rgbString(norm > 0.8 ? cAccent : cSecondary);
      ctx.setLineDash([seg2 + 0.5, perimeter - seg2]);
      ctx.lineDashOffset = -wrapped;
      this.roundedRectPath(ctx, x, y, w, h, r);
      ctx.stroke();
    }
    ctx.restore();

    // Comet Beta Head Particle Orb
    const pt2 = this.pointOnPath(headPos2, x, y, w, h, r);
    ctx.save();
    ctx.globalAlpha = activeIntensity;
    ctx.fillStyle = "#ffffff";
    ctx.shadowColor = this.rgbString(cAccent, 1.0);
    ctx.shadowBlur = 12 * T * dpr;
    ctx.beginPath();
    ctx.arc(pt2.x, pt2.y, 2.8 * T, 0, Math.PI * 2);
    ctx.fill();
    ctx.restore();

    // 4. Crossing flash flare (when two comets cross nearby)
    const distSq = (pt1.x - pt2.x) * (pt1.x - pt2.x) + (pt1.y - pt2.y) * (pt1.y - pt2.y);
    if (distSq < 1600) {
      const prox = 1 - Math.sqrt(distSq) / 40;
      ctx.save();
      ctx.globalAlpha = prox * activeIntensity;
      ctx.fillStyle = "#ffffff";
      ctx.shadowColor = "#ffffff";
      ctx.shadowBlur = 18 * T * dpr;
      const mx = (pt1.x + pt2.x) / 2;
      const my = (pt1.y + pt2.y) / 2;
      ctx.beginPath();
      ctx.arc(mx, my, 4.0 * T * prox, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
  }

  // ───────────────────────── Style 4: Audio EQ Bars ─────────────────────────

  private renderAudioEq(
    ctx: CanvasRenderingContext2D,
    dpr: number,
    timestamp: number,
    _delta: number,
    speedMult: number,
    _blurMult: number,
    activeIntensity: number,
    T: number,
    _core: number,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
    _cx: number,
    _cy: number,
    perimeter: number,
    cPrimary: ColorLerp,
    cSecondary: ColorLerp,
    cAccent: ColorLerp
  ) {
    const N = 56;
    const step = perimeter / N;
    const dashLen = step * 0.68;

    ctx.save();
    ctx.lineCap = "round";

    for (let i = 0; i < N; i++) {
      const pos = i * step;
      const normPos = pos / perimeter;

      // Simulated multi-band frequency response:
      // - normPos 0.5..0.75 (bottom border) = rhythmic bass
      // - normPos 0.25..0.5 & 0.75..1.0 (sides) = mid synth/vocal waves
      // - normPos 0.0..0.25 (top) = sparkling treble
      let amp = 0.2;
      if (normPos >= 0.45 && normPos <= 0.75) {
        // Bass band
        const bassWave = Math.abs(Math.sin(timestamp * 0.0055 * speedMult + i * 0.22));
        amp = 0.25 + 0.75 * Math.pow(bassWave, 1.5);
      } else if (normPos < 0.25) {
        // Treble band
        const trebleWave = Math.abs(Math.sin(timestamp * 0.009 * speedMult + i * 0.85));
        amp = 0.15 + 0.85 * Math.pow(trebleWave, 2.0);
      } else {
        // Mid-range band
        const midWave = Math.abs(Math.sin(timestamp * 0.004 * speedMult + i * 0.45));
        amp = 0.3 + 0.7 * midWave;
      }

      const barLw = (1.3 + 2.2 * amp) * T;
      const barBlur = (2.5 + 4.5 * amp) * T;
      const color = amp > 0.7 ? cAccent : (amp > 0.4 ? cPrimary : cSecondary);

      ctx.save();
      ctx.globalAlpha = (0.55 + 0.45 * amp) * activeIntensity;
      ctx.lineWidth = barLw;
      ctx.strokeStyle = this.rgbString(color);
      ctx.shadowColor = this.rgbString(color, 1.0);
      ctx.shadowBlur = barBlur * dpr;
      ctx.setLineDash([dashLen, perimeter - dashLen]);
      ctx.lineDashOffset = -pos;
      this.roundedRectPath(ctx, x, y, w, h, r);
      ctx.stroke();
      ctx.restore();
    }

    ctx.restore();
  }

  // ───────────────────────── Style 5: Plasma Storm ─────────────────────────

  private renderPlasmaStorm(
    ctx: CanvasRenderingContext2D,
    dpr: number,
    timestamp: number,
    delta: number,
    speedMult: number,
    blurMult: number,
    activeIntensity: number,
    T: number,
    core: number,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
    cx: number,
    cy: number,
    _perimeter: number,
    cPrimary: ColorLerp,
    cSecondary: ColorLerp,
    cAccent: ColorLerp
  ) {
    this.rotationAngle = (this.rotationAngle + delta * 0.0009 * speedMult) % (Math.PI * 2);
    const counterAngle = (-timestamp * 0.0012 * speedMult) % (Math.PI * 2);

    let grad1: CanvasGradient;
    let grad2: CanvasGradient;

    if (typeof ctx.createConicGradient === "function") {
      grad1 = ctx.createConicGradient(this.rotationAngle, cx, cy);
      grad1.addColorStop(0.0, this.rgbString(cPrimary));
      grad1.addColorStop(0.5, this.rgbString(cSecondary));
      grad1.addColorStop(1.0, this.rgbString(cPrimary));

      grad2 = ctx.createConicGradient(counterAngle, cx, cy);
      grad2.addColorStop(0.0, this.rgbString(cSecondary));
      grad2.addColorStop(0.5, this.rgbString(cAccent));
      grad2.addColorStop(1.0, this.rgbString(cSecondary));
    } else {
      grad1 = ctx.createLinearGradient(x, y, x + w, y + h);
      grad1.addColorStop(0.0, this.rgbString(cPrimary));
      grad1.addColorStop(1.0, this.rgbString(cSecondary));

      grad2 = ctx.createLinearGradient(x + w, y, x, y + h);
      grad2.addColorStop(0.0, this.rgbString(cSecondary));
      grad2.addColorStop(1.0, this.rgbString(cAccent));
    }

    // Outer plasma wave surge
    ctx.save();
    ctx.globalAlpha = 0.70 * activeIntensity;
    ctx.lineWidth = 2.4 * T;
    ctx.strokeStyle = grad1;
    ctx.shadowColor = this.rgbString(cPrimary, 1.0);
    ctx.shadowBlur = 5.5 * T * blurMult * dpr;
    this.roundedRectPath(ctx, x, y, w, h, r);
    ctx.stroke();
    ctx.restore();

    // Opposing liquid chroma wave surge
    ctx.save();
    ctx.globalAlpha = 0.85 * activeIntensity;
    ctx.lineWidth = 1.6 * T;
    ctx.strokeStyle = grad2;
    ctx.shadowColor = this.rgbString(cAccent, 1.0);
    ctx.shadowBlur = 3.6 * T * blurMult * dpr;
    this.roundedRectPath(ctx, x, y, w, h, r);
    ctx.stroke();
    ctx.restore();

    // Central electric filament
    ctx.save();
    ctx.globalAlpha = 1.0 * activeIntensity;
    ctx.lineWidth = core * 1.25;
    ctx.strokeStyle = this.rgbString(cAccent);
    ctx.shadowColor = "#ffffff";
    ctx.shadowBlur = 2.5 * T * dpr;
    this.roundedRectPath(ctx, x, y, w, h, r);
    ctx.stroke();
    ctx.restore();

    // 4 Corner Plasma Flare Nodes
    if (!this.isMaximized) {
      const corners = [
        { cx: x + r, cy: y + r },
        { cx: x + w - r, cy: y + r },
        { cx: x + w - r, cy: y + h - r },
        { cx: x + r, cy: y + h - r },
      ];

      for (let k = 0; k < 4; k++) {
        const flare = 0.5 + 0.5 * Math.sin(timestamp * 0.0035 * speedMult + k * 1.57);
        ctx.save();
        ctx.globalAlpha = (0.55 + 0.45 * flare) * activeIntensity;
        ctx.fillStyle = "#ffffff";
        ctx.shadowColor = this.rgbString(cAccent, 1.0);
        ctx.shadowBlur = (6.0 + 12.0 * flare) * T * dpr;
        ctx.beginPath();
        ctx.arc(corners[k].cx, corners[k].cy, (2.2 + 1.4 * flare) * T, 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
      }
    }
  }

  // ───────────────────────── Style 6: Zen Progress ─────────────────────────

  private renderZenProgress(
    ctx: CanvasRenderingContext2D,
    dpr: number,
    timestamp: number,
    _delta: number,
    _speedMult: number,
    _blurMult: number,
    activeIntensity: number,
    T: number,
    core: number,
    x: number,
    y: number,
    w: number,
    h: number,
    r: number,
    _cx: number,
    _cy: number,
    perimeter: number,
    cPrimary: ColorLerp,
    _cSecondary: ColorLerp,
    cAccent: ColorLerp
  ) {
    // 1. Radiant ambient baseline framing
    ctx.save();
    ctx.globalAlpha = 0.32 * activeIntensity;
    ctx.lineWidth = core * 1.0;
    ctx.strokeStyle = this.rgbString(cPrimary);
    ctx.shadowColor = this.rgbString(cPrimary, 1.0);
    ctx.shadowBlur = 2.5 * T * dpr;
    this.roundedRectPath(ctx, x, y, w, h, r);
    ctx.stroke();
    ctx.restore();

    // 2. Real-time song progress arc
    let progress = 0;
    if (this.timelineEndMs > 0) {
      const extrapolatedMs =
        this.status === "playing"
          ? this.timelinePositionMs + (performance.now() - this.lastTimelineTimestamp)
          : this.timelinePositionMs;
      progress = Math.max(0, Math.min(1, extrapolatedMs / this.timelineEndMs));
    }

    const arcLength = perimeter * progress;

    if (arcLength > 1) {
      // Completed progress line
      ctx.save();
      ctx.setLineDash([arcLength, perimeter]);
      ctx.lineDashOffset = 0;
      ctx.lineWidth = 1.8 * T;
      ctx.strokeStyle = this.rgbString(cPrimary, activeIntensity);
      ctx.shadowColor = this.rgbString(cPrimary, 1.0);
      ctx.shadowBlur = 6.0 * T * dpr;
      this.roundedRectPath(ctx, x, y, w, h, r);
      ctx.stroke();
      ctx.restore();

      // Playhead Beacon at the tip of the progress arc
      const head = this.pointOnPath(arcLength, x, y, w, h, r);
      const beaconBreath = 0.85 + 0.15 * Math.sin(timestamp * 0.003);

      // Outer aura
      ctx.save();
      ctx.globalAlpha = 0.9 * activeIntensity;
      ctx.strokeStyle = this.rgbString(cAccent, 1.0);
      ctx.shadowColor = this.rgbString(cAccent, 1.0);
      ctx.shadowBlur = 12 * T * dpr;
      ctx.lineWidth = 1.4 * T;
      ctx.beginPath();
      ctx.arc(head.x, head.y, 4.5 * T * beaconBreath, 0, Math.PI * 2);
      ctx.stroke();
      ctx.restore();

      // Inner solid incandescent core
      ctx.save();
      ctx.globalAlpha = activeIntensity;
      ctx.fillStyle = "#ffffff";
      ctx.shadowColor = "#ffffff";
      ctx.shadowBlur = 8 * T * dpr;
      ctx.beginPath();
      ctx.arc(head.x, head.y, 2.6 * T, 0, Math.PI * 2);
      ctx.fill();
      ctx.restore();
    }
  }
}