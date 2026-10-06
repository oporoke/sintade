import { Injectable, InjectionToken } from '@angular/core';
import { Observable } from 'rxjs';

import { components } from '../api/schema';

export type RecordingStatus = components['schemas']['RecordingStatus'];

/**
 * Live status of a recording (docs/design.md §9 `GET /recordings/{id}/events`, `GET
 * /s/{slug}/events`): the current status at once, then again every time it changes. The
 * observable errors when the stream can't be opened or is refused (the caller falls back to
 * asking now and then) and completes when the recording is gone.
 */
export interface StatusStreamPort {
  follow(path: string): Observable<RecordingStatus>;
}

const API_BASE_URL = '/api/v1';

/** Server-Sent Events through the browser's `EventSource`, which also reconnects by itself. */
@Injectable({ providedIn: 'root' })
export class EventSourceStatusStream implements StatusStreamPort {
  follow(path: string): Observable<RecordingStatus> {
    return new Observable<RecordingStatus>((subscriber) => {
      if (typeof EventSource === 'undefined') {
        subscriber.error(new Error('EventSource is not available'));
        return undefined;
      }
      const source = new EventSource(`${API_BASE_URL}${path}`, { withCredentials: true });
      source.addEventListener('status', (event) => {
        try {
          subscriber.next(JSON.parse((event as MessageEvent<string>).data) as RecordingStatus);
        } catch {
          // A frame that isn't ours: ignore it, the next one carries the whole status.
        }
      });
      source.addEventListener('gone', () => subscriber.complete());
      source.addEventListener('error', () => {
        // While `CONNECTING` the browser is retrying on its own; `CLOSED` means it gave up
        // (a 401/404, or the server is down for good).
        if (source.readyState === EventSource.CLOSED) {
          subscriber.error(new Error('status stream closed'));
        }
      });
      return () => source.close();
    });
  }
}

export const STATUS_STREAM = new InjectionToken<StatusStreamPort>('StatusStreamPort', {
  providedIn: 'root',
  factory: () => new EventSourceStatusStream(),
});
