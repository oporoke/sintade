import { CaptureError } from '../../capture';
import { BrowserInfo } from '../../core/browser';
import { captureHelp } from './capture-help';

const denied = new CaptureError('permission-denied', 'NotAllowedError');
const on = (engine: BrowserInfo['engine'], os: BrowserInfo['os'] = 'windows'): BrowserInfo => ({
  engine,
  os,
  mobile: false,
});

describe('captureHelp', () => {
  it('gives browser-specific steps for a blocked microphone', () => {
    expect(captureHelp(denied, 'mic', on('chromium')).steps[0]).toMatch(/site settings icon/);
    expect(captureHelp(denied, 'mic', on('firefox')).steps[0]).toMatch(/crossed-out microphone/);
    expect(captureHelp(denied, 'mic', on('safari', 'mac')).steps[0]).toMatch(
      /Settings for This Website/,
    );
    expect(captureHelp(denied, 'mic', on('other')).steps[0]).toMatch(/browser's settings/);
  });

  it('points Mac users to the OS Screen Recording permission', () => {
    const help = captureHelp(denied, 'screen', on('chromium', 'mac'));
    expect(help.title).toBe('Screen sharing was cancelled or blocked');
    expect(help.steps.join(' ')).toMatch(/Privacy & Security/);
  });

  it('always has a title and at least one step, for every kind and source', () => {
    const kinds = [
      'permission-denied',
      'no-device',
      'device-busy',
      'not-supported',
      'aborted',
      'unknown',
    ] as const;
    for (const kind of kinds) {
      for (const source of ['screen', 'mic'] as const) {
        for (const engine of ['chromium', 'firefox', 'safari', 'other'] as const) {
          const help = captureHelp(new CaptureError(kind, 'x'), source, on(engine));
          expect(help.title.length, `${kind}/${source}/${engine}`).toBeGreaterThan(0);
          expect(help.steps.length, `${kind}/${source}/${engine}`).toBeGreaterThan(0);
        }
      }
    }
  });
});
