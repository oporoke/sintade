import { DEFAULT_QUALITY, allowedResolutions, bitsPerSecond, clampQuality } from './quality';

describe('quality presets', () => {
  it('offers only the resolutions a plan allows', () => {
    expect(allowedResolutions(1080)).toEqual([720, 1080]);
    expect(allowedResolutions(2160)).toEqual([720, 1080, 2160]);
    expect(allowedResolutions(480)).toEqual([720]);
  });

  it('a free user cannot pick 4K: the request drops to 1080p', () => {
    expect(clampQuality({ height: 2160, fps: 60 }, 1080)).toEqual({ height: 1080, fps: 60 });
    expect(clampQuality({ height: 2160, fps: 30 }, 2160)).toEqual({ height: 2160, fps: 30 });
    expect(clampQuality({ height: 720, fps: 30 }, 1080)).toEqual({ height: 720, fps: 30 });
  });

  it('gives taller and faster presets more bits', () => {
    expect(bitsPerSecond(DEFAULT_QUALITY)).toBe(5_000_000);
    expect(bitsPerSecond({ height: 1080, fps: 60 })).toBe(8_000_000);
    expect(bitsPerSecond({ height: 720, fps: 30 })).toBeLessThan(bitsPerSecond(DEFAULT_QUALITY));
    expect(bitsPerSecond({ height: 2160, fps: 30 })).toBeGreaterThan(
      bitsPerSecond({ height: 1080, fps: 60 }),
    );
  });
});
