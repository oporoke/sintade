import { HttpErrorResponse } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable, firstValueFrom } from 'rxjs';

import { components } from '../api/schema';
import { PresignedUrl, UploadApi, UploadHttpError } from '../capture';
import { ApiClient } from './api-client.service';

type Schemas = components['schemas'];
export type CreateRecordingBody = Schemas['CreateRecordingBody'];
export type CreateRecordingResponse = Schemas['CreateRecordingResponse'];
export type TakeStatusResponse = Schemas['TakeStatusResponse'];

/**
 * The ingest endpoints (upload protocol v1) over the app's HTTP stack, which adds the session
 * cookie and CSRF header. Implements the capture engine's `UploadApi` port; failures surface as
 * `UploadHttpError`s (status 0 = no response) so the uploader can decide whether to retry.
 */
@Injectable({ providedIn: 'root' })
export class IngestApi implements UploadApi {
  private readonly api = inject(ApiClient);

  createRecording(body: CreateRecordingBody): Promise<CreateRecordingResponse> {
    return call(this.api.post<CreateRecordingResponse>('/recordings', body));
  }

  async presign(takeId: string, idx: number, count: number): Promise<PresignedUrl[]> {
    const response = await call(
      this.api.post<Schemas['PresignChunksResponse']>(
        `/takes/${encodeURIComponent(takeId)}/chunks/${idx}/url?count=${count}`,
        null,
      ),
    );
    return response.urls;
  }

  async ack(takeId: string, idx: number, sizeBytes: number, sha256: string): Promise<void> {
    const body: Schemas['AckChunkBody'] = { size_bytes: sizeBytes, sha256 };
    await call(
      this.api.post<Schemas['AckChunkResponse']>(
        `/takes/${encodeURIComponent(takeId)}/chunks/${idx}/ack`,
        body,
      ),
    );
  }

  status(takeId: string): Promise<TakeStatusResponse> {
    return call(this.api.get<TakeStatusResponse>(`/takes/${encodeURIComponent(takeId)}/status`));
  }
}

/** Awaits one response, turning HTTP failures into the capture engine's `UploadHttpError`. */
async function call<T>(request: Observable<T>): Promise<T> {
  try {
    return await firstValueFrom(request);
  } catch (error) {
    if (error instanceof HttpErrorResponse) {
      throw new UploadHttpError('api', error.status, error.message);
    }
    throw error;
  }
}
