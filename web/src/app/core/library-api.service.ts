import { Injectable, InjectionToken, inject } from '@angular/core';
import { firstValueFrom } from 'rxjs';

import { components } from '../api/schema';
import { ApiClient } from './api-client.service';

type Schemas = components['schemas'];
export type RecordingSummary = Schemas['RecordingSummary'];
export type RecordingList = Schemas['RecordingList'];
export type DownloadData = Schemas['DownloadResponse'];

export interface LibraryPort {
  list(cursor: string | null): Promise<RecordingList>;
  download(recordingId: string): Promise<DownloadData>;
}

/** The workspace's recordings (docs/design.md §9: `GET /recordings`, 24 per page). */
@Injectable({ providedIn: 'root' })
export class LibraryApi implements LibraryPort {
  private readonly api = inject(ApiClient);

  list(cursor: string | null): Promise<RecordingList> {
    const query = cursor ? `?cursor=${encodeURIComponent(cursor)}` : '';
    return firstValueFrom(this.api.get<RecordingList>(`/recordings${query}`));
  }

  download(recordingId: string): Promise<DownloadData> {
    return firstValueFrom(
      this.api.get<DownloadData>(`/recordings/${encodeURIComponent(recordingId)}/download`),
    );
  }
}

export const LIBRARY_API = new InjectionToken<LibraryPort>('LibraryPort', {
  providedIn: 'root',
  factory: () => inject(LibraryApi),
});
