import { describe, expect, it, vi } from 'vitest';

import { CLIENT_HEADER, Fetch, SintadeApi } from './api';

const ME = {
  user: { id: 'u1', email: 'a@example.com', display_name: 'Amina', email_verified: true },
  workspaces: [
    { id: 'w1', name: 'Personal', role: 'owner', is_personal: true },
    { id: 'w2', name: 'Team', role: 'member', is_personal: false },
  ],
  current_workspace_id: 'w2',
};

function json(body: unknown, status = 200): Response {
  return new Response(JSON.stringify(body), {
    status,
    headers: { 'content-type': 'application/json' },
  });
}

function api(fetchFn: Fetch) {
  return new SintadeApi('https://app.test', '0.2.0', fetchFn);
}

describe('SintadeApi.whoAmI', () => {
  it('calls /me with the session cookie and the extension header', async () => {
    const fetchFn = vi.fn<Fetch>().mockResolvedValue(json(ME));
    const reply = await api(fetchFn).whoAmI();

    const [url, init] = fetchFn.mock.calls[0] ?? [];
    expect(url).toBe('https://app.test/api/v1/me');
    expect(init?.credentials).toBe('include');
    expect(new Headers(init?.headers).get(CLIENT_HEADER)).toBe('extension/0.2.0');
    expect(reply).toEqual({
      state: 'signed-in',
      email: 'a@example.com',
      displayName: 'Amina',
      workspace: { id: 'w2', name: 'Team', role: 'member', is_personal: false },
    });
  });

  it('is signed out when there is no session and nothing to refresh', async () => {
    const fetchFn = vi
      .fn<Fetch>()
      .mockResolvedValueOnce(json({ title: 'Unauthorized' }, 401))
      .mockResolvedValueOnce(json({ title: 'Unauthorized' }, 401));
    expect(await api(fetchFn).whoAmI()).toEqual({ state: 'signed-out' });
    expect(fetchFn.mock.calls[1]?.[0]).toBe('https://app.test/api/v1/auth/refresh');
    expect(fetchFn.mock.calls[1]?.[1]?.method).toBe('POST');
  });

  it('refreshes an expired access cookie once, then reads /me again', async () => {
    const fetchFn = vi
      .fn<Fetch>()
      .mockResolvedValueOnce(json({}, 401))
      .mockResolvedValueOnce(json({ message: 'ok' }))
      .mockResolvedValueOnce(json(ME));
    const reply = await api(fetchFn).whoAmI();
    expect(reply.state).toBe('signed-in');
    expect(fetchFn).toHaveBeenCalledTimes(3);
  });

  it('is unreachable when the server cannot be contacted or errors', async () => {
    expect(await api(vi.fn<Fetch>().mockRejectedValue(new TypeError('offline'))).whoAmI()).toEqual({
      state: 'unreachable',
    });
    expect(await api(vi.fn<Fetch>().mockResolvedValue(json({}, 500))).whoAmI()).toEqual({
      state: 'unreachable',
    });
  });

  it('points sign-in at the web app', () => {
    expect(api(vi.fn<Fetch>()).signInUrl()).toBe('https://app.test/login');
  });
});
