import { Injectable, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import { components } from '../api/schema';
import { ApiClient } from './api-client.service';

export type Chapter = components['schemas']['ChapterDto'];
type ChaptersBody = components['schemas']['ChaptersBody'];

/** A recording's chapters, for its owner to edit (`/recordings/{id}/chapters`). */
@Injectable({ providedIn: 'root' })
export class ChaptersApi {
  private readonly api = inject(ApiClient);

  async get(recordingId: string): Promise<Chapter[]> {
    const body = await firstValueFrom(
      this.api.get<ChaptersBody>(`/recordings/${encodeURIComponent(recordingId)}/chapters`),
    );
    return body.chapters;
  }

  async put(recordingId: string, chapters: Chapter[]): Promise<Chapter[]> {
    const body = await firstValueFrom(
      this.api.put<ChaptersBody>(`/recordings/${encodeURIComponent(recordingId)}/chapters`, {
        chapters,
      }),
    );
    return body.chapters;
  }
}
