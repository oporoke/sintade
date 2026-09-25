import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth.service';
import { extractErrorMessage } from '../../core/http-error';

@Component({
  selector: 'app-forgot-password-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ReactiveFormsModule, RouterLink],
  template: `
    <h1 i18n>Reset your password</h1>
    @if (submitted()) {
      <p data-testid="forgot-password-success" i18n>
        If that email exists, a reset link has been sent.
      </p>
    } @else {
      <form [formGroup]="form" (ngSubmit)="submit()">
        <label>
          <span i18n>Email</span>
          <input type="email" formControlName="email" data-testid="forgot-password-email" />
        </label>
        @if (error()) {
          <p role="alert" data-testid="forgot-password-error">{{ error() }}</p>
        }
        <button
          type="submit"
          [disabled]="form.invalid || submitting()"
          data-testid="forgot-password-submit"
          i18n
        >
          Send reset link
        </button>
      </form>
      <p>
        <a routerLink="/login" i18n>Back to login</a>
      </p>
    }
  `,
})
export class ForgotPasswordPage {
  private readonly authService = inject(AuthService);
  private readonly formBuilder = inject(FormBuilder);

  readonly form = this.formBuilder.nonNullable.group({
    email: ['', [Validators.required, Validators.email]],
  });

  readonly submitting = signal(false);
  readonly submitted = signal(false);
  readonly error = signal<string | null>(null);

  submit(): void {
    if (this.form.invalid) {
      return;
    }
    this.submitting.set(true);
    this.error.set(null);
    const { email } = this.form.getRawValue();
    this.authService.forgotPassword(email).subscribe({
      next: () => {
        this.submitting.set(false);
        this.submitted.set(true);
      },
      error: (error: unknown) => {
        this.submitting.set(false);
        this.error.set(extractErrorMessage(error));
      },
    });
  }
}
