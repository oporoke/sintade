import { TestBed } from '@angular/core/testing';
import { provideRouter } from '@angular/router';
import { of } from 'rxjs';

import { AuthService } from '../../core/auth.service';
import { ForgotPasswordPage } from './forgot-password-page';

describe('ForgotPasswordPage', () => {
  it('shows the same generic message regardless of whether the email exists', () => {
    const forgotPassword = vi.fn().mockReturnValue(of({ message: 'ok' }));
    TestBed.configureTestingModule({
      imports: [ForgotPasswordPage],
      providers: [provideRouter([]), { provide: AuthService, useValue: { forgotPassword } }],
    });
    const fixture = TestBed.createComponent(ForgotPasswordPage);
    fixture.detectChanges();

    fixture.componentInstance.form.setValue({ email: 'someone@example.com' });
    fixture.componentInstance.submit();
    fixture.detectChanges();

    expect(forgotPassword).toHaveBeenCalledWith('someone@example.com');
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="forgot-password-success"]')).not.toBeNull();
  });
});
