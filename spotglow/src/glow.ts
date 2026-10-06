import { ColorRgb, Mood, PlaybackStatus, TrackUpdatePayload, WindowRect } from "./main";

export type GlowMode = "flow" | "breathe" | "progress" | "comet" | "all";

interface ColorLerp {
  r: number;
  g: number;
  b: number;
}

interface EffectSet {
  breathe: boolean;
  flow: boolean;
  comet: boolean;
  progress: boolean;
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
  private mode: GlowMode = "all";

  // Timeline extrapolation
  private timelinePositionMs: number = 0;
  private timelineEndMs: number = 1;
  private lastTimelineTimestamp: number = 0;

  // Window & Render State
  private isVisible: boolean = false;
  private isMaximized: boolean = false;
  private margin: number = 18; // logical px, must match the Rust overlay margin
  private cornerRadius: number = 8; // Windows 11 window corner radius
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

  public setMode(mode: GlowMode) {
    this.mode = mode;
  }

  public getMode(): GlowMode {
    return this.mode;
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
    if (maxVal < 140) {
      const factor = 140 / Math.max(1, maxVal);
      return {
        r: Math.min(255, Math.round(c.r * factor)),
        g: Math.min(255, Math.round(c.g * factor)),
        b: Math.min(255, Math.round(c.b * factor)),
      };
    }
    return { r: c.r, g: c.g, b: c.b };
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

  // ───────────────────────── Effect presets ─────────────────────────

  private activeEffects(): EffectSet {
    switch (this.mode) {
      case "breathe":
        return { breathe: true, flow: false, comet: false, progress: false };
      case "flow":
        return { breathe: false, flow: true, comet: false, progress: false };
      case "progress":
        return { breathe: false, flow: false, comet: false, progress: true };
      case "comet":
        return { breathe: false, flow: false, comet: true, progress: false };
      case "all":
      default:
        break;
    }

    // "all" → choose the effect mix from the song's mood
    switch (this.mood) {
      case "vivid":
        return { breathe: true, flow: true, comet: true, progress: true };
      case "mellow":
        return { breathe: true, flow: false, comet: false, progress: true };
      case "monochrome":
        return { breathe: true, flow: false, comet: false, progress: true };
      case "dual_tone":
        return { breathe: true, flow: true, comet: false, progress: true };
      case "balanced":
      default:
        return { breathe: true, flow: true, comet: true, progress: true };
    }
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
        speedMult = 1.4;
        blurMult = 1.15;
        break;
      case "mellow":
        speedMult = 0.65;
        blurMult = 0.9;
        break;
      case "monochrome":
        speedMult = 0.5;
        blurMult = 0.8;
        break;
      case "dual_tone":
        speedMult = 1.1;
        blurMult = 1.05;
        break;
      case "balanced":
      default:
        break;
    }

    const fx = this.activeEffects();

    // 4. Breathe
    const breatheCycle = (timestamp * 0.0018 * speedMult) % (Math.PI * 2);
    const breatheFactor = fx.breathe ? 0.8 + 0.2 * Math.sin(breatheCycle) : 1.0;

    // 5. Burst decay (track-change flare)
    this.burst = Math.max(0, this.burst - delta * 0.0012);
    const burstBoost = 1 + this.burst * 0.6;
    const activeIntensity = Math.min(1, this.currentIntensity * breatheFactor * burstBoost);

    // 6. Flow rotation
    if (fx.flow) {
      this.rotationAngle = (this.rotationAngle + delta * 0.0006 * speedMult) % (Math.PI * 2);
    }

    // 7. Geometry: path hugs Spotify's edge (outside), or the screen edge (inside) when maximized
    const T = this.thickness * (this.isMaximized ? 1.3 : 1.0);
    // const core = 2 * T; // crisp core line width
    // const T = this.thickness * (this.isMaximized ? 1.0 : 1.0);
    const core = 1.2 * T;
    // const m = this.isMaximized ? 0 : this.margin;
    let m = 0;
    if (!this.isMaximized) {
      m = this.margin;
      if (this.lastRect && this.lastRect.width > 0) {
        const derived = (width - this.lastRect.width / dpr) / 2;
        if (derived > 0 && derived < 100) m = derived;
      }
    }
    const off = this.isMaximized ? core / 2 : -core / 2;
    const x = m + off;
    const y = m + off;
    const w = Math.max(0, width - (m + off) * 2);
    const h = Math.max(0, height - (m + off) * 2);
    const r = this.isMaximized ? 0 : this.cornerRadius + core / 2;
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

    // Stroke gradient: true conic sweep around the border (falls back to linear)
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
    ctx.lineCap = "round";
    ctx.lineJoin = "round";

    // 8. Glow layers (outer → inner). Widths/blur scale with thickness.
    const widthBoost = 1 + this.burst * 0.3;
    

const layers = [
    {
        lw: 2.0 * T * widthBoost,
        blur: 3.0 * T * blurMult,
        alpha: 0.35,
        color: cPrimary
    },
    {
        lw: 1.5 * T * widthBoost,
        blur: 2.0 * T * blurMult,
        alpha: 0.55,
        color: cSecondary
    },
    {
        lw: core,
        blur: 1.0 * T,
        alpha: 1.0,
        color: cAccent
    },
];

    for (const layer of layers) {
      ctx.save();
      ctx.globalAlpha = layer.alpha * activeIntensity;
      ctx.lineWidth = layer.lw;
      ctx.shadowColor = this.rgbString(layer.color, 0.95);
      ctx.shadowBlur = layer.blur * breatheFactor * dpr; // shadowBlur ignores ctx.scale
      this.roundedRectPath(ctx, x, y, w, h, r);
      ctx.stroke();
      ctx.restore();
    }

    // 9. Comet: bright head with a fading tail orbiting the border
    if (fx.comet) {
      const cometLen = perimeter * 0.14;
      const steps = 6;
      const seg = cometLen / steps;
      const headPos = ((timestamp * 0.00007 * speedMult) % 1) * perimeter;

      ctx.save();
      ctx.lineCap = "butt";
      ctx.lineWidth = core * 0.8;
      ctx.shadowBlur = 2.5 * T * dpr;
      ctx.shadowColor = this.rgbString(cAccent, 0.9);
      for (let k = 0; k < steps; k++) {
        const segStart = headPos - cometLen + k * seg;
        const wrapped = ((segStart % perimeter) + perimeter) % perimeter;
        const a = (k + 1) / steps;
        ctx.globalAlpha = a * a * activeIntensity;
        ctx.strokeStyle = this.rgbString(a > 0.8 ? { r: 255, g: 255, b: 255 } : cAccent);
        ctx.setLineDash([seg + 0.5, perimeter - seg]);
        ctx.lineDashOffset = -wrapped;
        this.roundedRectPath(ctx, x, y, w, h, r);
        ctx.stroke();
      }
      ctx.restore();
    }

    // 10. Progress sweep + glowing head
    if (fx.progress && this.status === "playing" && this.timelineEndMs > 0) {
      const extrapolatedMs = this.timelinePositionMs + (performance.now() - this.lastTimelineTimestamp);
      const progress = Math.max(0, Math.min(1, extrapolatedMs / this.timelineEndMs));
      const arcLength = perimeter * progress;

      if (arcLength > 1) {
        ctx.save();
        ctx.setLineDash([arcLength, perimeter]);
        ctx.lineDashOffset = 0;
        ctx.lineWidth = 3 * T;
        ctx.strokeStyle = this.rgbString(cAccent, activeIntensity);
        ctx.shadowColor = "#ffffff";
        ctx.shadowBlur = 6 * dpr;
        this.roundedRectPath(ctx, x, y, w, h, r);
        ctx.stroke();
        ctx.restore();

        // Glowing dot at the tip of the progress arc
        const head = this.pointOnPath(arcLength, x, y, w, h, r);
        ctx.save();
        ctx.globalAlpha = Math.min(1, activeIntensity);
        ctx.fillStyle = "#ffffff";
        ctx.shadowColor = this.rgbString(cAccent, 1);
        ctx.shadowBlur = 10 * T * dpr;
        ctx.beginPath();
        ctx.arc(head.x, head.y, 2.6 * T, 0, Math.PI * 2);
        ctx.fill();
        ctx.restore();
      }
    }

    ctx.restore();
  }
}