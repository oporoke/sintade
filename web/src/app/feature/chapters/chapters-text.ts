import { components } from '../../api/schema';

export type Chapter = components['schemas']['ChapterDto'];

/** `m:ss` (or `h:mm:ss` from an hour) for a time in milliseconds. */
export function formatClock(ms: number): string {
  const total = Math.floor(ms / 1000);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor(total / 60) % 60;
  const seconds = total % 60;
  const ss = String(seconds).padStart(2, '0');
  return hours > 0 ? `${hours}:${String(minutes).padStart(2, '0')}:${ss}` : `${minutes}:${ss}`;
}

/** Milliseconds for `m:ss` / `h:mm:ss` / plain seconds, or `null` if it isn't a time. */
export function parseClock(text: string): number | null {
  const parts = text.split(':');
  if (parts.length > 3 || parts.some((part) => !/^\d{1,3}$/.test(part))) {
    return null;
  }
  const numbers = parts.map(Number);
  if (numbers.length > 1 && numbers.slice(1).some((value) => value > 59)) {
    return null;
  }
  return numbers.reduce((total, value) => total * 60 + value, 0) * 1000;
}

export type ParsedChapters = { ok: true; chapters: Chapter[] } | { ok: false; line: number };

/**
 * The editor's text, one chapter per line as `m:ss Title` (blank lines ignored). On a bad line
 * returns its 1-based number.
 */
export function parseChapterLines(text: string): ParsedChapters {
  const chapters: Chapter[] = [];
  const lines = text.split(/\r?\n/);
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i].trim();
    if (line === '') {
      continue;
    }
    const match = /^(\S+)\s+(.+)$/.exec(line);
    const start = match ? parseClock(match[1]) : null;
    if (!match || start === null) {
      return { ok: false, line: i + 1 };
    }
    chapters.push({ start_ms: start, title: match[2].trim() });
  }
  return { ok: true, chapters };
}

export function formatChapterLines(chapters: readonly Chapter[]): string {
  return chapters.map((chapter) => `${formatClock(chapter.start_ms)} ${chapter.title}`).join('\n');
}

/** The chapter playing at `timeS`: the last one that has started; `null` before the first. */
export function chapterAt(chapters: readonly Chapter[], timeS: number): Chapter | null {
  let found: Chapter | null = null;
  for (const chapter of chapters) {
    if (chapter.start_ms / 1000 <= timeS) {
      found = chapter;
    } else {
      break;
    }
  }
  return found;
}
