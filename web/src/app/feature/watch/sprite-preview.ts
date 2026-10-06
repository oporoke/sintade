/** One thumbnail of the scrub sprite: when it shows and where it sits on its sheet. */
export interface SpriteCue {
  startS: number;
  endS: number;
  /** The signed sheet. */
  url: string;
  x: number;
  y: number;
  width: number;
  height: number;
}

function seconds(clock: string): number | null {
  const parts = clock.trim().split(':');
  if (parts.length < 2 || parts.length > 3) {
    return null;
  }
  const values = parts.map(Number);
  if (values.some((value) => !Number.isFinite(value))) {
    return null;
  }
  return values.reduce((total, value) => total * 60 + value, 0);
}

/** Reads `sprite.vtt`: cues of `<sheet url>#xywh=x,y,w,h`. Cues it can't read are skipped. */
export function parseSpriteVtt(text: string): SpriteCue[] {
  const cues: SpriteCue[] = [];
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length - 1; i++) {
    const [from, to] = lines[i].split('-->');
    if (to === undefined) {
      continue;
    }
    const startS = seconds(from);
    const endS = seconds(to);
    const target = lines[i + 1];
    const at = target.lastIndexOf('#xywh=');
    if (startS === null || endS === null || at < 0) {
      continue;
    }
    const [x, y, width, height] = target
      .slice(at + 6)
      .split(',')
      .map(Number);
    if ([x, y, width, height].some((value) => !Number.isFinite(value))) {
      continue;
    }
    cues.push({ startS, endS, url: target.slice(0, at), x, y, width, height });
  }
  return cues;
}

/** The cue showing at `timeS`; the last one for a time past the end, `null` if there are none. */
export function cueAt(cues: readonly SpriteCue[], timeS: number): SpriteCue | null {
  if (cues.length === 0) {
    return null;
  }
  let low = 0;
  let high = cues.length - 1;
  while (low < high) {
    const mid = (low + high + 1) >> 1;
    if (cues[mid].startS <= timeS) {
      low = mid;
    } else {
      high = mid - 1;
    }
  }
  return cues[low];
}
