/** "6 min 20 s", "45 s", "1 h 2 min" (seconds dropped past an hour), localised. */
export function formatDuration(ms: number): string {
  const totalSeconds = Math.max(0, Math.round(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  if (hours > 0) {
    return $localize`:duration hours and minutes:${hours}:hours: h ${minutes}:minutes: min`;
  }
  if (minutes > 0) {
    return $localize`:duration minutes and seconds:${minutes}:minutes: min ${seconds}:seconds: s`;
  }
  return $localize`:duration seconds:${seconds}:seconds: s`;
}

/** "14:02" for today, otherwise "3 Oct, 14:02", in the user's locale. */
export function formatStart(epochMs: number, now: Date = new Date()): string {
  const start = new Date(epochMs);
  const time = new Intl.DateTimeFormat(undefined, { hour: '2-digit', minute: '2-digit' }).format(
    start,
  );
  if (start.toDateString() === now.toDateString()) {
    return time;
  }
  const day = new Intl.DateTimeFormat(undefined, { day: 'numeric', month: 'short' }).format(start);
  return `${day}, ${time}`;
}

/** The recording timer: "0:05", "12:03", "1:02:03". Seconds are floored, like a stopwatch. */
export function formatClock(ms: number): string {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = String(totalSeconds % 60).padStart(2, '0');
  return hours > 0
    ? `${hours}:${String(minutes).padStart(2, '0')}:${seconds}`
    : `${minutes}:${seconds}`;
}
