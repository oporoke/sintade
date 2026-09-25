import { Injectable, inject, signal } from '@angular/core';
import { Observable, switchMap, tap } from 'rxjs';

import { ApiClient } from './api-client.service';
import {
  ForgotPasswordBody,
  LoginBody,
  MeResponse,
  MeUser,
  MessageResponse,
  RegisterBody,
  ResetPasswordBody,
  UpdateProfileBody,
  VerifyEmailBody,
} from './auth.models';

@Injectable({ providedIn: 'root' })
export class AuthService {
  private readonly api = inject(ApiClient);

  /** `null` means "not logged in (or not checked yet)" — see `fetchMe()`. */
  readonly currentUser = signal<MeResponse | null>(null);

  register(email: string, password: string, displayName: string): Observable<MessageResponse> {
    const body: RegisterBody = { email, password, display_name: displayName };
    return this.api.post<MessageResponse>('/auth/register', body);
  }

  /** Chains into `fetchMe()` so `currentUser` is populated before this observable completes --
   * callers navigating on success (e.g. to a guarded route) won't race the guard's own check. */
  login(email: string, password: string): Observable<MeResponse> {
    return this.api
      .post<MessageResponse>('/auth/login', { email, password } satisfies LoginBody)
      .pipe(switchMap(() => this.fetchMe()));
  }

  logout(): Observable<void> {
    return this.api.post<void>('/auth/logout', {}).pipe(tap(() => this.currentUser.set(null)));
  }

  /** Silent refresh (Day 17): exchanges a still-valid refresh cookie for a new access cookie. */
  refresh(): Observable<MessageResponse> {
    return this.api.post<MessageResponse>('/auth/refresh', {});
  }

  verifyEmail(token: string): Observable<MessageResponse> {
    return this.api.post<MessageResponse>('/auth/verify-email', {
      token,
    } satisfies VerifyEmailBody);
  }

  forgotPassword(email: string): Observable<MessageResponse> {
    return this.api.post<MessageResponse>('/auth/password/forgot', {
      email,
    } satisfies ForgotPasswordBody);
  }

  resetPassword(token: string, password: string): Observable<MessageResponse> {
    return this.api.post<MessageResponse>('/auth/password/reset', {
      token,
      password,
    } satisfies ResetPasswordBody);
  }

  /** Profile settings (Day 19): renames the user and patches `currentUser` in place. */
  updateProfile(displayName: string): Observable<MeUser> {
    const body: UpdateProfileBody = { display_name: displayName };
    return this.api
      .patch<MeUser>('/me', body)
      .pipe(tap((user) => this.currentUser.update((me) => (me ? { ...me, user } : me))));
  }

  /** Revokes every session of this user on every device, then forgets the local user. */
  logoutAll(): Observable<void> {
    return this.api.post<void>('/auth/logout-all', {}).pipe(tap(() => this.currentUser.set(null)));
  }

  fetchMe(): Observable<MeResponse> {
    return this.api.get<MeResponse>('/me').pipe(tap((me) => this.currentUser.set(me)));
  }
}
