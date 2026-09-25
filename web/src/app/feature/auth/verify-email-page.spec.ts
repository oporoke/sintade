import { TestBed } from '@angular/core/testing';
import { ActivatedRoute, convertToParamMap, provideRouter } from '@angular/router';
import { of, throwError } from 'rxjs';

import { AuthService } from '../../core/auth.service';
import { VerifyEmailPage } from './verify-email-page';

describe('VerifyEmailPage', () => {
  function setup(authServiceStub: Partial<AuthService>, token: string | null) {
    TestBed.configureTestingModule({
      imports: [VerifyEmailPage],
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
    const fixture = TestBed.createComponent(VerifyEmailPage);
    fixture.detectChanges();
    return fixture;
  }

  it('verifies the token from the query string and shows success', () => {
    const verifyEmail = vi.fn().mockReturnValue(of({ message: 'ok' }));
    const fixture = setup({ verifyEmail }, 'a-real-token');

    expect(verifyEmail).toHaveBeenCalledWith('a-real-token');
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="verify-email-success"]')).not.toBeNull();
  });

  it('shows an error without calling the API when the token is missing', () => {
    const verifyEmail = vi.fn();
    const fixture = setup({ verifyEmail }, null);

    expect(verifyEmail).not.toHaveBeenCalled();
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="verify-email-error"]')).not.toBeNull();
  });

  it('shows the API error when the token is invalid or expired', () => {
    const verifyEmail = vi.fn().mockReturnValue(throwError(() => new Error('expired')));
    const fixture = setup({ verifyEmail }, 'a-stale-token');

    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="verify-email-error"]')).not.toBeNull();
  });
});
