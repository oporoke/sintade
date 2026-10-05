import { Subject } from 'rxjs';
import { describe, expect, it, vi } from 'vitest';

import type { TakeMeta, TakeSession } from '@capture';
import { UploadHttpError } from '@capture';

import type { RecordingState } from '../shared/recording';
import { ControllerDeps, LIMIT_MARGIN_MS, RecordingController } from './recording-controller';

const TAKE = '01a0e7a3-c969-756c-93d0-000000000001';

function fakeSession() {
  const stored = new Subject<number>();
  const elapsed = new Subject<number>();
  let end!: (take: TakeMeta) => void;
  let fail!: (error: unknown) => void;
  const ended = new Promise<TakeMeta>((resolve, reject) => {
    end = resolve;
    fail = reject;
  });
  const session = {
    stored$: stored.asObservable(),
    elapsed$: () => elapsed.asObservable(),
    ended,
    stop: vi.fn(async () => {
      end({ chunkCount: 3, durationMs: 6000 } as TakeMeta);
      return ended;
    }),
  };
  return {
    session: session as unknown as TakeSession,
    stored,
    elapsed,
    end,
    fail,
    stop: session.stop,
  };
}

function setup(overrides: Partial<ControllerDeps> = {}) {
  const take = fakeSession();
  const states: RecordingState[] = [];
  const uploader = {
    enqueue: vi.fn(),
    finalize: vi.fn().mockResolvedValue({ recording_id: 'rec-1' }),
  };
  const trackStop = vi.fn();
  const stream = {
    getAudioTracks: () => [{}],
    getTracks: () => [{ stop: trackStop }],
  } as unknown as MediaStream;
  const api = {
    createRecording: vi.fn().mockResolvedValue({
      recording_id: 'rec-1',
      take_id: TAKE,
      max_duration_ms: 600_000,
    }),
    createLink: vi.fn().mockResolvedValue({ slug: 'abcdefghijkl' }),
  };
  const startTake = vi.fn().mockResolvedValue(take.session);
  const deps: ControllerDeps = {
    api,
    stream,
    store: {} as ControllerDeps['store'],
    mixer: {} as ControllerDeps['mixer'],
    startTake,
    createUploader: vi.fn().mockReturnValue(uploader),
    linkUrl: (slug) => `https://app.test/s/${slug}`,
    onState: (state) => states.push(state),
    now: () => 1000,
    mimeType: () => 'video/webm;codecs=vp9,opus',
    ...overrides,
  };
  return {
    controller: new RecordingController(deps),
    take,
    states,
    uploader,
    api,
    startTake,
    trackStop,
  };
}

describe('RecordingController', () => {
  it('records the tab under the server take, uploads each chunk, then finalizes and links', async () => {
    const t = setup();
    await t.controller.start();

    expect(t.api.createRecording).toHaveBeenCalledWith({
      mime_type: 'video/webm;codecs=vp9,opus',
      has_system_audio: true,
      has_mic: false,
      has_camera: false,
    });
    expect(t.startTake.mock.calls[0]?.[0]).toMatchObject({
      takeId: TAKE,
      serverTakeId: TAKE,
      mic: null,
      mimeType: 'video/webm;codecs=vp9,opus',
    });
    expect(t.controller.current).toEqual({
      phase: 'recording',
      startedAt: 1000,
      maxDurationMs: 600000,
    });

    [0, 1, 2].forEach((idx) => t.take.stored.next(idx));
    expect(t.uploader.enqueue.mock.calls.map((c) => c[0])).toEqual([0, 1, 2]);

    await t.controller.stop();
    await vi.waitFor(() => expect(t.controller.current.phase).toBe('done'));
    expect(t.uploader.finalize).toHaveBeenCalledWith(3, 6000);
    expect(t.api.createLink).toHaveBeenCalledWith('rec-1');
    expect(t.controller.current).toEqual({
      phase: 'done',
      recordingId: 'rec-1',
      url: 'https://app.test/s/abcdefghijkl',
    });
    expect(t.states.map((s) => s.phase)).toEqual(['starting', 'recording', 'uploading', 'done']);
    expect(t.trackStop).toHaveBeenCalled(); // the tab's capture is released
  });

  it('mixes the microphone in when it is on, and releases it at the end', async () => {
    const micStop = vi.fn();
    const mic = {
      getAudioTracks: () => [{}],
      getTracks: () => [{ stop: micStop }],
    } as unknown as MediaStream;
    const t = setup({ mic });
    await t.controller.start();
    expect(t.api.createRecording).toHaveBeenCalledWith(
      expect.objectContaining({ has_system_audio: true, has_mic: true }),
    );
    expect(t.startTake.mock.calls[0]?.[0]).toMatchObject({ mic });
    await t.controller.stop();
    await vi.waitFor(() => expect(t.controller.current.phase).toBe('done'));
    expect(micStop).toHaveBeenCalled();
  });

  it('records a tab without sound as video only, or the microphone alone as audio', async () => {
    const silent = {
      getAudioTracks: () => [],
      getTracks: () => [{ stop: vi.fn() }],
    } as unknown as MediaStream;
    const mimeTypes: boolean[] = [];
    const t = setup({
      stream: silent,
      mimeType: (hasAudio) => {
        mimeTypes.push(hasAudio);
        return 'video/webm;codecs=vp9';
      },
    });
    await t.controller.start();
    expect(mimeTypes).toEqual([false]);
    expect(t.api.createRecording).toHaveBeenCalledWith(
      expect.objectContaining({ has_system_audio: false, has_mic: false }),
    );

    const mic = { getAudioTracks: () => [{}], getTracks: () => [] } as unknown as MediaStream;
    const micOnly = setup({
      stream: silent,
      mic,
      mimeType: (a) => (mimeTypes.push(a), 'video/webm'),
    });
    await micOnly.controller.start();
    expect(mimeTypes.at(-1)).toBe(true);
  });

  it('stops by itself just inside the plan limit', async () => {
    const t = setup();
    await t.controller.start();
    t.take.elapsed.next(600_000 - LIMIT_MARGIN_MS - 1);
    expect(t.take.stop).not.toHaveBeenCalled();
    t.take.elapsed.next(600_000 - LIMIT_MARGIN_MS);
    expect(t.take.stop).toHaveBeenCalled();
  });

  it('finishes the same way when the tab closes (the take ends without Stop)', async () => {
    const t = setup();
    await t.controller.start();
    t.take.end({ chunkCount: 2, durationMs: 4000 } as TakeMeta);
    await vi.waitFor(() => expect(t.controller.current.phase).toBe('done'));
    expect(t.uploader.finalize).toHaveBeenCalledWith(2, 4000);
  });

  it.each([
    [401, 'not-signed-in'],
    [402, 'limit-reached'],
    [0, 'unreachable'],
    [500, 'unreachable'],
  ])('a %i from the server when creating the recording is %s', async (status, code) => {
    const t = setup();
    (t.api.createRecording as ReturnType<typeof vi.fn>).mockRejectedValue(
      new UploadHttpError('api', status, 'nope'),
    );
    await t.controller.start();
    expect(t.controller.current).toMatchObject({ phase: 'error', code });
    expect(t.startTake).not.toHaveBeenCalled();
    expect(t.trackStop).toHaveBeenCalled();
  });

  it('reports a capture that cannot start', async () => {
    const t = setup();
    t.startTake.mockRejectedValue(new Error('no recorder'));
    await t.controller.start();
    expect(t.controller.current).toMatchObject({ phase: 'error', code: 'capture-failed' });
  });

  it('reports an upload that fails, and nothing recorded', async () => {
    const failed = setup();
    failed.uploader.finalize.mockRejectedValue(new Error('offline'));
    await failed.controller.start();
    await failed.controller.stop();
    await vi.waitFor(() => expect(failed.controller.current.phase).toBe('error'));
    expect(failed.controller.current).toMatchObject({ code: 'upload-failed' });
    expect(failed.api.createLink).not.toHaveBeenCalled();

    const empty = setup();
    await empty.controller.start();
    empty.take.end({ chunkCount: 0, durationMs: 0 } as TakeMeta);
    await vi.waitFor(() => expect(empty.controller.current.phase).toBe('error'));
  });

  it('says so when the link cannot be created, since the upload itself worked', async () => {
    const t = setup();
    (t.api.createLink as ReturnType<typeof vi.fn>).mockRejectedValue(new Error('403'));
    await t.controller.start();
    await t.controller.stop();
    await vi.waitFor(() => expect(t.controller.current.phase).toBe('error'));
    expect((t.controller.current as { message: string }).message).toContain('uploaded');
  });

  it('refuses a format the browser cannot record', async () => {
    const t = setup({ mimeType: () => null });
    await t.controller.start();
    expect(t.controller.current).toMatchObject({ phase: 'error', code: 'capture-failed' });
    expect(t.api.createRecording).not.toHaveBeenCalled();
  });
});
