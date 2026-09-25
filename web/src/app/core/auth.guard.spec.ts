import { TestBed } from '@angular/core/testing';
import { UrlTree, provideRouter } from '@angular/router';
import { signal } from '@angular/core';
import { Observable, firstValueFrom, isObservable, of, throwError } from 'rxjs';

import { authGuard } from './auth.guard';
import { AuthService } from './auth.service';
import { MeResponse } from './auth.models';

describe('authGuard', () => {
  const me: MeResponse = {
    user: { id: '1', email: 'a@example.com', display_name: 'A', email_verified: true },
    workspaces: [],
    current_workspace_id: 'w1',
  };

  function run(authServiceStub: Partial<AuthService>) {
    TestBed.configureTestingModule({
      providers: [provideRouter([]), { provide: AuthService, useValue: authServiceStub }],
    });
    return TestBed.runInInjectionContext(() =>
      // eslint-disable-next-line @typescript-eslint/no-explicit-any
      authGuard({} as any, { url: '/home' } as any),
    );
  }

  it('allows navigation when a user is already known', async () => {
    const result = run({ currentUser: signal(me) });
    expect(await toPromise(result)).toBe(true);
  });

  it('allows navigation when /me resolves', async () => {
    const result = run({ currentUser: signal(null), fetchMe: () => of(me) });
    expect(await toPromise(result)).toBe(true);
  });

  it('redirects to /login when /me fails', async () => {
    const result = run({
      currentUser: signal(null),
      fetchMe: () => throwError(() => new Error('unauthorized')),
    });
    const value = await toPromise(result);
    expect(value).toBeInstanceOf(UrlTree);
    expect((value as UrlTree).toString()).toBe('/login');
  });

  async function toPromise(value: unknown): Promise<boolean | UrlTree> {
    if (isObservable(value)) {
      return firstValueFrom(value as Observable<boolean | UrlTree>);
    }
    return value as boolean | UrlTree;
  }
});
