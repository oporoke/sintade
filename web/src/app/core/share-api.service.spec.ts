import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { ShareApi, shareUrl } from './share-api.service';

describe('ShareApi', () => {
  let api: ShareApi;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    api = TestBed.inject(ShareApi);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('creates, lists, updates and revokes a recording’s links', async () => {
    const created = api.create('rec-1', { visibility: 'link' });
    const create = http.expectOne('/api/v1/recordings/rec-1/links');
    expect(create.request.method).toBe('POST');
    expect(create.request.body).toEqual({ visibility: 'link' });
    create.flush({ id: 'l1', slug: 'abcdefghijkl' });
    expect((await created).slug).toBe('abcdefghijkl');

    const listed = api.list('rec-1');
    http.expectOne({ method: 'GET', url: '/api/v1/recordings/rec-1/links' }).flush([]);
    expect(await listed).toEqual([]);

    const updated = api.update('rec-1', 'l1', { visibility: 'private', expires_at: null });
    const patch = http.expectOne('/api/v1/recordings/rec-1/links/l1');
    expect(patch.request.method).toBe('PATCH');
    expect(patch.request.body).toEqual({ visibility: 'private', expires_at: null });
    patch.flush({ id: 'l1' });
    await updated;

    const revoked = api.revoke('rec-1', 'l1');
    const del = http.expectOne('/api/v1/recordings/rec-1/links/l1');
    expect(del.request.method).toBe('DELETE');
    del.flush(null, { status: 204, statusText: 'No Content' });
    await revoked;
  });
});

describe('shareUrl', () => {
  it('points at the watch page', () => {
    expect(shareUrl('abcdefghijkl', 'https://sintade.app')).toBe(
      'https://sintade.app/s/abcdefghijkl',
    );
  });
});
