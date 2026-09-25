import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

// Same-origin, relative path (ADR-0006): in dev, `ng serve` (HTTPS) proxies /api to the API on
// :8080 via proxy.conf.json, so the browser only ever talks to https://localhost:4200 and the
// API's `Secure` cookies are accepted by every engine, WebKit included.
// TODO: Verify — production topology (same-origin reverse proxy vs a separate API host) is not
// decided yet; revisit when a deploy day lands.
const API_BASE_URL = '/api/v1';

@Injectable({ providedIn: 'root' })
export class ApiClient {
  private readonly http = inject(HttpClient);

  get<T>(path: string): Observable<T> {
    return this.http.get<T>(`${API_BASE_URL}${path}`, { withCredentials: true });
  }

  post<T>(path: string, body: unknown): Observable<T> {
    return this.http.post<T>(`${API_BASE_URL}${path}`, body, { withCredentials: true });
  }

  put<T>(path: string, body: unknown): Observable<T> {
    return this.http.put<T>(`${API_BASE_URL}${path}`, body, { withCredentials: true });
  }

  delete<T>(path: string): Observable<T> {
    return this.http.delete<T>(`${API_BASE_URL}${path}`, { withCredentials: true });
  }
}
