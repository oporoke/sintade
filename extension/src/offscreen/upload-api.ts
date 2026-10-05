import type { FinalizedTake, PresignedUrl, UploadApi, UploadStatus } from '@capture';
import { UploadHttpError } from '@capture';

import type { SintadeApi } from '../background/api';

export interface CreatedRecording {
  recording_id: string;
  take_id: string;
  max_duration_ms: number;
}

export interface NewRecordingBody {
  mime_type: string;
  has_system_audio: boolean;
  has_mic: boolean;
  has_camera: boolean;
}

export type ExtensionApi = Pick<SintadeApi, 'request'>;

/** The ingest protocol (upload protocol v1) over the extension's requests; the capture engine's `UploadApi` port. */
export class ExtensionUploadApi implements UploadApi {
  constructor(private readonly api: ExtensionApi) {}

  createRecording(body: NewRecordingBody): Promise<CreatedRecording> {
    return this.json<CreatedRecording>('/recordings', { method: 'POST', body });
  }

  async presign(takeId: string, idx: number, count: number): Promise<PresignedUrl[]> {
    const response = await this.json<{ urls: PresignedUrl[] }>(
      `/takes/${encodeURIComponent(takeId)}/chunks/${idx}/url?count=${count}`,
      { method: 'POST' },
    );
    return response.urls;
  }

  async ack(takeId: string, idx: number, sizeBytes: number, sha256: string): Promise<void> {
    await this.json(`/takes/${encodeURIComponent(takeId)}/chunks/${idx}/ack`, {
      method: 'POST',
      body: { size_bytes: sizeBytes, sha256 },
    });
  }

  status(takeId: string): Promise<UploadStatus> {
    return this.json<UploadStatus>(`/takes/${encodeURIComponent(takeId)}/status`);
  }

  finalize(takeId: string, chunkCount: number, durationMs: number): Promise<FinalizedTake> {
    return this.json<FinalizedTake>(`/takes/${encodeURIComponent(takeId)}/finalize`, {
      method: 'POST',
      body: { chunk_count: chunkCount, duration_ms: Math.round(durationMs) },
    });
  }

  /** `POST /recordings/{id}/links`: the shareable address of the recording. */
  async createLink(recordingId: string): Promise<{ slug: string }> {
    return this.json<{ slug: string }>(`/recordings/${encodeURIComponent(recordingId)}/links`, {
      method: 'POST',
      body: { visibility: 'link' },
    });
  }

  private async json<T>(
    path: string,
    options: { method?: string; body?: unknown } = {},
  ): Promise<T> {
    let response: Response;
    try {
      response = await this.api.request(path, {
        method: options.method ?? 'GET',
        ...(options.body === undefined
          ? {}
          : {
              headers: { 'content-type': 'application/json' },
              body: JSON.stringify(options.body),
            }),
      });
    } catch (error) {
      throw new UploadHttpError('api', 0, error instanceof Error ? error.message : 'offline');
    }
    if (!response.ok) {
      throw new UploadHttpError('api', response.status, `${path}: ${response.status}`);
    }
    const text = await response.text();
    return (text ? JSON.parse(text) : null) as T;
  }
}
