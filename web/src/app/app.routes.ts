import { Routes } from '@angular/router';

import { authGuard } from './core/auth.guard';

export const routes: Routes = [
  { path: '', redirectTo: 'debug', pathMatch: 'full' },
  {
    path: 'debug',
    loadComponent: () => import('./feature/debug/debug-page').then((m) => m.DebugPage),
  },
  {
    path: 'signup',
    loadComponent: () => import('./feature/auth/signup-page').then((m) => m.SignupPage),
  },
  {
    path: 'login',
    loadComponent: () => import('./feature/auth/login-page').then((m) => m.LoginPage),
  },
  {
    path: 'verify-email',
    loadComponent: () => import('./feature/auth/verify-email-page').then((m) => m.VerifyEmailPage),
  },
  {
    path: 'forgot-password',
    loadComponent: () =>
      import('./feature/auth/forgot-password-page').then((m) => m.ForgotPasswordPage),
  },
  {
    path: 'reset-password',
    loadComponent: () =>
      import('./feature/auth/reset-password-page').then((m) => m.ResetPasswordPage),
  },
  {
    path: 'home',
    canActivate: [authGuard],
    loadComponent: () => import('./feature/home/home-page').then((m) => m.HomePage),
  },
  {
    path: 'record',
    canActivate: [authGuard],
    loadComponent: () => import('./feature/recorder/recorder-page').then((m) => m.RecorderPage),
  },
  {
    path: 'settings/profile',
    canActivate: [authGuard],
    loadComponent: () => import('./feature/settings/profile-page').then((m) => m.ProfilePage),
  },
];
