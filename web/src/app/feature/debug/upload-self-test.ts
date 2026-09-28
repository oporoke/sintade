import { ChunkStore, Uploader, sha256Hex } from '../../capture';
import { IngestApi } from '../../core/ingest-api.service';
import { openBackend } from './chunk-store-self-test';

export interface UploadedChunkCheck {
  idx: number;
  sizeBytes: number;
  /** Hash of the bytes this page stored and uploaded. */
  localSha256: string;
  /** Hash the server recorded at ack (`GET /takes/{id}/status`). */
  serverSha256: string | null;
}

export interface UploadSelfTestResult {
  recordingId: string;
  takeId: string;
  backend: ChunkStore['backend'];
  chunks: UploadedChunkCheck[];
  /** Every chunk reached the server and its recorded hash equals the local one. */
  allMatch: boolean;
}

const CHUNKS = 3;

/**
 * Day 37 Check, "uploaded chunk hashes match server records": creates a recording on the real
 * API, writes a few reproducible chunks to a diagnostics store, uploads them with the real
 * `Uploader` (SubtleCrypto hash, presign, PUT to storage, ack), then compares each local hash
 * with what the server recorded. Needs a signed-in session.
 */
export async function runUploadSelfTest(api: IngestApi): Promise<UploadSelfTestResult> {
  const store = (await openBackend('opfs')) ?? (await openBackend('indexeddb'));
  if (!store) {
    throw new Error('no chunk store available');
  }
  const { recording_id, take_id } = await api.createRecording({
    title: 'Upload self-test',
    mime_type: 'video/webm',
    has_system_audio: false,
    has_mic: false,
    has_camera: false,
  });
  try {
    const uploader = new Uploader({ api, store, takeId: take_id });
    const localSha256 = new Map<number, string>();
    const sizes = new Map<number, number>();
    for (let idx = 0; idx < CHUNKS; idx += 1) {
      const blob = new Blob([chunkBytes(take_id, idx)], { type: 'video/webm' });
      await store.put(take_id, idx, blob);
      localSha256.set(idx, await sha256Hex(blob));
      sizes.set(idx, blob.size);
      uploader.enqueue(idx);
    }
    await uploader.drained();

    const status = await api.status(take_id);
    const server = new Map(status.chunks.map((chunk) => [chunk.idx, chunk.sha256]));
    const chunks = [...localSha256].map(([idx, sha]) => ({
      idx,
      sizeBytes: sizes.get(idx) ?? 0,
      localSha256: sha,
      serverSha256: server.get(idx) ?? null,
    }));
    return {
      recordingId: recording_id,
      takeId: take_id,
      backend: store.backend,
      chunks,
      allMatch: chunks.every((chunk) => chunk.serverSha256 === chunk.localSha256),
    };
  } finally {
    await store.deleteTake(take_id);
  }
}

/** 32 KiB plus a per-index amount of pseudo-random bytes seeded by (take, index). */
export function chunkBytes(takeId: string, idx: number): Uint8Array<ArrayBuffer> {
  let seed = [...`${takeId}#${idx}`].reduce((hash, c) => (hash * 31 + c.charCodeAt(0)) >>> 0, 11);
  const bytes = new Uint8Array(32_768 + idx * 777);
  for (let i = 0; i < bytes.length; i += 1) {
    seed = (seed * 1_664_525 + 1_013_904_223) >>> 0;
    bytes[i] = seed >>> 24;
  }
  return bytes;
}
