import { CaptureError } from '../../capture';
import { BrowserInfo } from '../../core/browser';

export type CaptureSource = 'screen' | 'mic';

export interface CaptureProblem {
  source: CaptureSource;
  title: string;
  steps: string[];
}

/**
 * Turns a capture failure into something a person can act on (US-10: "Denied permissions show
 * recovery instructions per browser, not a blank screen"): a title plus concrete steps named
 * after this browser's own UI.
 */
export function captureHelp(
  error: CaptureError,
  source: CaptureSource,
  browser: BrowserInfo,
): CaptureProblem {
  switch (error.kind) {
    case 'permission-denied':
      return source === 'mic'
        ? {
            source,
            title: $localize`Microphone access is blocked`,
            steps: micPermissionSteps(browser),
          }
        : {
            source,
            title: $localize`Screen sharing was cancelled or blocked`,
            steps: screenPermissionSteps(browser),
          };
    case 'no-device':
      return {
        source,
        title:
          source === 'mic' ? $localize`No microphone found` : $localize`Nothing available to share`,
        steps: [
          source === 'mic'
            ? $localize`Connect a microphone (or headset), then choose it again.`
            : $localize`Open the window or tab you want to record, then choose it again.`,
          $localize`You can also record without a microphone.`,
        ],
      };
    case 'device-busy':
      return {
        source,
        title: $localize`The device is in use`,
        steps: [
          $localize`Close other apps that may be using it (video calls, other recorders).`,
          $localize`Then try again.`,
        ],
      };
    case 'not-supported':
      return {
        source,
        title: $localize`This browser can't record`,
        steps: [$localize`Use a recent desktop version of Chrome, Edge, Firefox or Safari.`],
      };
    case 'aborted':
      return {
        source,
        title: $localize`The shared screen went away`,
        steps: [$localize`Choose what to share again.`],
      };
    default:
      return {
        source,
        title: $localize`Something went wrong`,
        steps: [$localize`Try again. If it keeps happening, reload the page.`],
      };
  }
}

function micPermissionSteps(browser: BrowserInfo): string[] {
  switch (browser.engine) {
    case 'chromium':
      return [
        $localize`Click the site settings icon at the left of the address bar.`,
        $localize`Set Microphone to Allow.`,
        $localize`Then choose your microphone again.`,
      ];
    case 'firefox':
      return [
        $localize`Click the crossed-out microphone icon in the address bar.`,
        $localize`Remove the "Blocked" permission for the microphone.`,
        $localize`Then choose your microphone again.`,
      ];
    case 'safari':
      return [
        $localize`In the menu bar, choose Safari → Settings for This Website.`,
        $localize`Set Microphone to Allow.`,
        $localize`Then choose your microphone again.`,
      ];
    default:
      return [
        $localize`Allow microphone access for this site in your browser's settings.`,
        $localize`Then choose your microphone again.`,
      ];
  }
}

function screenPermissionSteps(browser: BrowserInfo): string[] {
  const steps = [$localize`If you closed the picker, choose what to share again and press Share.`];
  if (browser.os === 'mac') {
    steps.push(
      browser.engine === 'safari'
        ? $localize`If it keeps failing, open System Settings → Privacy & Security → Screen & System Audio Recording and allow Safari.`
        : $localize`If it keeps failing, open System Settings → Privacy & Security → Screen & System Audio Recording, allow your browser, then restart it.`,
    );
  } else if (browser.engine === 'chromium') {
    steps.push(
      $localize`If it keeps failing, check the site settings icon at the left of the address bar and allow screen sharing.`,
    );
  } else if (browser.engine === 'firefox') {
    steps.push(
      $localize`If it keeps failing, click the screen-sharing icon in the address bar and remove the block.`,
    );
  }
  return steps;
}
