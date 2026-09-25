import { signal } from '@angular/core';
import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import { of, throwError } from 'rxjs';

import { MeResponse } from '../../core/auth.models';
import { AuthService } from '../../core/auth.service';
import { ProfilePage } from './profile-page';

describe('ProfilePage', () => {
  const me: MeResponse = {
    user: { id: '1', email: 'a@example.com', display_name: 'Asha', email_verified: true },
    workspaces: [{ id: 'w1', name: "Asha's workspace", role: 'owner', is_personal: true }],
    current_workspace_id: 'w1',
  };

  function setup(stub: Partial<AuthService>) {
    TestBed.configureTestingModule({
      imports: [ProfilePage],
      providers: [
        provideRouter([]),
        { provide: AuthService, useValue: { currentUser: signal(me), ...stub } },
      ],
    });
    const fixture = TestBed.createComponent(ProfilePage);
    fixture.detectChanges();
    return fixture;
  }

  it('prefills the current display name', () => {
    const fixture = setup({});
    expect(fixture.componentInstance.form.getRawValue().displayName).toBe('Asha');
  });

  it('saves a new display name and shows confirmation', () => {
    const updateProfile = vi.fn().mockReturnValue(of({ ...me.user, display_name: 'Asha M.' }));
    const fixture = setup({ updateProfile });

    fixture.componentInstance.form.setValue({ displayName: 'Asha M.' });
    fixture.componentInstance.save();
    fixture.detectChanges();

    expect(updateProfile).toHaveBeenCalledWith('Asha M.');
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="profile-saved"]')).not.toBeNull();
  });

  it('does not submit a blank name', () => {
    const updateProfile = vi.fn();
    const fixture = setup({ updateProfile });

    fixture.componentInstance.form.setValue({ displayName: '' });
    fixture.componentInstance.save();

    expect(updateProfile).not.toHaveBeenCalled();
  });

  it('shows the server error when saving fails', () => {
    const updateProfile = vi.fn().mockReturnValue(throwError(() => new Error('nope')));
    const fixture = setup({ updateProfile });

    fixture.componentInstance.save();
    fixture.detectChanges();

    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="profile-error"]')).not.toBeNull();
  });

  it('logs out everywhere and navigates to /login', () => {
    const logoutAll = vi.fn().mockReturnValue(of(undefined));
    const fixture = setup({ logoutAll });
    const navigateSpy = vi.spyOn(TestBed.inject(Router), 'navigateByUrl');

    fixture.componentInstance.logoutEverywhere();

    expect(logoutAll).toHaveBeenCalled();
    expect(navigateSpy).toHaveBeenCalledWith('/login');
  });
});
