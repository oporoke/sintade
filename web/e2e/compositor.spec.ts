import { signUpAndLogIn } from './support/auth';
import { keepTakesOnDevice } from './support/offline';
import { expect, test } from './support/test';

/**
 * Day 71 Check: "bubble appears in the output video". The camera is replaced by a solid magenta
 * canvas stream so the bubble is unmistakable; the take is recorded through the real recorder
 * page, its chunks are read back from OPFS and a frame is decoded and sampled: magenta at the
 * default bubble's centre, not magenta in the bubble square's corner (the bubble is a circle).
 */
test('the webcam bubble appears in the recorded video', async ({ page, request, browserName }) => {
  test.skip(browserName !== 'chromium', 'MediaRecorder + canvas capture checked on Chromium');
  await page.addInitScript(() => {
    const original = navigator.mediaDevices.getUserMedia.bind(navigator.mediaDevices);
    navigator.mediaDevices.getUserMedia = async (constraints) => {
      if (constraints && constraints.video) {
        const canvas = document.createElement('canvas');
        canvas.width = 640;
        canvas.height = 480;
        const ctx = canvas.getContext('2d');
        const paint = () => {
          if (ctx) {
            ctx.fillStyle = '#ff00ff';
            ctx.fillRect(0, 0, 640, 480);
          }
        };
        paint();
        setInterval(paint, 50);
        return canvas.captureStream(20);
      }
      return original(constraints);
    };
  });
  await signUpAndLogIn(page, request);
  await keepTakesOnDevice(page);
  await page.goto('/record');
  await page.getByTestId('recorder-choose-screen').click();
  await expect(page.getByTestId('recorder-screen-preview')).toBeVisible();
  await page.getByTestId('recorder-camera').check();
  await expect(page.getByTestId('recorder-camera-preview')).toBeVisible();
  await page.getByTestId('recorder-start').click();
  await page.keyboard.press('Escape');
  await expect(page.getByTestId('recorder-status')).toHaveText('Recording');
  await page.waitForTimeout(4000);
  await page.getByTestId('recorder-stop').click();
  const summary = page.getByTestId('recorder-done-summary');
  await expect(summary).toBeVisible({ timeout: 10_000 });
  const takeId = await summary.getAttribute('data-take-id');

  const pixels = await page.evaluate(async (id) => {
    const root = await navigator.storage.getDirectory();
    const take = await (
      await root.getDirectoryHandle('sintade-chunks')
    ).getDirectoryHandle(id ?? '');
    const parts: { name: string; blob: Blob }[] = [];
    let type = 'video/webm';
    for await (const [name, handle] of take.entries()) {
      if (handle.kind !== 'file') continue;
      const file = await (handle as FileSystemFileHandle).getFile();
      if (name === 'take.json') type = JSON.parse(await file.text()).mimeType;
      else parts.push({ name, blob: file });
    }
    parts.sort((a, b) => a.name.localeCompare(b.name));
    const video = document.createElement('video');
    video.muted = true;
    video.src = URL.createObjectURL(
      new Blob(
        parts.map((p) => p.blob),
        { type },
      ),
    );
    await new Promise<void>((resolve, reject) => {
      video.onloadeddata = () => resolve();
      video.onerror = () => reject(new Error('cannot decode the take'));
    });
    video.currentTime = 1;
    await new Promise<void>((resolve) => (video.onseeked = () => resolve()));
    const canvas = document.createElement('canvas');
    canvas.width = video.videoWidth;
    canvas.height = video.videoHeight;
    const ctx = canvas.getContext('2d');
    if (!ctx) throw new Error('no 2d context');
    ctx.drawImage(video, 0, 0);
    const w = canvas.width;
    const h = canvas.height;
    // Default bubble: centre (0.88, 0.82), diameter 0.24 of the height.
    const side = 0.24 * h;
    const cx = 0.88 * w;
    const cy = 0.82 * h;
    const at = (x: number, y: number) =>
      Array.from(ctx.getImageData(Math.round(x), Math.round(y), 1, 1).data);
    return {
      centre: at(cx, cy),
      corner: at(cx - side * 0.46, cy - side * 0.46),
    };
  }, takeId);

  const [r, g, b] = pixels.centre;
  expect(r).toBeGreaterThan(200);
  expect(g).toBeLessThan(80);
  expect(b).toBeGreaterThan(200);
  const [cr, cg, cb] = pixels.corner;
  expect(cr > 200 && cg < 80 && cb > 200).toBe(false);
});
