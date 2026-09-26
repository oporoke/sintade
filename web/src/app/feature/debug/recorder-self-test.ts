import {
  ChunkRecorder,
  DEFAULT_TIMESLICE_MS,
  DEFAULT_VIDEO_BITS_PER_SECOND,
  selectMimeType,
  startAudio,
} from '../../capture';

export interface RecorderSelfTestResult {
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
 * Day 23 Check, "concatenated chunks play as a valid file", without devices: an animated canvas
 * plus a tone are recorded by the real `ChunkRecorder` in 2 s slices; the chunks are
 * concatenated in index order (as the worker will, §10 Process step 2) and the result is played
 * to the end in a `<video>`.
 */
export async function runRecorderSelfTest(durationMs = 6000): Promise<RecorderSelfTestResult> {
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
    await delay(durationMs);
    const summary = await recorder.stop();
    const file = new Blob(chunks, { type: recorder.mimeType ?? mimeType });
    return {
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

function delay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}
