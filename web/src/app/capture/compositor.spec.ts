import {
  Canvas2D,
  Compositor,
  CompositorCanvas,
  FrameSource,
  bubbleRect,
  clampRegion,
  regionPixels,
  coverSquare,
} from './compositor';

describe('bubbleRect', () => {
  it('centres the bubble where asked, sized by frame height', () => {
    expect(
      bubbleRect({ shape: 'circle', cx: 0.5, cy: 0.5, size: 0.25, cameraOnly: false }, 1000, 800),
    ).toEqual({
      x: 400,
      y: 300,
      size: 200,
    });
  });

  it('keeps the bubble inside the frame', () => {
    const corner = bubbleRect(
      { shape: 'circle', cx: 1.4, cy: -0.3, size: 0.25, cameraOnly: false },
      1000,
      800,
    );
    expect(corner).toEqual({ x: 800, y: 0, size: 200 });
  });
});

describe('coverSquare', () => {
  it('crops a landscape camera to its centre square', () => {
    expect(coverSquare(640, 480)).toEqual({ sx: 80, sy: 0, side: 480 });
  });
});

function setup(ready = { screen: true, camera: true }, shape: 'circle' | 'rounded' = 'circle') {
  const calls: string[] = [];
  const ctx = {
    fillStyle: '',
    fillRect: () => calls.push('fill'),
    drawImage: (image: unknown) => calls.push(`draw:${(image as { id: string }).id}`),
    save: () => calls.push('save'),
    restore: () => calls.push('restore'),
    beginPath: () => calls.push('begin'),
    arc: () => calls.push('arc'),
    roundRect: () => calls.push('roundRect'),
    clip: () => calls.push('clip'),
  } as unknown as Canvas2D;
  const track = { stop: vi.fn() } as unknown as MediaStreamTrack;
  const canvas: CompositorCanvas = {
    width: 1920,
    height: 1080,
    getContext: () => ctx,
    captureStream: () => ({ getVideoTracks: () => [track] }) as unknown as MediaStream,
  };
  const source = (id: string, isReady: boolean): FrameSource => ({
    width: 640,
    height: 480,
    ready: isReady,
    image: { id } as unknown as CanvasImageSource,
    stop: vi.fn(),
  });
  const screen = source('screen', ready.screen);
  const camera = source('camera', ready.camera);
  const stopTimer = vi.fn();
  let tick: () => void = () => undefined;
  const compositor = new Compositor({
    screen: {
      getVideoTracks: () => [{ getSettings: () => ({ width: 1920, height: 1080 }) }],
    } as unknown as MediaStream,
    camera: {} as MediaStream,
    layout: { shape, cx: 0.5, cy: 0.5, size: 0.2, cameraOnly: false },
    createCanvas: () => canvas,
    createSource: (stream) => ('getVideoTracks' in stream ? screen : camera),
    schedule: (fn) => {
      tick = fn;
      return stopTimer;
    },
  });
  return { compositor, calls, screen, camera, track, stopTimer, tick: () => tick() };
}

describe('Compositor', () => {
  it('draws the screen, then the camera clipped to a circle', () => {
    const { calls } = setup();
    expect(calls).toEqual([
      'draw:screen',
      'save',
      'begin',
      'arc',
      'clip',
      'draw:camera',
      'restore',
    ]);
  });

  it('clears to black only while the screen is not ready to cover the frame', () => {
    const { calls } = setup({ screen: false, camera: true });
    expect(calls[0]).toBe('fill');
  });

  it('clips to a rounded square for the rounded shape', () => {
    const { calls } = setup(undefined, 'rounded');
    expect(calls).toContain('roundRect');
    expect(calls).not.toContain('arc');
  });

  it('skips sources that are not ready yet', () => {
    const { calls } = setup({ screen: true, camera: false });
    expect(calls).toEqual(['draw:screen']);
  });

  it('follows a moved bubble on the next frame', () => {
    const { compositor } = setup();
    compositor.setBubble({ cx: 0.1 });
    expect(compositor.bubble.cx).toBe(0.1);
  });

  it('clamps a moved or resized bubble into range', () => {
    const { compositor } = setup();
    compositor.setBubble({ cx: 2, cy: -1, size: 5 });
    expect(compositor.bubble).toMatchObject({ cx: 1, cy: 0, size: 0.6 });
    compositor.setBubble({ size: 0 });
    expect(compositor.bubble.size).toBe(0.1);
  });

  it('camera-only draws just the camera, covering the frame', () => {
    const { compositor, calls } = setup();
    calls.length = 0;
    compositor.setBubble({ cameraOnly: true });
    compositor.draw();
    expect(calls).toEqual(['draw:camera']);
  });

  it('stops the timer, the track and both sources once', () => {
    const { compositor, stopTimer, track, screen, camera, calls, tick } = setup();
    compositor.stop();
    compositor.stop();
    const before = calls.length;
    tick();
    expect(calls.length).toBe(before);
    expect(stopTimer).toHaveBeenCalledTimes(1);
    expect(track.stop).toHaveBeenCalledTimes(1);
    expect(screen.stop).toHaveBeenCalledTimes(1);
    expect(camera.stop).toHaveBeenCalledTimes(1);
  });
});

describe('regions', () => {
  it('clamps a region inside the frame and keeps it at least 5 % wide', () => {
    expect(clampRegion({ x: 0.9, y: -1, w: 0.5, h: 0 })).toEqual({ x: 0.5, y: 0, w: 0.5, h: 0.05 });
  });

  it('turns a region into source pixels and an even output size', () => {
    expect(regionPixels({ x: 0.5, y: 0.25, w: 0.25, h: 0.5 }, 1921, 1081)).toMatchObject({
      sx: 960.5,
      sy: 270.25,
      outWidth: 480,
      outHeight: 540,
    });
  });

  it('crops: the canvas is the region and the screen is drawn from the region only', () => {
    const draws: unknown[][] = [];
    let size: [number, number] = [0, 0];
    const ctx = {
      fillStyle: '',
      fillRect: vi.fn(),
      drawImage: (...args: unknown[]) => draws.push(args),
    } as unknown as Canvas2D;
    const canvas = {
      width: 0,
      height: 0,
      getContext: () => ctx,
      captureStream: () =>
        ({ getVideoTracks: () => [{ stop: vi.fn() }] }) as unknown as MediaStream,
    } as CompositorCanvas;
    new Compositor({
      screen: {
        getVideoTracks: () => [{ getSettings: () => ({ width: 1920, height: 1080 }) }],
      } as unknown as MediaStream,
      camera: null,
      region: { x: 0.5, y: 0, w: 0.5, h: 1 },
      createCanvas: (w, h) => {
        size = [w, h];
        canvas.width = w;
        canvas.height = h;
        return canvas;
      },
      createSource: () => ({
        width: 1920,
        height: 1080,
        ready: true,
        image: { id: 'screen' } as unknown as CanvasImageSource,
        stop: vi.fn(),
      }),
      schedule: () => () => undefined,
    });
    expect(size).toEqual([960, 1080]);
    // The right half of the screen fills the whole output frame.
    expect(draws).toEqual([[{ id: 'screen' }, 960, 0, 960, 1080, 0, 0, 960, 1080]]);
  });
});
