import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import { signal } from '@angular/core';
import { of } from 'rxjs';

import { AuthService } from '../../core/auth.service';
import { MeResponse } from '../../core/auth.models';
import { HomePage } from './home-page';

describe('HomePage', () => {
  const me: MeResponse = {
    user: { id: '1', email: 'a@example.com', display_name: 'A', email_verified: true },
    workspaces: [{ id: 'w1', name: "A's workspace", role: 'owner', is_personal: true }],
    current_workspace_id: 'w1',
  };

  it('shows the current user email', () => {
    TestBed.configureTestingModule({
      imports: [HomePage],
      providers: [
        provideRouter([]),
        { provide: AuthService, useValue: { currentUser: signal(me) } },
      ],
    });
    const fixture = TestBed.createComponent(HomePage);
    fixture.detectChanges();

    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="home-user-email"]')?.textContent?.trim()).toBe(
      'a@example.com',
    );
  });

  it('logs out and navigates to /login', () => {
    const logout = vi.fn().mockReturnValue(of(undefined));
    TestBed.configureTestingModule({
      imports: [HomePage],
      providers: [
        provideRouter([]),
        { provide: AuthService, useValue: { currentUser: signal(me), logout } },
      ],
    });
    const fixture = TestBed.createComponent(HomePage);
    fixture.detectChanges();
    const router = TestBed.inject(Router);
    const navigateSpy = vi.spyOn(router, 'navigateByUrl');

    fixture.componentInstance.logout();

    expect(logout).toHaveBeenCalled();
    expect(navigateSpy).toHaveBeenCalledWith('/login');
  });
});
