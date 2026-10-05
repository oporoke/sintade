import { signUpAndLogIn } from './support/auth';
import { keepTakesOnDevice } from './support/offline';
import { expect, test } from './support/test';

import type { Page } from '@playwright/test';

type Point = [number, number]; // fractions of the frame
type Rgb = number[];

async function magentaCamera(page: Page) {
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
}

async function openRecorderWithCamera(page: Page, request: Parameters<typeof signUpAndLogIn>[1]) {
  await magentaCamera(page);
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
}

async function stopAndGetTake(page: Page): Promise<string | null> {
  await page.getByTestId('recorder-stop').click();
  const summary = page.getByTestId('recorder-done-summary');
  await expect(summary).toBeVisible({ timeout: 10_000 });
  return summary.getAttribute('data-take-id');
}

/** Decodes the stored take and samples the frame at each time at each point. */
async function sample(page: Page, takeId: string | null, times: number[], points: Point[]) {
  return page.evaluate(
    async ({ id, times, points }) => {
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
      const canvas = document.createElement('canvas');
      canvas.width = video.videoWidth;
      canvas.height = video.videoHeight;
      const ctx = canvas.getContext('2d');
      if (!ctx) throw new Error('no 2d context');
      const frames: number[][][] = [];
      for (const t of times) {
        video.currentTime = t;
        await new Promise<void>((resolve) => (video.onseeked = () => resolve()));
        ctx.drawImage(video, 0, 0);
        frames.push(
          points.map(([fx, fy]) =>
            Array.from(
              ctx.getImageData(Math.round(fx * canvas.width), Math.round(fy * canvas.height), 1, 1)
                .data,
            ),
          ),
        );
      }
      return { frames, duration: video.duration, width: canvas.width, height: canvas.height };
    },
    { id: takeId, times, points },
  );
}

const isMagenta = ([r, g, b]: Rgb) => r > 200 && g < 80 && b > 200;

/**
 * Day 71 Check: "bubble appears in the output video". The camera is replaced by a solid magenta
 * canvas stream so the bubble is unmistakable; the take is recorded through the real recorder
 * page, its chunks are read back from OPFS and a frame is decoded and sampled: magenta at the
 * default bubble's centre, not magenta in the bubble square's corner (the bubble is a circle).
 */
test('the webcam bubble appears in the recorded video', async ({ page, request, browserName }) => {
  test.skip(browserName !== 'chromium', 'MediaRecorder + canvas capture checked on Chromium');
  await openRecorderWithCamera(page, request);
  await page.waitForTimeout(4000);
  const takeId = await stopAndGetTake(page);
  // Default bubble: centre (0.88, 0.82), diameter 0.24 of the height; 16:9 frame.
  const side = 0.24;
  const { frames } = await sample(
    page,
    takeId,
    [1],
    [
      [0.88, 0.82],
      [0.88 - (side * 0.46 * 9) / 16, 0.82 - side * 0.46],
    ],
  );
  expect(isMagenta(frames[0][0])).toBe(true);
  expect(isMagenta(frames[0][1])).toBe(false);
});

/**
 * Day 72 Check: "position changes are recorded live". The bubble is dragged on the pad while
 * recording; an early frame has it at the default spot, a late frame at the new one.
 */
test('moving the bubble while recording moves it in the video', async ({
  page,
  request,
  browserName,
}) => {
  test.skip(browserName !== 'chromium', 'MediaRecorder + canvas capture checked on Chromium');
  await openRecorderWithCamera(page, request);
  await page.waitForTimeout(2500);
  const pad = page.getByTestId('recorder-bubble-pad');
  await expect(page.getByTestId('recorder-bubble-dot')).toBeVisible();
  const box = await pad.boundingBox();
  if (!box) throw new Error('no bubble pad');
  await page.mouse.move(box.x + box.width * 0.5, box.y + box.height * 0.5);
  await page.mouse.down();
  await page.mouse.move(box.x + box.width * 0.15, box.y + box.height * 0.2, { steps: 5 });
  await page.mouse.up();
  await page.waitForTimeout(5000);
  const takeId = await stopAndGetTake(page);
  const old: Point = [0.88, 0.82];
  const moved: Point = [0.15, 0.2];
  const { frames } = await sample(page, takeId, [1], [old, moved]);
  expect(isMagenta(frames[0][0])).toBe(true); // early: at the default spot
  expect(isMagenta(frames[0][1])).toBe(false);
  const late = await sample(page, takeId, [6], [old, moved]);
  expect(isMagenta(late.frames[0][1])).toBe(true); // late: at the dragged spot
  expect(isMagenta(late.frames[0][0])).toBe(false);
});

/** Camera-only: the whole frame is the camera. */
test('camera-only fills the frame with the camera', async ({ page, request, browserName }) => {
  test.skip(browserName !== 'chromium', 'MediaRecorder + canvas capture checked on Chromium');
  await openRecorderWithCamera(page, request);
  await page.getByTestId('recorder-camera-only').check();
  await page.waitForTimeout(3500);
  const takeId = await stopAndGetTake(page);
  const { frames } = await sample(
    page,
    takeId,
    [2],
    [
      [0.1, 0.1],
      [0.5, 0.5],
      [0.9, 0.9],
    ],
  );
  expect(frames[0].every(isMagenta)).toBe(true);
});
