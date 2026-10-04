import type { WhoAmIReply } from '../shared/messages';

/** The header every request from the extension carries (see `bin/api/src/csrf.rs`). */
export const CLIENT_HEADER = 'X-Sintade-Client';

/** The part of `GET /api/v1/me` the extension reads. */
interface MeBody {
  user: { email: string; display_name: string };
  workspaces: { id: string; name: string; role: string }[];
  current_workspace_id: string;
}

export type Fetch = (input: string, init?: RequestInit) => Promise<Response>;

/**
 * Sintade's API as the extension sees it. The extension has host permission for the app's
 * origin, so its requests carry the browser's session cookies: whoever is signed in to the web
 * app is signed in here (ADR-0018). Nothing is stored or copied.
 */
export class SintadeApi {
  constructor(
    private readonly origin: string,
    private readonly version: string,
    private readonly doFetch: Fetch = (input, init) => fetch(input, init),
  ) {}

  /** A request as the signed-in user: session cookie, and the client header in place of CSRF. */
  request(path: string, init: RequestInit = {}): Promise<Response> {
    const headers = new Headers(init.headers);
    headers.set(CLIENT_HEADER, `extension/${this.version}`);
    return this.doFetch(`${this.origin}/api/v1${path}`, {
      ...init,
      headers,
      credentials: 'include',
      cache: 'no-store',
    });
  }

  async whoAmI(): Promise<WhoAmIReply> {
    let response: Response;
    try {
      response = await this.request('/me');
    } catch {
      return { state: 'unreachable' };
    }
    if (response.status === 401) {
      // The access cookie lasts 15 minutes; the web app refreshes it from the refresh cookie.
      const refreshed = await this.tryRefresh();
      if (!refreshed) {
        return { state: 'signed-out' };
      }
      try {
        response = await this.request('/me');
      } catch {
        return { state: 'unreachable' };
      }
      if (response.status === 401) {
        return { state: 'signed-out' };
      }
    }
    if (!response.ok) {
      return { state: 'unreachable' };
    }
    const me = (await response.json()) as MeBody;
    const workspace = me.workspaces.find((w) => w.id === me.current_workspace_id) ?? null;
    return {
      state: 'signed-in',
      email: me.user.email,
      displayName: me.user.display_name,
      workspace,
    };
  }

  /** Swaps the refresh cookie for a new access cookie, as the web app does on a 401. */
  private async tryRefresh(): Promise<boolean> {
    try {
      const response = await this.request('/auth/refresh', { method: 'POST' });
      return response.ok;
    } catch {
      return false;
    }
  }

  signInUrl(): string {
    return `${this.origin}/login`;
  }
}
