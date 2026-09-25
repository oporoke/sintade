import { provideRouter } from '@angular/router';
import { TestBed } from '@angular/core/testing';
import { of, throwError } from 'rxjs';

import { AuthService } from '../../core/auth.service';
import { SignupPage } from './signup-page';

describe('SignupPage', () => {
  function setup(authServiceStub: Partial<AuthService>) {
    TestBed.configureTestingModule({
      imports: [SignupPage],
      providers: [provideRouter([]), { provide: AuthService, useValue: authServiceStub }],
    });
    const fixture = TestBed.createComponent(SignupPage);
    fixture.detectChanges();
    return fixture;
  }

  it('shows a generic success message after a valid submission', () => {
    const register = vi.fn().mockReturnValue(of({ message: 'ok' }));
    const fixture = setup({ register });

    const component = fixture.componentInstance;
    component.form.setValue({
      email: 'new@example.com',
      displayName: 'New User',
      password: 'correct-horse-battery-staple-42',
    });
    component.submit();
    fixture.detectChanges();

    expect(register).toHaveBeenCalledWith(
      'new@example.com',
      'correct-horse-battery-staple-42',
      'New User',
    );
    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="signup-success"]')).not.toBeNull();
  });

  it('shows the API error message when registration fails', () => {
    const register = vi.fn().mockReturnValue(throwError(() => new Error('boom')));
    const fixture = setup({ register });

    const component = fixture.componentInstance;
    component.form.setValue({
      email: 'new@example.com',
      displayName: 'New User',
      password: 'correct-horse-battery-staple-42',
    });
    component.submit();
    fixture.detectChanges();

    const element: HTMLElement = fixture.nativeElement;
    expect(element.querySelector('[data-testid="signup-error"]')).not.toBeNull();
    expect(element.querySelector('[data-testid="signup-success"]')).toBeNull();
  });

  it('does not submit an invalid form', () => {
    const register = vi.fn();
    const fixture = setup({ register });

    fixture.componentInstance.submit();

    expect(register).not.toHaveBeenCalled();
  });
});
