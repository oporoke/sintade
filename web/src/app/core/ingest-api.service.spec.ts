import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { UploadHttpError } from '../capture';
import { IngestApi } from './ingest-api.service';

describe('IngestApi', () => {
  let api: IngestApi;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    api = TestBed.inject(IngestApi);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('presigns a batch and returns its URLs', async () => {
    const result = api.presign('take-1', 3, 2);
    const request = http.expectOne('/api/v1/takes/take-1/chunks/3/url?count=2');
    expect(request.request.method).toBe('POST');
    expect(request.request.withCredentials).toBe(true);
    request.flush({
      urls: [
        { idx: 3, url: 'https://store/3' },
        { idx: 4, url: 'https://store/4' },
      ],
      expires_in_s: 300,
    });
    expect(await result).toEqual([
      { idx: 3, url: 'https://store/3' },
      { idx: 4, url: 'https://store/4' },
    ]);
  });

  it('acks with the size and hash', async () => {
    const result = api.ack('take-1', 0, 1234, 'ab'.repeat(32));
    const request = http.expectOne('/api/v1/takes/take-1/chunks/0/ack');
    expect(request.request.body).toEqual({ size_bytes: 1234, sha256: 'ab'.repeat(32) });
    request.flush({ idx: 0, status: 'acked' });
    await result;
  });

  it('rejects when the server refuses an ack', async () => {
    const result = api.ack('take-1', 0, 1, 'ab'.repeat(32));
    http
      .expectOne('/api/v1/takes/take-1/chunks/0/ack')
      .flush({ status: 409 }, { status: 409, statusText: 'Conflict' });
    const error = await result.catch((e: unknown) => e);
    expect(error).toBeInstanceOf(UploadHttpError);
    expect((error as UploadHttpError).status).toBe(409);
    expect((error as UploadHttpError).source).toBe('api');
  });

  it('reports a dropped connection as status 0', async () => {
    const result = api.status('take-1');
    http
      .expectOne('/api/v1/takes/take-1/status')
      .error(new ProgressEvent('error'), { status: 0, statusText: 'Unknown Error' });
    const error = await result.catch((e: unknown) => e);
    expect((error as UploadHttpError).status).toBe(0);
  });

  it('finalizes with the chunk count and a whole-millisecond duration', async () => {
    const result = api.finalize('take-1', 5, 9875.9);
    const request = http.expectOne('/api/v1/takes/take-1/finalize');
    expect(request.request.body).toEqual({ chunk_count: 5, duration_ms: 9876 });
    request.flush(
      { recording_id: 'rec-1', state: 'processing' },
      { status: 202, statusText: 'Accepted' },
    );
    expect((await result).recording_id).toBe('rec-1');
  });
});
