/** Pages the browser does not let an extension capture or script. */
const BLOCKED_HOSTS = [
  'chromewebstore.google.com',
  'chrome.google.com',
  'microsoftedge.microsoft.com',
];

/**
 * Why a tab can't be recorded, in words for the popup, or `null` if it can: only ordinary web
 * pages (http and https) can, and not the extension stores.
 */
export function unrecordableReason(url: string | undefined): string | null {
  if (!url) {
    return "Sintade can't see this tab. Open a website and try again.";
  }
  let parsed: URL;
  try {
    parsed = new URL(url);
  } catch {
    return "Sintade can't record this page.";
  }
  if (parsed.protocol !== 'http:' && parsed.protocol !== 'https:') {
    return "Browsers don't let extensions record their own pages. Open a website and try again.";
  }
  if (BLOCKED_HOSTS.includes(parsed.hostname)) {
    return "Browsers don't let extensions record their extension store. Open another website.";
  }
  return null;
}
