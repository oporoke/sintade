import { HttpClient } from '@angular/common/http';
import { Injectable, inject } from '@angular/core';
import { Observable } from 'rxjs';

// TODO: Verify — no environment-config day has landed yet; the API origin is hardcoded to the
// local dev backend (docs/design.md's "First run" has the API on :8080).
const API_BASE_URL = 'http://localhost:8080/api/v1';

@Injectable({ providedIn: 'root' })
export class ApiClient {
  private readonly http = inject(HttpClient);

  get<T>(path: string): Observable<T> {
    return this.http.get<T>(`${API_BASE_URL}${path}`);
  }

  post<T>(path: string, body: unknown): Observable<T> {
    return this.http.post<T>(`${API_BASE_URL}${path}`, body);
  }

  put<T>(path: string, body: unknown): Observable<T> {
    return this.http.put<T>(`${API_BASE_URL}${path}`, body);
  }

  delete<T>(path: string): Observable<T> {
    return this.http.delete<T>(`${API_BASE_URL}${path}`);
  }
}
