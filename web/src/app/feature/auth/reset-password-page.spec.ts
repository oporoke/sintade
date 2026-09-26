import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, convertToParamMap, provideRouter } from '@angular/router';
import { of, throwError } from 'rxjs';

import { AuthService } from '../../core/auth.service';
import { ResetPasswordPage } from './reset-password-page';

describe('ResetPasswordPage', () => {
  function setup(authServiceStub: Partial<AuthService>, token: string | null) {
    TestBed.configureTestingModule({
      imports: [ResetPasswordPage],
      providers: [
        provideRouter([]),
        { provide: AuthService, useValue: authServiceStub },
        {
          provide: ActivatedRoute,
          useValue: {
            snapshot: { queryParamMap: convertToParamMap(token ? { token } : {}) },
          },
        },
      ],
    });
    const fixture = TestBed.createComponent(ResetPasswordPage);
    fixture.detectChanges();
    return fixture;
  }

  it('shows a missing-token message without rendering the form', () => {
    const fixture = setup({}, null);
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="reset-password-missing-token"]')).not.toBeNull();
    expect(element.querySelector('[data-testid="reset-password-submit"]')).toBeNull();
  });

  it('resets the password and shows success', () => {
    const resetPassword = vi.fn().mockReturnValue(of({ message: 'ok' }));
    const fixture = setup({ resetPassword }, 'a-real-token');

    fixture.componentInstance.form.setValue({ password: 'a-brand-new-strong-password-77' });
    fixture.componentInstance.submit();
    fixture.detectChanges();

    expect(resetPassword).toHaveBeenCalledWith('a-real-token', 'a-brand-new-strong-password-77');
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="reset-password-success"]')).not.toBeNull();
  });

  it('shows the API error for an invalid or expired token', () => {
    const resetPassword = vi.fn().mockReturnValue(throwError(() => new Error('expired')));
    const fixture = setup({ resetPassword }, 'a-stale-token');

    fixture.componentInstance.form.setValue({ password: 'a-brand-new-strong-password-77' });
    fixture.componentInstance.submit();
    fixture.detectChanges();

    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="reset-password-error"]')).not.toBeNull();
  });
});
