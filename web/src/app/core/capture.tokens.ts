import { InjectionToken, Signal, computed, inject } from '@angular/core';

import {
  AudioMixer,
  ChunkStore,
  SourceManager,
  StorageStatus,
  WakeLockGuard,
  checkStorage,
  TakeSession,
  TakeSessionOptions,
  openChunkStore,
  UploadApi,
} from '../capture';
import { AuthService } from './auth.service';
import { CreateRecordingBody, CreateRecordingResponse, IngestApi } from './ingest-api.service';

/*
 * Angular's handles on the framework-free capture engine. The capture package itself knows
 * nothing about DI; pages inject these so tests can substitute fakes.
 */

export const SOURCE_MANAGER = new InjectionToken<SourceManager>('SourceManager', {
  factory: () => new SourceManager(),
});

/** A fresh mixer per injector: each recorder/page owns its own Web Audio graph. */
export const AUDIO_MIXER = new InjectionToken<AudioMixer>('AudioMixer', {
  factory: () => new AudioMixer(),
});

/**
 * The device's chunk store, opened once per app (OPFS, or IndexedDB where OPFS can't be
 * written). A promise: opening is async, and callers must handle it being unavailable.
 */
export const CHUNK_STORE = new InjectionToken<Promise<ChunkStore>>('ChunkStore', {
  providedIn: 'root',
  factory: () => {
    const store = openChunkStore();
    // Consumers await it and handle failure; don't report an unhandled rejection meanwhile.
    store.catch(() => undefined);
    return store;
  },
});

/** Starts a take (overridable so pages can be tested without MediaRecorder and storage). */
export const START_TAKE = new InjectionToken<(options: TakeSessionOptions) => Promise<TakeSession>>(
  'StartTake',
  { providedIn: 'root', factory: () => (options) => TakeSession.start(options) },
);

/** Creating recordings plus the upload protocol: what the recorder needs from the server. */
export interface RecordingsApi extends UploadApi {
  createRecording(body: CreateRecordingBody): Promise<CreateRecordingResponse>;
}

/** The server side of recording (overridable so pages can be tested without HTTP). */
export const RECORDINGS_API = new InjectionToken<RecordingsApi>('RecordingsApi', {
  providedIn: 'root',
  factory: () => inject(IngestApi),
});

/** What the current plan lets the recorder offer. Unknown limits are the free tier's. */
export interface PlanLimits {
  maxResolution: number;
  maxDurationMs: number;
}

export const FREE_PLAN_LIMITS: PlanLimits = { maxResolution: 1080, maxDurationMs: 600_000 };

export const PLAN_LIMITS = new InjectionToken<Signal<PlanLimits>>('PlanLimits', {
  factory: () => {
    const auth = inject(AuthService);
    return computed(() => {
      const entitlements = auth.currentUser()?.entitlements;
      return entitlements
        ? {
            maxResolution: entitlements.max_resolution,
            maxDurationMs: entitlements.max_duration_ms,
          }
        : FREE_PLAN_LIMITS;
    });
  },
});

/** A fresh wake-lock guard per recorder page. */
export const WAKE_LOCK_GUARD = new InjectionToken<WakeLockGuard>('WakeLockGuard', {
  factory: () => new WakeLockGuard(),
});

/** Reads the device's storage headroom; replaceable in tests. */
export const STORAGE_CHECK = new InjectionToken<(expectedBytes: number) => Promise<StorageStatus>>(
  'StorageCheck',
  { factory: () => (expectedBytes) => checkStorage(undefined, expectedBytes) },
);
