// Draws the extension icons (a filled circle with a white record dot) as PNGs, no dependencies.
import { deflateSync } from 'node:zlib';

function crc32(buf) {
  let c = ~0;
  for (const byte of buf) {
    c ^= byte;
    for (let k = 0; k < 8; k += 1) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type, data) {
  const body = Buffer.concat([Buffer.from(type), data]);
  const out = Buffer.alloc(8 + data.length + 4);
  out.writeUInt32BE(data.length, 0);
  body.copy(out, 4);
  out.writeUInt32BE(crc32(body), 8 + data.length);
  return out;
}

/** An RGBA PNG of `size` px: a blue disc with a white dot, anti-aliased by 4x supersampling. */
export function iconPng(size) {
  const raw = Buffer.alloc(size * (size * 4 + 1));
  const centre = size / 2;
  const sub = 4;
  for (let y = 0; y < size; y += 1) {
    raw[y * (size * 4 + 1)] = 0;
    for (let x = 0; x < size; x += 1) {
      let outer = 0;
      let inner = 0;
      for (let sy = 0; sy < sub; sy += 1) {
        for (let sx = 0; sx < sub; sx += 1) {
          const dx = x + (sx + 0.5) / sub - centre;
          const dy = y + (sy + 0.5) / sub - centre;
          const d = Math.hypot(dx, dy);
          if (d <= size * 0.47) outer += 1;
          if (d <= size * 0.2) inner += 1;
        }
      }
      const total = sub * sub;
      const alpha = outer / total;
      const white = inner / outer || 0;
      const i = y * (size * 4 + 1) + 1 + x * 4;
      raw[i] = Math.round(0x3b * (1 - white) + 255 * white);
      raw[i + 1] = Math.round(0x63 * (1 - white) + 255 * white);
      raw[i + 2] = Math.round(0xe6 * (1 - white) + 255 * white);
      raw[i + 3] = Math.round(alpha * 255);
    }
  }
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header[8] = 8;
  header[9] = 6;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk('IHDR', header),
    chunk('IDAT', deflateSync(raw)),
    chunk('IEND', Buffer.alloc(0)),
  ]);
}

export const ICON_SIZES = [16, 32, 48, 128];
