import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { ActivatedRoute, RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth.service';
import { extractErrorMessage } from '../../core/http-error';

@Component({
  selector: 'app-reset-password-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ReactiveFormsModule, RouterLink],
  template: `
    <h1 i18n>Choose a new password</h1>
    @if (!token()) {
      <p role="alert" data-testid="reset-password-missing-token" i18n>
        Missing reset token. Use the link from your email.
      </p>
    } @else if (submitted()) {
      <p data-testid="reset-password-success" i18n>
        Your password has been reset.
      </p>
      <p>
        <a routerLink="/login" i18n>Go to login</a>
      </p>
    } @else {
      <form [formGroup]="form" (ngSubmit)="submit()">
        <label>
          <span i18n>New password</span>
          <input type="password" formControlName="password" data-testid="reset-password-password" />
        </label>
        @if (error()) {
          <p role="alert" data-testid="reset-password-error">{{ error() }}</p>
        }
        <button
          type="submit"
          [disabled]="form.invalid || submitting()"
          data-testid="reset-password-submit"
          i18n
        >
          Reset password
        </button>
      </form>
    }
  `,
})
export class ResetPasswordPage implements OnInit {
  private readonly authService = inject(AuthService);
  private readonly formBuilder = inject(FormBuilder);
  private readonly route = inject(ActivatedRoute);

  readonly form = this.formBuilder.nonNullable.group({
    password: ['', [Validators.required, Validators.minLength(10)]],
  });

  readonly token = signal<string | null>(null);
  readonly submitting = signal(false);
  readonly submitted = signal(false);
  readonly error = signal<string | null>(null);

  ngOnInit(): void {
    this.token.set(this.route.snapshot.queryParamMap.get('token'));
  }

  submit(): void {
    const token = this.token();
    if (this.form.invalid || !token) {
      return;
    }
    this.submitting.set(true);
    this.error.set(null);
    const { password } = this.form.getRawValue();
    this.authService.resetPassword(token, password).subscribe({
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
