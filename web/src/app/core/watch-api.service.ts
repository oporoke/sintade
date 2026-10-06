import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import { components } from '../api/schema';
import { ApiClient } from './api-client.service';

type Schemas = components['schemas'];
export type WatchData = Schemas['WatchResponse'];
export type PlaybackData = Schemas['PlaybackResponse'];
export type DownloadData = Schemas['DownloadResponse'];

/** `status` is the HTTP status (0: no response). */
export class WatchHttpError extends Error {
  constructor(readonly status: number) {
    super(`watch request failed (${status})`);
  }
}

/** The viewer-facing endpoints: reachable without a session (docs/design.md §10 Watch). */
@Injectable({ providedIn: 'root' })
export class WatchApi {
  private readonly api = inject(ApiClient);

  watch(slug: string): Promise<WatchData> {
    return call(this.api.get<WatchData>(`/s/${encodeURIComponent(slug)}`));
  }

  playback(slug: string): Promise<PlaybackData> {
    return call(this.api.get<PlaybackData>(`/s/${encodeURIComponent(slug)}/playback`));
  }

  /** The scrub sprite's cue file (`PlaybackData.sprite_url`), as text. */
  async sprite(url: string): Promise<string> {
    const response = await fetch(url, { credentials: 'include' });
    if (!response.ok) {
      throw new WatchHttpError(response.status);
    }
    return response.text();
  }

  download(slug: string): Promise<DownloadData> {
    return call(this.api.get<DownloadData>(`/s/${encodeURIComponent(slug)}/download`));
  }
}

async function call<T>(request: ReturnType<ApiClient['get']>): Promise<T> {
  try {
    return (await firstValueFrom(request)) as T;
  } catch (error) {
    throw new WatchHttpError(error instanceof HttpErrorResponse ? error.status : 0);
  }
}
