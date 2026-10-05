import { keyLabel } from './keys';

export interface OverlayOptions {
  highlights: boolean;
  keystrokes: boolean;
}

/** How long a click ripple and a key label stay on screen. */
export const RIPPLE_MS = 700;
export const KEY_MS = 1500;

const STYLE = `
:host { all: initial; }
.layer { position: fixed; inset: 0; pointer-events: none; z-index: 2147483647; }
.ripple {
  position: fixed; width: 44px; height: 44px; margin: -22px 0 0 -22px; border-radius: 50%;
  background: rgba(255, 196, 0, 0.45); border: 3px solid #ffc400; box-sizing: border-box;
  animation: ripple ${RIPPLE_MS}ms ease-out forwards;
}
@keyframes ripple { from { transform: scale(0.6); opacity: 1; } to { transform: scale(1.6); opacity: 0; } }
.keys {
  position: fixed; left: 50%; bottom: 36px; transform: translateX(-50%); display: flex; gap: 8px;
}
.key {
  font: 600 22px/1 system-ui, sans-serif; color: #fff; background: rgba(10, 10, 10, 0.88);
  padding: 10px 16px; border-radius: 10px; box-shadow: 0 2px 10px rgba(0, 0, 0, 0.4);
  animation: fade ${KEY_MS}ms ease-in forwards;
}
@keyframes fade { 0%, 70% { opacity: 1; } 100% { opacity: 0; } }
`;

/**
 * Draws click highlights and a keystroke overlay over the page. A closed shadow root keeps the
 * page's CSS and scripts away from it, and nothing in it takes pointer events. The tab's capture
 * includes it, so it appears in the recording. Returns a function that removes everything.
 */
export function createOverlay(doc: Document, options: OverlayOptions): () => void {
  const host = doc.createElement('div');
  host.setAttribute('data-sintade-overlay', '');
  const root = host.attachShadow({ mode: 'closed' });
  const style = doc.createElement('style');
  style.textContent = STYLE;
  const layer = doc.createElement('div');
  layer.className = 'layer';
  const keys = doc.createElement('div');
  keys.className = 'keys';
  layer.append(keys);
  root.append(style, layer);
  (doc.documentElement ?? doc.body).append(host);

  const cleanups: (() => void)[] = [];

  if (options.highlights) {
    const onDown = (event: PointerEvent) => {
      const ripple = doc.createElement('div');
      ripple.className = 'ripple';
      ripple.style.left = `${event.clientX}px`;
      ripple.style.top = `${event.clientY}px`;
      layer.append(ripple);
      setTimeout(() => ripple.remove(), RIPPLE_MS + 50);
    };
    doc.addEventListener('pointerdown', onDown, true);
    cleanups.push(() => doc.removeEventListener('pointerdown', onDown, true));
  }

  if (options.keystrokes) {
    const onKey = (event: KeyboardEvent) => {
      const target = event.target as Element | null;
      const label = keyLabel(event, {
        tagName: target?.tagName,
        type: (target as HTMLInputElement | null)?.type,
        autocomplete: (target as HTMLInputElement | null)?.autocomplete,
      });
      if (label === null) {
        return;
      }
      const pill = doc.createElement('div');
      pill.className = 'key';
      pill.textContent = label;
      keys.append(pill);
      while (keys.childElementCount > 4) {
        keys.firstElementChild?.remove();
      }
      setTimeout(() => pill.remove(), KEY_MS + 50);
    };
    doc.addEventListener('keydown', onKey, true);
    cleanups.push(() => doc.removeEventListener('keydown', onKey, true));
  }

  return () => {
    cleanups.forEach((cleanup) => cleanup());
    host.remove();
  };
}
