import { ChangeDetectionStrategy, Component, inject, signal } from '@angular/core';
import { FormBuilder, ReactiveFormsModule, Validators } from '@angular/forms';
import { Router, RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth.service';
import { extractErrorMessage } from '../../core/http-error';

/** Mirrors `DisplayName::MAX_CHARS` in crates/identity (the server is the authority). */
const DISPLAY_NAME_MAX_CHARS = 80;

/** Profile settings (Day 19): rename yourself, or log out of every device. */
@Component({
  selector: 'app-profile-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [ReactiveFormsModule, RouterLink],
  template: `
    <h1 i18n>Profile</h1>
    @if (authService.currentUser(); as me) {
      <p data-testid="profile-email">{{ me.user.email }}</p>
    }
    <form [formGroup]="form" (ngSubmit)="save()">
      <label>
        <span i18n>Display name</span>
        <input
          type="text"
          formControlName="displayName"
          [attr.maxlength]="maxChars"
          autocomplete="name"
          data-testid="profile-display-name"
        />
      </label>
      @if (error()) {
        <p role="alert" data-testid="profile-error">{{ error() }}</p>
      }
      @if (saved()) {
        <p role="status" data-testid="profile-saved" i18n>Saved.</p>
      }
      <button type="submit" [disabled]="form.invalid || busy()" data-testid="profile-save" i18n>
        Save
      </button>
    </form>

    <h2 i18n>Sessions</h2>
    <p i18n>
      Log out on every device where you are signed in, including this one. Other devices lose access
      within 15 minutes.
    </p>
    <button
      type="button"
      (click)="logoutEverywhere()"
      [disabled]="busy()"
      data-testid="profile-logout-all"
      i18n
    >
      Log out everywhere
    </button>
    <p><a routerLink="/home" i18n>Back</a></p>
  `,
})
export class ProfilePage {
  protected readonly authService = inject(AuthService);
  private readonly formBuilder = inject(FormBuilder);
  private readonly router = inject(Router);

  protected readonly maxChars = DISPLAY_NAME_MAX_CHARS;

  readonly form = this.formBuilder.nonNullable.group({
    displayName: [
      this.authService.currentUser()?.user.display_name ?? '',
      [Validators.required, Validators.maxLength(DISPLAY_NAME_MAX_CHARS)],
    ],
  });

  readonly busy = signal(false);
  readonly saved = signal(false);
  readonly error = signal<string | null>(null);

  save(): void {
    if (this.form.invalid) {
      return;
    }
    this.start();
    this.authService.updateProfile(this.form.getRawValue().displayName).subscribe({
      next: (user) => {
        this.busy.set(false);
        this.saved.set(true);
        this.form.setValue({ displayName: user.display_name });
      },
      error: (error: unknown) => this.fail(error),
    });
  }

  logoutEverywhere(): void {
    this.start();
    this.authService.logoutAll().subscribe({
      next: () => {
        this.busy.set(false);
        void this.router.navigateByUrl('/login');
      },
      error: (error: unknown) => this.fail(error),
    });
  }

  private start(): void {
    this.busy.set(true);
    this.saved.set(false);
    this.error.set(null);
  }

  private fail(error: unknown): void {
    this.busy.set(false);
    this.error.set(extractErrorMessage(error));
  }
}
