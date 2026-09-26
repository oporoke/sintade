import { firstValueFrom } from 'rxjs';

import {
  ChunkRecorder,
  RecordingSummary,
  DEFAULT_TIMESLICE_MS,
  DEFAULT_VIDEO_BITS_PER_SECOND,
  selectMimeType,
  startAudio,
} from '../../capture';

/**
 * - `continuous`: 6 s straight (Day 23).
 * - `pause`: 2 s, pause 2 s (the canvas keeps animating), 2 s more: the clip must be ~4 s.
 * - `track-ended`: the video track ends after 3 s, as with the browser's "Stop sharing";
 *   the recorder must stop by itself.
 */
export type RecorderScenario = 'continuous' | 'pause' | 'track-ended';

export interface RecorderSelfTestResult {
  scenario: RecorderScenario;
  /** How the take ended: our `stop()` call, or the recorder stopping itself. */
  stoppedBy: 'stop()' | 'track ended';
  mimeType: string;
  chunkSizes: number[];
  recordedMs: number;
  /** The concatenated file, for playback checks and download. */
  file: Blob;
  playback: PlaybackCheck;
}

export interface PlaybackCheck {
  videoWidth: number;
  videoHeight: number;
  /** Media time reached when playback ended, i.e. how much of the file actually played. */
  playedSeconds: number;
  ended: boolean;
}

const WIDTH = 640;
const HEIGHT = 360;
const PLAYBACK_RATE = 4;

/**
 * Day 23/24 Checks ("concatenated chunks play as a valid file", "clip with a pause plays
 * correctly"), without devices: an animated canvas
 * plus a tone are recorded by the real `ChunkRecorder` in 2 s slices; the chunks are
 * concatenated in index order (as the worker will, §10 Process step 2) and the result is played
 * to the end in a `<video>`.
 */
export async function runRecorderSelfTest(
  scenario: RecorderScenario = 'continuous',
): Promise<RecorderSelfTestResult> {
  if (typeof MediaRecorder === 'undefined') {
    throw new Error('MediaRecorder is not available in this browser');
  }
  const mimeType = selectMimeType();
  if (!mimeType) {
    throw new Error('no preferred recording format is supported');
  }

  const { stream, stop: stopSource } = await syntheticSource();
  const recorder = new ChunkRecorder();
  const chunks: Blob[] = [];
  recorder.chunks$.subscribe((chunk) => (chunks[chunk.index] = chunk.blob));
  try {
    recorder.start(stream, {
      mimeType,
      timesliceMs: DEFAULT_TIMESLICE_MS,
      bitsPerSecond: DEFAULT_VIDEO_BITS_PER_SECOND,
    });
    let stoppedBy: RecorderSelfTestResult['stoppedBy'] = 'stop()';
    let summary: RecordingSummary;
    if (scenario === 'pause') {
      await delay(2000);
      recorder.pause();
      await delay(2000);
      recorder.resume();
      await delay(2000);
      summary = await recorder.stop();
    } else if (scenario === 'track-ended') {
      const stopped = firstValueFrom(recorder.stopped$);
      await delay(3000);
      stream.getVideoTracks().forEach((track) => stopFromBrowser(track));
      summary = await withTimeout(stopped, 5000, 'the recorder did not stop when its track ended');
      stoppedBy = 'track ended';
    } else {
      await delay(6000);
      summary = await recorder.stop();
    }
    const file = new Blob(chunks, { type: recorder.mimeType ?? mimeType });
    return {
      scenario,
      stoppedBy,
      mimeType: file.type,
      chunkSizes: chunks.map((chunk) => chunk.size),
      recordedMs: summary.durationMs,
      file,
      playback: await playToEnd(file),
    };
  } finally {
    stopSource();
  }
}

async function syntheticSource(): Promise<{ stream: MediaStream; stop: () => void }> {
  const canvas = document.createElement('canvas');
  canvas.width = WIDTH;
  canvas.height = HEIGHT;
  const context = canvas.getContext('2d');
  if (!context) {
    throw new Error('2D canvas unavailable');
  }
  let frame = 0;
  let running = true;
  const draw = () => {
    if (!running) {
      return;
    }
    context.fillStyle = '#123';
    context.fillRect(0, 0, WIDTH, HEIGHT);
    context.fillStyle = '#4c4';
    context.fillRect((frame * 8) % WIDTH, HEIGHT / 3, 80, 80);
    context.fillStyle = '#fff';
    context.font = '24px sans-serif';
    context.fillText(`frame ${frame}`, 16, 32);
    frame += 1;
    requestAnimationFrame(draw);
  };
  draw();

  const audio = new AudioContext();
  await startAudio(audio);
  const oscillator = audio.createOscillator();
  oscillator.frequency.value = 440;
  const gain = audio.createGain();
  gain.gain.value = 0.1;
  const destination = audio.createMediaStreamDestination();
  oscillator.connect(gain).connect(destination);
  oscillator.start();

  const video = canvas.captureStream(30);
  const stream = new MediaStream([
    ...video.getVideoTracks(),
    ...destination.stream.getAudioTracks(),
  ]);
  return {
    stream,
    stop: () => {
      running = false;
      stream.getTracks().forEach((track) => track.stop());
      void audio.close();
    },
  };
}

function playToEnd(file: Blob): Promise<PlaybackCheck> {
  const url = URL.createObjectURL(file);
  const video = document.createElement('video');
  video.muted = true;
  video.playsInline = true;
  return new Promise<PlaybackCheck>((resolve, reject) => {
    const cleanup = () => {
      clearTimeout(timer);
      video.removeAttribute('src');
      video.load();
      URL.revokeObjectURL(url);
    };
    const timer = setTimeout(() => {
      const partial = snapshot(false);
      cleanup();
      reject(
        new Error(`playback did not reach the end (at ${partial.playedSeconds.toFixed(2)} s)`),
      );
    }, 20_000);
    const snapshot = (ended: boolean): PlaybackCheck => ({
      videoWidth: video.videoWidth,
      videoHeight: video.videoHeight,
      playedSeconds: video.currentTime,
      ended,
    });
    video.addEventListener('ended', () => {
      const result = snapshot(true);
      cleanup();
      resolve(result);
    });
    video.addEventListener('error', () => {
      const code = video.error?.code;
      cleanup();
      reject(new Error(`the concatenated file failed to play (MediaError ${code})`));
    });
    video.addEventListener(
      'canplay',
      () => {
        video.playbackRate = PLAYBACK_RATE;
        video.play().catch((error: unknown) => {
          cleanup();
          reject(error instanceof Error ? error : new Error(String(error)));
        });
      },
      { once: true },
    );
    video.src = url;
  });
}

/**
 * Simulates the user clicking the browser's "Stop sharing": `track.stop()` does not fire
 * `ended` (by spec only the browser ending a track does), so dispatch that event ourselves.
 * `dispatchEvent` also runs the track's `onended` handler, which is the path Firefox delivers
 * on (see `onTrackEnded`).
 */
function stopFromBrowser(track: MediaStreamTrack): void {
  track.dispatchEvent(new Event('ended'));
  track.stop();
}

function withTimeout<T>(promise: Promise<T>, ms: number, message: string): Promise<T> {
  return Promise.race([
    promise,
    new Promise<never>((_, reject) => setTimeout(() => reject(new Error(message)), ms)),
  ]);
}

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
