/** What the keystroke overlay shows for a key press, or `null` to show nothing. */

export interface KeyLike {
  key: string;
  ctrlKey?: boolean;
  metaKey?: boolean;
  altKey?: boolean;
  shiftKey?: boolean;
  repeat?: boolean;
}

export interface TargetLike {
  tagName?: string;
  type?: string;
  autocomplete?: string;
  isContentEditable?: boolean;
}

const NAMES: Record<string, string> = {
  ' ': 'Space',
  Escape: 'Esc',
  ArrowUp: '↑',
  ArrowDown: '↓',
  ArrowLeft: '←',
  ArrowRight: '→',
  Backspace: '⌫',
  Delete: 'Del',
  Enter: 'Enter',
  Tab: 'Tab',
  PageUp: 'PgUp',
  PageDown: 'PgDn',
};

const MODIFIER_KEYS = new Set(['Control', 'Meta', 'Alt', 'Shift', 'AltGraph', 'CapsLock']);

/**
 * Fields whose contents must never be shown on a recording: passwords, one-time codes and card
 * numbers. Nothing typed into them reaches the overlay, modifiers or not.
 */
export function isSensitiveTarget(target: TargetLike | null | undefined): boolean {
  if (!target) {
    return false;
  }
  if ((target.tagName ?? '').toUpperCase() === 'INPUT') {
    const type = (target.type ?? '').toLowerCase();
    if (type === 'password') {
      return true;
    }
    const autocomplete = (target.autocomplete ?? '').toLowerCase();
    return (
      autocomplete.includes('one-time-code') ||
      autocomplete.includes('cc-') ||
      autocomplete.includes('password')
    );
  }
  return false;
}

/**
 * The label for a key press: "Ctrl + K", "Enter", "Shift + ←", "a". Pure modifier presses and
 * auto-repeat give nothing. Keys typed into sensitive fields give nothing.
 */
export function keyLabel(event: KeyLike, target?: TargetLike | null): string | null {
  if (event.repeat || MODIFIER_KEYS.has(event.key) || isSensitiveTarget(target)) {
    return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey) parts.push('Ctrl');
  if (event.metaKey) parts.push('⌘');
  if (event.altKey) parts.push('Alt');
  const named = NAMES[event.key];
  const printable = event.key.length === 1;
  // Shift only matters when it isn't already in the character (Shift + ←, Ctrl + Shift + P).
  if (event.shiftKey && (named !== undefined || parts.length > 0)) parts.push('Shift');
  parts.push(named ?? (printable ? event.key.toUpperCase() : event.key));
  return parts.join(' + ');
}
