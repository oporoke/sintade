import {
  HttpClient,
  HttpErrorResponse,
  provideHttpClient,
  withInterceptors,
} from '@angular/common/http';
import { HttpTestingController, provideHttpClientTesting } from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';
import { of } from 'rxjs';

import { AuthService } from './auth.service';
import { silentRefreshInterceptor } from './silent-refresh.interceptor';

describe('silentRefreshInterceptor', () => {
  let httpClient: HttpClient;
  let httpTesting: HttpTestingController;

  function setup(refresh: ReturnType<typeof vi.fn>) {
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([silentRefreshInterceptor])),
        provideHttpClientTesting(),
        { provide: AuthService, useValue: { refresh } },
      ],
    });
    httpClient = TestBed.inject(HttpClient);
    httpTesting = TestBed.inject(HttpTestingController);
  }

  it('retries the request once after a silent refresh on 401', () => {
    const refresh = vi.fn().mockReturnValue(of({ message: 'refreshed' }));
    setup(refresh);

    let result: unknown;
    httpClient.get('/api/v1/me').subscribe((value) => (result = value));

    const first = httpTesting.expectOne('/api/v1/me');
    first.flush('unauthorized', { status: 401, statusText: 'Unauthorized' });

    expect(refresh).toHaveBeenCalled();

    const retried = httpTesting.expectOne('/api/v1/me');
    retried.flush({ ok: true });

    expect(result).toEqual({ ok: true });
  });

  it('does not attempt a silent refresh for /auth/login itself', () => {
    const refresh = vi.fn();
    setup(refresh);

    let error: unknown;
    httpClient.post('/api/v1/auth/login', {}).subscribe({ error: (e: unknown) => (error = e) });

    const request = httpTesting.expectOne('/api/v1/auth/login');
    request.flush('bad credentials', { status: 401, statusText: 'Unauthorized' });

    expect(refresh).not.toHaveBeenCalled();
    expect(error).toBeInstanceOf(HttpErrorResponse);
  });
});
