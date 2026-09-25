import { ChangeDetectionStrategy, Component, OnInit, inject, signal } from '@angular/core';
import { ActivatedRoute, RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth.service';
import { extractErrorMessage } from '../../core/http-error';

type VerifyState = 'verifying' | 'success' | 'error';

@Component({
  selector: 'app-verify-email-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterLink],
  template: `
    <h1 i18n>Verify your email</h1>
    @switch (state()) {
      @case ('verifying') {
        <p data-testid="verify-email-verifying" i18n>Verifying...</p>
      }
      @case ('success') {
        <p data-testid="verify-email-success" i18n>
          Your email is verified. You can now log in.
        </p>
        <p>
          <a routerLink="/login" i18n>Go to login</a>
        </p>
      }
      @case ('error') {
        <p role="alert" data-testid="verify-email-error">{{ error() }}</p>
      }
    }
  `,
})
export class VerifyEmailPage implements OnInit {
  private readonly authService = inject(AuthService);
  private readonly route = inject(ActivatedRoute);

  readonly state = signal<VerifyState>('verifying');
  readonly error = signal<string | null>(null);

  ngOnInit(): void {
    const token = this.route.snapshot.queryParamMap.get('token');
    if (!token) {
      this.state.set('error');
      this.error.set($localize`Missing verification token. Use the link from your email.`);
      return;
    }

    this.authService.verifyEmail(token).subscribe({
      next: () => this.state.set('success'),
      error: (error: unknown) => {
        this.state.set('error');
        this.error.set(extractErrorMessage(error));
      },
    });
  }
}
