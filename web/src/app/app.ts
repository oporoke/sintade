import { ChangeDetectionStrategy, Component } from '@angular/core';
import { RouterOutlet } from '@angular/router';

import { RecoveryDialog } from './feature/recorder/recovery-dialog';

@Component({
  imports: [RouterOutlet, RecoveryDialog],
  selector: 'app-root',
  changeDetection: ChangeDetectionStrategy.OnPush,
  styleUrl: './app.scss',
  templateUrl: './app.html',
})
export class App {}
