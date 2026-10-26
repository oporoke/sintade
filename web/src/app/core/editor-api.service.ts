import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import { components } from '../api/schema';
import { Waveform } from '../feature/editor/waveform';
import { ApiClient } from './api-client.service';

export type EditorSource = components['schemas']['EditorSourceResponse'];
type WaveformResponse = components['schemas']['WaveformResponse'];

/** `status` is the HTTP status (0: no response). */
export class EditorHttpError extends Error {
  constructor(readonly status: number) {
    super(`editor request failed (${status})`);
  }
}

/** What the editor works on: the original recording and its peaks (ADR-0039, ADR-0040). */
@Injectable({ providedIn: 'root' })
export class EditorApi {
  private readonly api = inject(ApiClient);

  async source(recordingId: string): Promise<EditorSource> {
    try {
      return await firstValueFrom(
        this.api.get<EditorSource>(`/recordings/${encodeURIComponent(recordingId)}/editor`),
      );
    } catch (error) {
      throw new EditorHttpError(error instanceof HttpErrorResponse ? error.status : 0);
    }
  }

  /** The peaks; `null` while they are still being made (the API answers `409`). */
  async waveform(recordingId: string): Promise<Waveform | null> {
    let grant: WaveformResponse;
    try {
      grant = await firstValueFrom(
        this.api.get<WaveformResponse>(`/recordings/${encodeURIComponent(recordingId)}/waveform`),
      );
    } catch (error) {
      const status = error instanceof HttpErrorResponse ? error.status : 0;
      if (status === 409) {
        return null;
      }
      throw new EditorHttpError(status);
    }
    const response = await fetch(grant.url);
    if (!response.ok) {
      throw new EditorHttpError(response.status);
    }
    return (await response.json()) as Waveform;
  }
}
