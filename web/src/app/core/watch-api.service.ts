import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import { components } from '../api/schema';
import { ApiClient } from './api-client.service';

type Schemas = components['schemas'];
export type WatchData = Schemas['WatchResponse'];
export type PlaybackData = Schemas['PlaybackResponse'];

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
}

async function call<T>(request: ReturnType<ApiClient['get']>): Promise<T> {
  try {
    return (await firstValueFrom(request)) as T;
  } catch (error) {
    throw new WatchHttpError(error instanceof HttpErrorResponse ? error.status : 0);
  }
}
