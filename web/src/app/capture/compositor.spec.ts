import {
  Canvas2D,
  Compositor,
  CompositorCanvas,
  FrameSource,
  bubbleRect,
  coverSquare,
} from './compositor';

describe('bubbleRect', () => {
  it('centres the bubble where asked, sized by frame height', () => {
    expect(bubbleRect({ shape: 'circle', cx: 0.5, cy: 0.5, size: 0.25 }, 1000, 800)).toEqual({
      x: 400,
      y: 300,
      size: 200,
    });
  });

  it('keeps the bubble inside the frame', () => {
    const corner = bubbleRect({ shape: 'circle', cx: 1.4, cy: -0.3, size: 0.25 }, 1000, 800);
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
    layout: { shape, cx: 0.5, cy: 0.5, size: 0.2 },
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
      'fill',
      'draw:screen',
      'save',
      'begin',
      'arc',
      'clip',
      'draw:camera',
      'restore',
    ]);
  });

  it('clips to a rounded square for the rounded shape', () => {
    const { calls } = setup(undefined, 'rounded');
    expect(calls).toContain('roundRect');
    expect(calls).not.toContain('arc');
  });

  it('skips sources that are not ready yet', () => {
    const { calls } = setup({ screen: true, camera: false });
    expect(calls).toEqual(['fill', 'draw:screen']);
  });

  it('follows a moved bubble on the next frame', () => {
    const { compositor } = setup();
    compositor.setBubble({ cx: 0.1 });
    expect(compositor.bubble.cx).toBe(0.1);
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
