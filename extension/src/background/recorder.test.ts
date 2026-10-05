import { describe, expect, it, vi } from 'vitest';

import type { OffscreenMessage, StateReply } from '../shared/messages';
import type { RecordingState } from '../shared/recording';
import { Recorder, RecorderPlatform } from './recorder';

function fake(initial: RecordingState = { phase: 'idle' }) {
  let stored = initial;
  let offscreen = false;
  let tab: number | null = null;
  let settings = { tabAudio: true, microphone: false, highlights: true, keystrokes: false };
  const sent: OffscreenMessage[] = [];
  let reply: StateReply | undefined;
  const platform: RecorderPlatform = {
    getStreamId: vi.fn().mockResolvedValue('stream-1'),
    ensureOffscreen: vi.fn(async () => {
      offscreen = true;
    }),
    closeOffscreen: vi.fn(async () => {
      offscreen = false;
    }),
    hasOffscreen: vi.fn(async () => offscreen),
    sendToOffscreen: vi.fn(async (message) => {
      sent.push(message);
      return reply;
    }),
    startOverlay: vi.fn().mockResolvedValue(undefined),
    stopOverlay: vi.fn().mockResolvedValue(undefined),
    loadSettings: vi.fn(async () => settings),
    loadTab: vi.fn(async () => tab),
    saveTab: vi.fn(async (value) => {
      tab = value;
    }),
    loadState: vi.fn(async () => stored),
    saveState: vi.fn(async (state) => {
      stored = state;
    }),
  };
  return {
    platform,
    sent,
    setReply: (value: StateReply | undefined) => (reply = value),
    stored: () => stored,
    tab: () => tab,
    setSettings: (value: typeof settings) => (settings = value),
    recorder: new Recorder(platform, { origin: 'https://app.test', version: '0.3.0' }),
  };
}

describe('Recorder.start', () => {
  it('captures the tab, opens the offscreen document and tells it where to upload', async () => {
    const f = fake();
    f.setReply({ state: { phase: 'recording', startedAt: 1, maxDurationMs: 600000 } });
    const state = await f.recorder.start(42);

    expect(f.platform.getStreamId).toHaveBeenCalledWith(42);
    expect(f.platform.ensureOffscreen).toHaveBeenCalled();
    expect(f.sent).toEqual([
      {
        target: 'offscreen',
        type: 'offscreen-start',
        streamId: 'stream-1',
        origin: 'https://app.test',
        version: '0.3.0',
        tabAudio: true,
        microphone: false,
      },
    ]);
    expect(state.phase).toBe('recording');
  });

  it('does not start a second recording while one is running', async () => {
    const f = fake();
    await f.platform.ensureOffscreen();
    f.setReply({ state: { phase: 'recording', startedAt: 1, maxDurationMs: null } });
    const state = await f.recorder.start(1);
    expect(state.phase).toBe('recording');
    expect(f.platform.getStreamId).not.toHaveBeenCalled();
  });

  it('reports a tab that cannot be captured and cleans up', async () => {
    const f = fake();
    (f.platform.getStreamId as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error('Cannot capture a tab with an active stream.'),
    );
    const state = await f.recorder.start(5);
    expect(state).toMatchObject({ phase: 'error', code: 'capture-failed' });
    expect((state as { message: string }).message).toContain('active stream');
    expect(f.stored()).toEqual(state);
    expect(f.platform.closeOffscreen).toHaveBeenCalled();
  });
});

describe('Recorder state', () => {
  it('remembers the last state and frees the offscreen document when a recording ends', async () => {
    const f = fake();
    await f.platform.ensureOffscreen();
    const done: RecordingState = { phase: 'done', recordingId: 'r1', url: 'https://app.test/s/x' };
    await f.recorder.onState(done);
    expect(f.stored()).toEqual(done);
    expect(f.platform.closeOffscreen).toHaveBeenCalled();
    expect(await f.recorder.status()).toEqual(done);
  });

  it('keeps the document while recording', async () => {
    const f = fake();
    await f.recorder.onState({ phase: 'recording', startedAt: 1, maxDurationMs: null });
    expect(f.platform.closeOffscreen).not.toHaveBeenCalled();
  });

  it('relays Stop and falls back to the stored state when nothing answers', async () => {
    const f = fake({ phase: 'idle' });
    f.setReply({ state: { phase: 'uploading' } });
    expect(await f.recorder.stop()).toEqual({ phase: 'uploading' });
    expect(f.sent.at(-1)).toEqual({ target: 'offscreen', type: 'offscreen-stop' });
    f.setReply(undefined);
    expect(await f.recorder.stop()).toEqual({ phase: 'idle' });
  });

  it('dismisses a finished recording but not a running one', async () => {
    const done = fake({ phase: 'done', recordingId: 'r', url: 'u' });
    await done.recorder.dismiss();
    expect(done.stored()).toEqual({ phase: 'idle' });
    const running = fake({ phase: 'recording', startedAt: 1, maxDurationMs: null });
    await running.recorder.dismiss();
    expect(running.stored().phase).toBe('recording');
  });
});

describe('Recorder overlay', () => {
  it('puts the click and key overlay into the recorded tab once recording starts', async () => {
    const f = fake();
    f.setSettings({ tabAudio: true, microphone: false, highlights: true, keystrokes: true });
    f.setReply({ state: { phase: 'starting' } });
    await f.recorder.start(9);
    expect(f.tab()).toBe(9);
    expect(f.platform.startOverlay).not.toHaveBeenCalled(); // not before the stream is recording

    await f.recorder.onState({ phase: 'recording', startedAt: 1, maxDurationMs: null });
    expect(f.platform.startOverlay).toHaveBeenCalledWith(9, {
      tabAudio: true,
      microphone: false,
      highlights: true,
      keystrokes: true,
    });
  });

  it('takes the overlay out when the recording ends, and forgets the tab', async () => {
    const f = fake();
    await f.platform.saveTab(9);
    await f.recorder.onState({ phase: 'uploading' });
    expect(f.platform.stopOverlay).toHaveBeenCalledWith(9);
    expect(f.tab()).toBe(9); // still needed until the end
    await f.recorder.onState({ phase: 'done', recordingId: 'r', url: 'u' });
    expect(f.tab()).toBeNull();
  });

  it('records a tab that cannot take the script (a browser page) without the overlay', async () => {
    const f = fake();
    await f.platform.saveTab(3);
    (f.platform.startOverlay as ReturnType<typeof vi.fn>).mockRejectedValue(
      new Error('Cannot access a chrome:// URL'),
    );
    await f.recorder.onState({ phase: 'recording', startedAt: 1, maxDurationMs: null });
    expect(f.stored().phase).toBe('recording');
  });

  it('forgets the tab when the capture could not start', async () => {
    const f = fake();
    (f.platform.getStreamId as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('no'));
    await f.recorder.start(4);
    expect(f.tab()).toBeNull();
  });
});
