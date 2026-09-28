import { Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import { components } from '../api/schema';
import { PresignedUrl, UploadApi } from '../capture';
import { ApiClient } from './api-client.service';

type Schemas = components['schemas'];
export type CreateRecordingBody = Schemas['CreateRecordingBody'];
export type CreateRecordingResponse = Schemas['CreateRecordingResponse'];
export type TakeStatusResponse = Schemas['TakeStatusResponse'];

/**
 * The ingest endpoints (upload protocol v1) over the app's HTTP stack, which adds the session
 * cookie and CSRF header. Implements the capture engine's `UploadApi` port.
 */
@Injectable({ providedIn: 'root' })
export class IngestApi implements UploadApi {
  private readonly api = inject(ApiClient);

  createRecording(body: CreateRecordingBody): Promise<CreateRecordingResponse> {
    return firstValueFrom(this.api.post<CreateRecordingResponse>('/recordings', body));
  }

  async presign(takeId: string, idx: number, count: number): Promise<PresignedUrl[]> {
    const response = await firstValueFrom(
      this.api.post<Schemas['PresignChunksResponse']>(
        `/takes/${encodeURIComponent(takeId)}/chunks/${idx}/url?count=${count}`,
        null,
      ),
    );
    return response.urls;
  }

  async ack(takeId: string, idx: number, sizeBytes: number, sha256: string): Promise<void> {
    const body: Schemas['AckChunkBody'] = { size_bytes: sizeBytes, sha256 };
    await firstValueFrom(
      this.api.post<Schemas['AckChunkResponse']>(
        `/takes/${encodeURIComponent(takeId)}/chunks/${idx}/ack`,
        body,
      ),
    );
  }

  status(takeId: string): Promise<TakeStatusResponse> {
    return firstValueFrom(
      this.api.get<TakeStatusResponse>(`/takes/${encodeURIComponent(takeId)}/status`),
    );
  }
}
