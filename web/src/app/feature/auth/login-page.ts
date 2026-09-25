import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth.service';
import { extractErrorMessage } from '../../core/http-error';

@Component({
  selector: 'app-login-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ReactiveFormsModule, RouterLink],
  template: `
    <h1 i18n>Log in</h1>
    <form [formGroup]="form" (ngSubmit)="submit()">
      <label>
        <span i18n>Email</span>
        <input type="email" formControlName="email" data-testid="login-email" />
      </label>
      <label>
        <span i18n>Password</span>
        <input type="password" formControlName="password" data-testid="login-password" />
      </label>
      @if (error()) {
        <p role="alert" data-testid="login-error">{{ error() }}</p>
      }
      <button
        type="submit"
        [disabled]="form.invalid || submitting()"
        data-testid="login-submit"
        i18n
      >
        Log in
      </button>
    </form>
    <p>
      <a routerLink="/signup" i18n>Need an account? Sign up</a>
    </p>
    <p>
      <a routerLink="/forgot-password" i18n>Forgot your password?</a>
    </p>
  `,
})
export class LoginPage {
  private readonly authService = inject(AuthService);
  private readonly formBuilder = inject(FormBuilder);
  private readonly router = inject(Router);

  readonly form = this.formBuilder.nonNullable.group({
    email: ['', [Validators.required, Validators.email]],
    password: ['', Validators.required],
  });

  readonly submitting = signal(false);
  readonly error = signal<string | null>(null);

  submit(): void {
    if (this.form.invalid) {
      return;
    }
    this.submitting.set(true);
    this.error.set(null);
    const { email, password } = this.form.getRawValue();
    this.authService.login(email, password).subscribe({
      next: () => {
        this.submitting.set(false);
        void this.router.navigateByUrl('/home');
      },
      error: (error: unknown) => {
        this.submitting.set(false);
        this.error.set(extractErrorMessage(error));
      },
    });
  }
}
