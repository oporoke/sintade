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

/** A crop of the shared screen, as fractions of its width and height. */
export interface Region {
  x: number;
  y: number;
  w: number;
  h: number;
}

export const MIN_REGION = 0.05;

/** Keeps a region inside the frame and at least `MIN_REGION` wide and tall. */
export function clampRegion(region: Region): Region {
  const unit = (n: number) => Math.min(Math.max(Number.isFinite(n) ? n : 0, 0), 1);
  const w = Math.min(Math.max(unit(region.w), MIN_REGION), 1);
  const h = Math.min(Math.max(unit(region.h), MIN_REGION), 1);
  return {
    x: Math.min(unit(region.x), 1 - w),
    y: Math.min(unit(region.y), 1 - h),
    w,
    h,
  };
}

/** The region in source pixels, rounded to even sizes (H.264 4:2:0 needs them). */
export function regionPixels(region: Region, width: number, height: number) {
  const even = (n: number) => Math.max(2, Math.round(n / 2) * 2);
  return {
    sx: region.x * width,
    sy: region.y * height,
    sw: region.w * width,
    sh: region.h * height,
    outWidth: even(region.w * width),
    outHeight: even(region.h * height),
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
  getContext(kind: '2d', options?: CanvasRenderingContext2DSettings): Canvas2D | null;
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
  /** The webcam, or null when only the screen is cropped. */
  camera: MediaStream | null;
  /** Crop to this part of the screen; the output frame is the region, fixed for the take. */
  region?: Region | null;
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
  private readonly camera: FrameSource | null;
  private readonly region: Region | null;
  private readonly stopTimer: () => void;
  private layout: BubbleLayout;
  private stopped = false;
  readonly track: MediaStreamTrack;

  constructor(options: CompositorOptions) {
    const fps = options.fps ?? DEFAULT_COMPOSITOR_FPS;
    const [screenTrack] = options.screen.getVideoTracks();
    const settings = screenTrack?.getSettings() ?? {};
    const sourceWidth = Math.max(MIN_SIDE, settings.width ?? 1280);
    const sourceHeight = Math.max(MIN_SIDE, settings.height ?? 720);
    this.region = options.region ? clampRegion(options.region) : null;
    const { outWidth, outHeight } = this.region
      ? regionPixels(this.region, sourceWidth, sourceHeight)
      : { outWidth: sourceWidth, outHeight: sourceHeight };
    // Even sizes: H.264 4:2:0 cannot encode odd ones.
    const width = Math.max(MIN_SIDE, outWidth - (outWidth % 2));
    const height = Math.max(MIN_SIDE, outHeight - (outHeight % 2));
    this.canvas = (options.createCanvas ?? browserCanvas)(width, height);
    // Opaque and unsynchronised: the frame is always fully painted, so the browser can skip alpha
    // blending and does not have to wait for the page's own compositing (Day 78 profile).
    const ctx = this.canvas.getContext('2d', { alpha: false, desynchronized: true });
    if (!ctx) {
      throw new Error('2d canvas is not available');
    }
    this.ctx = ctx;
    this.layout = clampLayout(options.layout ?? DEFAULT_BUBBLE);
    const createSource = options.createSource ?? browserSource;
    this.screen = createSource(options.screen);
    this.camera = options.camera ? createSource(options.camera) : null;
    const [track] = this.canvas.captureStream(fps).getVideoTracks();
    if (!track) {
      throw new Error('the canvas produced no video track');
    }
    this.track = track;
    this.draw();
    this.stopTimer = (options.schedule ?? browserSchedule)(() => this.draw(), 1000 / fps);
  }

  get hasCamera(): boolean {
    return this.camera !== null;
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
    const camera = this.camera;
    // Painting over the whole frame needs no clear first; only a not-yet-ready source does.
    const coversFrame = this.layout.cameraOnly ? camera?.ready : this.screen.ready;
    if (!coversFrame) {
      ctx.fillStyle = '#000';
      ctx.fillRect(0, 0, width, height);
    }
    if (this.layout.cameraOnly && camera) {
      if (camera.ready) {
        const { w, h } = { w: camera.width, h: camera.height };
        // Cover: the largest centred crop with the frame's aspect, so nothing is stretched.
        const scale = Math.max(width / w, height / h);
        const sw = width / scale;
        const sh = height / scale;
        ctx.drawImage(camera.image, (w - sw) / 2, (h - sh) / 2, sw, sh, 0, 0, width, height);
      }
      return;
    }
    if (this.screen.ready) {
      if (this.region) {
        const { sx, sy, sw, sh } = regionPixels(this.region, this.screen.width, this.screen.height);
        ctx.drawImage(this.screen.image, sx, sy, sw, sh, 0, 0, width, height);
      } else {
        ctx.drawImage(this.screen.image, 0, 0, width, height);
      }
    }
    if (!camera?.ready) {
      return;
    }
    const { x, y, size } = bubbleRect(this.layout, width, height);
    const { sx, sy, side } = coverSquare(camera.width, camera.height);
    ctx.save();
    ctx.beginPath();
    if (this.layout.shape === 'circle') {
      ctx.arc(x + size / 2, y + size / 2, size / 2, 0, Math.PI * 2);
    } else {
      ctx.roundRect(x, y, size, size, size * 0.18);
    }
    ctx.clip();
    ctx.drawImage(camera.image, sx, sy, side, side, x, y, size, size);
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
    this.camera?.stop();
  }
}
