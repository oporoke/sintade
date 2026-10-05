import { describe, expect, it } from 'vitest';

import { handleMessage } from './handlers';

describe('handleMessage', () => {
  it('answers a ping with the extension version', () => {
    expect(handleMessage({ type: 'ping' }, { version: '1.2.3' })).toEqual({
      ok: true,
      version: '1.2.3',
    });
  });

  it('ignores anything that is not one of our messages', () => {
    for (const message of [null, undefined, 'ping', 42, {}, { type: 'other' }, { type: 1 }]) {
      expect(handleMessage(message, { version: '1.0.0' })).toBeNull();
    }
  });
});
