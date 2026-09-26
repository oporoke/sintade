import { InjectionToken } from '@angular/core';

import { SourceManager } from '../capture';

/**
 * Angular's handle on the framework-free `SourceManager`. The capture package itself knows
 * nothing about DI; pages inject this so tests can substitute a fake.
 */
export const SOURCE_MANAGER = new InjectionToken<SourceManager>('SourceManager', {
  factory: () => new SourceManager(),
});
