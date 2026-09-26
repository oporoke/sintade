import { detectBrowser } from './browser';

const UA = {
  chromeWin:
    'Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
  edgeMac:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36 Edg/140.0.0.0',
  firefoxLinux: 'Mozilla/5.0 (X11; Linux x86_64; rv:142.0) Gecko/20100101 Firefox/142.0',
  safariMac:
    'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Safari/605.1.15',
  iphone:
    'Mozilla/5.0 (iPhone; CPU iPhone OS 18_0 like Mac OS X) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/18.0 Mobile/15E148 Safari/604.1',
  androidChrome:
    'Mozilla/5.0 (Linux; Android 14; Pixel 8) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Mobile Safari/537.36',
  chromebook:
    'Mozilla/5.0 (X11; CrOS x86_64 14541.0.0) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36',
};

describe('detectBrowser', () => {
  it.each([
    ['chromeWin', { engine: 'chromium', os: 'windows', mobile: false }],
    ['edgeMac', { engine: 'chromium', os: 'mac', mobile: false }],
    ['firefoxLinux', { engine: 'firefox', os: 'linux', mobile: false }],
    ['safariMac', { engine: 'safari', os: 'mac', mobile: false }],
    ['iphone', { engine: 'safari', os: 'ios', mobile: true }],
    ['androidChrome', { engine: 'chromium', os: 'android', mobile: true }],
    ['chromebook', { engine: 'chromium', os: 'chromeos', mobile: false }],
  ] as const)('%s', (name, expected) => {
    expect(detectBrowser(UA[name])).toEqual(expected);
  });

  it('treats a touch-screen "Mac" as an iPad (iPadOS reports a desktop UA)', () => {
    expect(detectBrowser(UA.safariMac, 5)).toEqual({ engine: 'safari', os: 'ios', mobile: true });
  });
});
