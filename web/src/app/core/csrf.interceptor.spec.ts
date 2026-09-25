import { HttpClient, provideHttpClient, withInterceptors } from '@angular/common/http';
import {
  HttpTestingController,
  provideHttpClientTesting,
} from '@angular/common/http/testing';
import { TestBed } from '@angular/core/testing';

import { csrfInterceptor } from './csrf.interceptor';

describe('csrfInterceptor', () => {
  let httpClient: HttpClient;
  let httpTesting: HttpTestingController;

  beforeEach(() => {
    document.cookie = 'sintade_csrf=abc123; path=/';
    TestBed.configureTestingModule({
      providers: [
        provideHttpClient(withInterceptors([csrfInterceptor])),
        provideHttpClientTesting(),
      ],
    });
    httpClient = TestBed.inject(HttpClient);
    httpTesting = TestBed.inject(HttpTestingController);
  });

  afterEach(() => {
    httpTesting.verify();
    document.cookie = 'sintade_csrf=; path=/; expires=Thu, 01 Jan 1970 00:00:00 GMT';
  });

  it('adds X-CSRF-Token from the cookie on a POST request', () => {
    httpClient.post('/api/v1/auth/refresh', {}).subscribe();
    const request = httpTesting.expectOne('/api/v1/auth/refresh');
    expect(request.request.headers.get('X-CSRF-Token')).toBe('abc123');
    request.flush({});
  });

  it('does not add the header on a GET request', () => {
    httpClient.get('/api/v1/me').subscribe();
    const request = httpTesting.expectOne('/api/v1/me');
    expect(request.request.headers.has('X-CSRF-Token')).toBe(false);
    request.flush({});
  });
});
