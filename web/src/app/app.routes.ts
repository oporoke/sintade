import { Routes } from '@angular/router';

export const routes: Routes = [
  { path: '', redirectTo: 'debug', pathMatch: 'full' },
  {
    path: 'debug',
    loadComponent: () => import('./feature/debug/debug-page').then((m) => m.DebugPage),
  },
];
