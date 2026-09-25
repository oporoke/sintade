import { provideHttpClient, withInterceptors } from '@angular/common/http';
import { ApplicationConfig, provideBrowserGlobalErrorListeners } from '@angular/core';
import { provideRouter } from '@angular/router';

import { csrfInterceptor } from './core/csrf.interceptor';
import { errorInterceptor } from './core/error.interceptor';
import { silentRefreshInterceptor } from './core/silent-refresh.interceptor';
import { routes } from './app.routes';

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideRouter(routes),
    provideHttpClient(
      withInterceptors([csrfInterceptor, errorInterceptor, silentRefreshInterceptor]),
    ),
  ],
};
