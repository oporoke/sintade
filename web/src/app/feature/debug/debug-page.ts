import { ChangeDetectionStrategy, Component, computed, inject } from '@angular/core';

import { CapabilityService } from '../../core/capability.service';

interface CapabilityRow {
  name: string;
  supported: boolean;
}

@Component({
  selector: 'app-debug-page',
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <h1>Capability matrix</h1>
    <table data-testid="capability-matrix">
      <thead>
        <tr>
          <th>Capability</th>
          <th>Supported</th>
        </tr>
      </thead>
      <tbody>
        @for (row of rows(); track row.name) {
          <tr [attr.data-testid]="'capability-row-' + row.name">
            <td>{{ row.name }}</td>
            <td [attr.data-testid]="'capability-value-' + row.name">{{ row.supported }}</td>
          </tr>
        }
      </tbody>
    </table>
  `,
})
export class DebugPage {
  private readonly capabilityService = inject(CapabilityService);

  readonly rows = computed<CapabilityRow[]>(() => {
    const capabilities = this.capabilityService.capabilities();
    return [
      { name: 'getDisplayMedia', supported: capabilities.getDisplayMedia },
      { name: 'mediaRecorderWebm', supported: capabilities.mediaRecorderWebm },
      { name: 'opfs', supported: capabilities.opfs },
      { name: 'systemAudio', supported: capabilities.systemAudio },
    ];
  });
}
