import { describe, expect, it, vi } from 'vitest';

import type { WhoAmIReply } from '../shared/messages';
import { handleMessage } from './handlers';

function env(whoAmI: WhoAmIReply = { state: 'signed-out' }) {
  return {
    version: '1.2.3',
    api: { whoAmI: vi.fn().mockResolvedValue(whoAmI), signInUrl: () => 'https://app.test/login' },
    recorder: {
      start: vi.fn().mockResolvedValue({ phase: 'starting' }),
      stop: vi.fn().mockResolvedValue({ phase: 'uploading' }),
      status: vi.fn().mockResolvedValue({ phase: 'idle' }),
      dismiss: vi.fn().mockResolvedValue(undefined),
    },
    openTab: vi.fn().mockResolvedValue(undefined),
  };
}

describe('handleMessage', () => {
  it('answers a ping with the extension version', async () => {
    expect(await handleMessage({ type: 'ping' }, env())).toEqual({ ok: true, version: '1.2.3' });
  });

  it('answers whoami from the API', async () => {
    const me: WhoAmIReply = {
      state: 'signed-in',
      email: 'a@example.com',
      displayName: 'A',
      workspace: null,
    };
    expect(await handleMessage({ type: 'whoami' }, env(me))).toEqual(me);
  });

  it('opens the sign-in page in a tab', async () => {
    const e = env();
    expect(await handleMessage({ type: 'open-sign-in' }, e)).toEqual({ ok: true });
    expect(e.openTab).toHaveBeenCalledWith('https://app.test/login');
  });

  it('starts, stops and reports the recording', async () => {
    const e = env();
    expect(await handleMessage({ type: 'start-recording', tabId: 7 }, e)).toEqual({
      state: { phase: 'starting' },
    });
    expect(e.recorder.start).toHaveBeenCalledWith(7);
    expect(await handleMessage({ type: 'stop-recording' }, e)).toEqual({
      state: { phase: 'uploading' },
    });
    expect(await handleMessage({ type: 'recording-status' }, e)).toEqual({
      state: { phase: 'idle' },
    });
  });

  it('dismisses a finished recording and reports the state after', async () => {
    const e = env();
    expect(await handleMessage({ type: 'dismiss-recording' }, e)).toEqual({
      state: { phase: 'idle' },
    });
    expect(e.recorder.dismiss).toHaveBeenCalled();
  });

  it('refuses a start without a tab id', async () => {
    const e = env();
    expect(await handleMessage({ type: 'start-recording' }, e)).toBeNull();
    expect(await handleMessage({ type: 'start-recording', tabId: 'x' }, e)).toBeNull();
    expect(e.recorder.start).not.toHaveBeenCalled();
  });

  it('ignores anything that is not one of our messages', async () => {
    for (const message of [null, undefined, 'ping', 42, {}, { type: 'other' }, { type: 1 }]) {
      expect(await handleMessage(message, env())).toBeNull();
    }
  });
});
