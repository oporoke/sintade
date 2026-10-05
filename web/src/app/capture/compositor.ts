export type BubbleShape = 'circle' | 'rounded';

/** Where the webcam bubble sits. Fractions of the output frame, so it survives any resolution. */
export interface BubbleLayout {
  shape: BubbleShape;
  /** Bubble centre, 0..1 of frame width / height. */
  cx: number;
  cy: number;
  /** Bubble diameter (side) as a fraction of the frame height. */
  size: number;
  /** Camera-only mode: the camera fills the frame and the screen is not drawn. */
  cameraOnly: boolean;
}

export const MIN_BUBBLE_SIZE = 0.1;
export const MAX_BUBBLE_SIZE = 0.6;

/** Keeps a layout's numbers in range (centre inside the frame, size within the allowed span). */
export function clampLayout(layout: BubbleLayout): BubbleLayout {
  const unit = (n: number) => Math.min(Math.max(Number.isFinite(n) ? n : 0.5, 0), 1);
  return {
    ...layout,
    cx: unit(layout.cx),
    cy: unit(layout.cy),
    size: Math.min(Math.max(layout.size, MIN_BUBBLE_SIZE), MAX_BUBBLE_SIZE),
  };
}

export const DEFAULT_BUBBLE: BubbleLayout = {
  shape: 'circle',
  cx: 0.88,
  cy: 0.82,
  size: 0.24,
  cameraOnly: false,
};

export interface BubbleRect {
  x: number;
  y: number;
  size: number;
}

/** The bubble's square in output pixels, kept fully inside the frame. */
export function bubbleRect(layout: BubbleLayout, width: number, height: number): BubbleRect {
  const size = Math.max(1, Math.min(layout.size * height, width, height));
  const x = Math.min(Math.max(layout.cx * width - size / 2, 0), width - size);
  const y = Math.min(Math.max(layout.cy * height - size / 2, 0), height - size);
  return { x, y, size };
}

/** The largest centred square of a `w`×`h` source, so the camera fills the bubble undistorted. */
export function coverSquare(w: number, h: number): { sx: number; sy: number; side: number } {
  const side = Math.min(w, h);
  return { sx: (w - side) / 2, sy: (h - side) / 2, side };
}

/** The slice of the canvas the 2D calls need; a subset so tests can fake it. */
export type Canvas2D = Pick<
  CanvasRenderingContext2D,
  'drawImage' | 'save' | 'restore' | 'beginPath' | 'arc' | 'roundRect' | 'clip' | 'fillRect'
> & { fillStyle: unknown };

export interface CompositorCanvas {
  width: number;
  height: number;
  getContext(kind: '2d'): Canvas2D | null;
  captureStream(fps: number): MediaStream;
}

export interface FrameSource {
  readonly width: number;
  readonly height: number;
  readonly ready: boolean;
  readonly image: CanvasImageSource;
  stop(): void;
}

export interface CompositorOptions {
  screen: MediaStream;
  camera: MediaStream;
  fps?: number;
  layout?: BubbleLayout;
  /** Test seams; default to the browser (OffscreenCanvas where available, else a canvas element). */
  createCanvas?: (width: number, height: number) => CompositorCanvas;
  createSource?: (stream: MediaStream) => FrameSource;
  schedule?: (tick: () => void, intervalMs: number) => () => void;
}

export const DEFAULT_COMPOSITOR_FPS = 30;
const MIN_SIDE = 2;

function browserCanvas(width: number, height: number): CompositorCanvas {
  // `captureStream` exists only on HTMLCanvasElement (OffscreenCanvas has none), so the element
  // is the recorded surface; drawing is the same either way.
  const canvas = document.createElement('canvas');
  canvas.width = width;
  canvas.height = height;
  return canvas as unknown as CompositorCanvas;
}

function browserSource(stream: MediaStream): FrameSource {
  const video = document.createElement('video');
  video.muted = true;
  video.playsInline = true;
  video.srcObject = stream;
  void video.play().catch(() => undefined);
  return {
    get width() {
      return video.videoWidth;
    },
    get height() {
      return video.videoHeight;
    },
    get ready() {
      return video.readyState >= 2 && video.videoWidth > 0;
    },
    image: video,
    stop() {
      video.pause();
      video.srcObject = null;
    },
  };
}

function browserSchedule(tick: () => void, intervalMs: number): () => void {
  // A timer, not requestAnimationFrame: the recorder tab is usually in the background while the
  // user records another window, and rAF stops there.
  const id = setInterval(tick, intervalMs);
  return () => clearInterval(id);
}

/**
 * Draws the shared screen with the webcam as a bubble on a canvas and exposes the canvas as one
 * video track (§10 Record). Only built when a camera is on; with no camera the screen track goes
 * straight to the recorder. Framework-free (CLAUDE.md rule 9).
 */
export class Compositor {
  private readonly canvas: CompositorCanvas;
  private readonly ctx: Canvas2D;
  private readonly screen: FrameSource;
  private readonly camera: FrameSource;
  private readonly stopTimer: () => void;
  private layout: BubbleLayout;
  private stopped = false;
  readonly track: MediaStreamTrack;

  constructor(options: CompositorOptions) {
    const fps = options.fps ?? DEFAULT_COMPOSITOR_FPS;
    const [screenTrack] = options.screen.getVideoTracks();
    const settings = screenTrack?.getSettings() ?? {};
    const width = Math.max(MIN_SIDE, settings.width ?? 1280);
    const height = Math.max(MIN_SIDE, settings.height ?? 720);
    this.canvas = (options.createCanvas ?? browserCanvas)(width, height);
    const ctx = this.canvas.getContext('2d');
    if (!ctx) {
      throw new Error('2d canvas is not available');
    }
    this.ctx = ctx;
    this.layout = clampLayout(options.layout ?? DEFAULT_BUBBLE);
    const createSource = options.createSource ?? browserSource;
    this.screen = createSource(options.screen);
    this.camera = createSource(options.camera);
    const [track] = this.canvas.captureStream(fps).getVideoTracks();
    if (!track) {
      throw new Error('the canvas produced no video track');
    }
    this.track = track;
    this.draw();
    this.stopTimer = (options.schedule ?? browserSchedule)(() => this.draw(), 1000 / fps);
  }

  get bubble(): BubbleLayout {
    return this.layout;
  }

  setBubble(layout: Partial<BubbleLayout>): void {
    this.layout = clampLayout({ ...this.layout, ...layout });
  }

  /** Paints one frame: the screen, then the camera bubble on top. Safe before sources are ready. */
  draw(): void {
    if (this.stopped) {
      return;
    }
    const { ctx, canvas } = this;
    const { width, height } = canvas;
    ctx.fillStyle = '#000';
    ctx.fillRect(0, 0, width, height);
    if (this.layout.cameraOnly) {
      if (this.camera.ready) {
        const { w, h } = { w: this.camera.width, h: this.camera.height };
        // Cover: the largest centred crop with the frame's aspect, so nothing is stretched.
        const scale = Math.max(width / w, height / h);
        const sw = width / scale;
        const sh = height / scale;
        ctx.drawImage(this.camera.image, (w - sw) / 2, (h - sh) / 2, sw, sh, 0, 0, width, height);
      }
      return;
    }
    if (this.screen.ready) {
      ctx.drawImage(this.screen.image, 0, 0, width, height);
    }
    if (!this.camera.ready) {
      return;
    }
    const { x, y, size } = bubbleRect(this.layout, width, height);
    const { sx, sy, side } = coverSquare(this.camera.width, this.camera.height);
    ctx.save();
    ctx.beginPath();
    if (this.layout.shape === 'circle') {
      ctx.arc(x + size / 2, y + size / 2, size / 2, 0, Math.PI * 2);
    } else {
      ctx.roundRect(x, y, size, size, size * 0.18);
    }
    ctx.clip();
    ctx.drawImage(this.camera.image, sx, sy, side, side, x, y, size, size);
    ctx.restore();
  }

  stop(): void {
    if (this.stopped) {
      return;
    }
    this.stopped = true;
    this.stopTimer();
    this.track.stop();
    this.screen.stop();
    this.camera.stop();
  }
}
