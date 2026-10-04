import { provideHttpClient } from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { LibraryApi } from './library-api.service';

describe('LibraryApi', () => {
  let api: LibraryApi;
  let http: HttpTestingController;

  beforeEach(() => {
    TestBed.configureTestingModule({
      providers: [provideHttpClient(), provideHttpClientTesting()],
    });
    api = TestBed.inject(LibraryApi);
    http = TestBed.inject(HttpTestingController);
  });

  afterEach(() => http.verify());

  it('lists the first page, then the next with its cursor', async () => {
    const first = api.list(null);
    http.expectOne({ method: 'GET', url: '/api/v1/recordings' }).flush({
      items: [],
      next_cursor: '17.abc',
    });
    expect((await first).next_cursor).toBe('17.abc');

    const second = api.list('17.abc');
    http.expectOne('/api/v1/recordings?cursor=17.abc').flush({ items: [], next_cursor: null });
    expect((await second).next_cursor).toBeNull();
  });

  it('asks for a download URL', async () => {
    const result = api.download('rec 1');
    http
      .expectOne('/api/v1/recordings/rec%201/download')
      .flush({ url: 'https://store/x', filename: 'x.mp4', expires_in_s: 900 });
    expect((await result).filename).toBe('x.mp4');
  });

  it('renames and trashes a recording', async () => {
    const renamed = api.rename('rec-1', 'New name');
    const patch = http.expectOne('/api/v1/recordings/rec-1');
    expect(patch.request.method).toBe('PATCH');
    expect(patch.request.body).toEqual({ title: 'New name' });
    patch.flush({ id: 'rec-1', title: 'New name' });
    expect((await renamed).title).toBe('New name');

    const trashed = api.trash('rec-1');
    const del = http.expectOne('/api/v1/recordings/rec-1');
    expect(del.request.method).toBe('DELETE');
    del.flush(null, { status: 204, statusText: 'No Content' });
    await trashed;
  });
});
