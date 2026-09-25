import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth.service';
import { extractErrorMessage } from '../../core/http-error';

@Component({
  selector: 'app-signup-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ReactiveFormsModule, RouterLink],
  template: `
    <h1 i18n>Create your account</h1>
    @if (submitted()) {
      <p data-testid="signup-success" i18n>
        If your details are valid, a verification email has been sent. Check your inbox, then
        log in.
      </p>
    } @else {
      <form [formGroup]="form" (ngSubmit)="submit()">
        <label>
          <span i18n>Email</span>
          <input type="email" formControlName="email" data-testid="signup-email" />
        </label>
        <label>
          <span i18n>Display name</span>
          <input type="text" formControlName="displayName" data-testid="signup-display-name" />
        </label>
        <label>
          <span i18n>Password</span>
          <input type="password" formControlName="password" data-testid="signup-password" />
        </label>
        @if (error()) {
          <p role="alert" data-testid="signup-error">{{ error() }}</p>
        }
        <button
          type="submit"
          [disabled]="form.invalid || submitting()"
          data-testid="signup-submit"
          i18n
        >
          Sign up
        </button>
      </form>
      <p>
        <a routerLink="/login" i18n>Already have an account? Log in</a>
      </p>
    }
  `,
})
export class SignupPage {
  private readonly authService = inject(AuthService);
  private readonly formBuilder = inject(FormBuilder);

  readonly form = this.formBuilder.nonNullable.group({
    email: ['', [Validators.required, Validators.email]],
    displayName: ['', Validators.required],
    password: ['', [Validators.required, Validators.minLength(10)]],
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
    const { email, displayName, password } = this.form.getRawValue();
    this.authService.register(email, password, displayName).subscribe({
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
