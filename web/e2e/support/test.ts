import { test as base } from '@playwright/test';

export { expect } from '@playwright/test';

/**
 * The fake-media harness (Day 31). Every test importing `test` from here can capture a "screen"
 * and a microphone on all three engines, with no prompts:
 *
 * - Chromium: native fake devices and auto-accepted pickers (launch flags in
 *   playwright.config.ts), so the real getDisplayMedia/getUserMedia paths run.
 * - Firefox: native fake microphone (prefs in playwright.config.ts). Its getDisplayMedia always
 *   waits on a picker, so it's replaced with an animated-canvas stream.
 * - WebKit: the same canvas screen, plus a granted microphone permission (mock capture device).
 *
 * The canvas stream is a real MediaStream, so everything downstream (preview, mixing,
 * MediaRecorder) runs for real. Ending its track is how a test mimics "Stop sharing".
 */
export const test = base.extend<{ fakeMedia: void }>({
  fakeMedia: [
    async ({ page, context, browserName }, use) => {
      if (browserName === 'webkit') {
        await context.grantPermissions(['microphone']);
      }
      if (browserName !== 'chromium') {
        await page.addInitScript(installCanvasScreen);
      }
      await use();
    },
    { auto: true },
  ],
});

/** Runs in the page before app code. Must be self-contained (it's serialised). */
function installCanvasScreen(): void {
  const devices = navigator.mediaDevices;
  if (!devices) {
    return;
  }
  const fakeScreen = async (): Promise<MediaStream> => {
    const canvas = document.createElement('canvas');
    canvas.width = 1280;
    canvas.height = 720;
    const context = canvas.getContext('2d');
    let frame = 0;
    const draw = () => {
      if (!context) {
        return;
      }
      context.fillStyle = '#0b3d2e';
      context.fillRect(0, 0, canvas.width, canvas.height);
      context.fillStyle = '#4cd964';
      context.fillRect((frame * 12) % canvas.width, 300, 120, 120);
      context.fillStyle = '#ffffff';
      context.font = '32px sans-serif';
      context.fillText(`fake screen · frame ${frame}`, 24, 48);
      frame += 1;
      requestAnimationFrame(draw);
    };
    draw();
    return canvas.captureStream(30);
  };
  Object.defineProperty(devices, 'getDisplayMedia', { value: fakeScreen, configurable: true });
}
