import { ChangeDetectionStrategy, Component, inject } from '@angular/core';
import { Router, RouterLink } from '@angular/router';

import { AuthService } from '../../core/auth.service';

/**
 * Placeholder protected landing page (Day 17): proves the auth guard and the full
 * signup-to-login flow end to end. Real workspace/recordings content lands in later days.
 */
@Component({
  selector: 'app-home-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [RouterLink],
  template: `
    <h1 i18n>Welcome</h1>
    @if (authService.currentUser(); as me) {
      <p data-testid="home-user-email">{{ me.user.email }}</p>
    }
    <p>
      <a routerLink="/settings/profile" data-testid="home-profile-link" i18n>Profile settings</a>
    </p>
    <button type="button" (click)="logout()" data-testid="home-logout" i18n>Log out</button>
  `,
})
export class HomePage {
  protected readonly authService = inject(AuthService);
  private readonly router = inject(Router);

  logout(): void {
    this.authService.logout().subscribe(() => {
      void this.router.navigateByUrl('/login');
    });
  }
}
