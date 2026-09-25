import { TestBed } from '@angular/core/testing';
import { Router, provideRouter } from '@angular/router';
import { of, throwError } from 'rxjs';

import { AuthService } from '../../core/auth.service';
import { LoginPage } from './login-page';

describe('LoginPage', () => {
  function setup(authServiceStub: Partial<AuthService>) {
    TestBed.configureTestingModule({
      imports: [LoginPage],
      providers: [provideRouter([]), { provide: AuthService, useValue: authServiceStub }],
    });
    const fixture = TestBed.createComponent(LoginPage);
    fixture.detectChanges();
    return fixture;
  }

  it('navigates to /home after a successful login', () => {
    const login = vi.fn().mockReturnValue(
      of({
        user: { id: '1', email: 'a@example.com', display_name: 'A', email_verified: true },
        workspaces: [],
        current_workspace_id: 'w1',
      }),
    );
    const fixture = setup({ login });
    const router = TestBed.inject(Router);
    const navigateSpy = vi.spyOn(router, 'navigateByUrl');

    fixture.componentInstance.form.setValue({
      email: 'a@example.com',
      password: 'correct-horse-battery-staple-42',
    });
    fixture.componentInstance.submit();

    expect(login).toHaveBeenCalledWith('a@example.com', 'correct-horse-battery-staple-42');
    expect(navigateSpy).toHaveBeenCalledWith('/home');
  });

  it('shows an error and does not navigate on invalid credentials', () => {
    const login = vi.fn().mockReturnValue(throwError(() => new Error('invalid')));
    const fixture = setup({ login });
    const router = TestBed.inject(Router);
    const navigateSpy = vi.spyOn(router, 'navigateByUrl');

    fixture.componentInstance.form.setValue({
      email: 'a@example.com',
      password: 'wrong-password',
    });
    fixture.componentInstance.submit();
    fixture.detectChanges();

    expect(navigateSpy).not.toHaveBeenCalled();
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="login-error"]')).not.toBeNull();
  });
});
