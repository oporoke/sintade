import { describe, expect, it } from 'vitest';

import { isSensitiveTarget, keyLabel } from './keys';

describe('keyLabel', () => {
  it('shows shortcuts with their modifiers', () => {
    expect(keyLabel({ key: 'k', ctrlKey: true })).toBe('Ctrl + K');
    expect(keyLabel({ key: 'p', ctrlKey: true, shiftKey: true })).toBe('Ctrl + Shift + P');
    expect(keyLabel({ key: 's', metaKey: true })).toBe('⌘ + S');
    expect(keyLabel({ key: 'Tab', altKey: true })).toBe('Alt + Tab');
  });

  it('names special keys', () => {
    expect(keyLabel({ key: 'Enter' })).toBe('Enter');
    expect(keyLabel({ key: ' ' })).toBe('Space');
    expect(keyLabel({ key: 'ArrowLeft', shiftKey: true })).toBe('Shift + ←');
    expect(keyLabel({ key: 'Escape' })).toBe('Esc');
    expect(keyLabel({ key: 'F5' })).toBe('F5');
  });

  it('shows plain characters as typed, upper-cased', () => {
    expect(keyLabel({ key: 'a' })).toBe('A');
    expect(keyLabel({ key: 'A', shiftKey: true })).toBe('A');
    expect(keyLabel({ key: '?', shiftKey: true })).toBe('?');
  });

  it('ignores pure modifiers and auto-repeat', () => {
    for (const key of ['Control', 'Shift', 'Alt', 'Meta', 'CapsLock']) {
      expect(keyLabel({ key })).toBeNull();
    }
    expect(keyLabel({ key: 'a', repeat: true })).toBeNull();
  });

  it('shows nothing typed into passwords, one-time codes and card fields', () => {
    expect(keyLabel({ key: 'h' }, { tagName: 'INPUT', type: 'password' })).toBeNull();
    expect(
      keyLabel({ key: 'v', ctrlKey: true }, { tagName: 'input', type: 'PASSWORD' }),
    ).toBeNull();
    expect(
      keyLabel({ key: '1' }, { tagName: 'INPUT', type: 'text', autocomplete: 'one-time-code' }),
    ).toBeNull();
    expect(
      keyLabel({ key: '4' }, { tagName: 'INPUT', type: 'text', autocomplete: 'cc-number' }),
    ).toBeNull();
    expect(keyLabel({ key: 'h' }, { tagName: 'INPUT', type: 'text' })).toBe('H');
  });
});

describe('isSensitiveTarget', () => {
  it('is false without a target or for ordinary elements', () => {
    expect(isSensitiveTarget(null)).toBe(false);
    expect(isSensitiveTarget(undefined)).toBe(false);
    expect(isSensitiveTarget({ tagName: 'DIV' })).toBe(false);
    expect(isSensitiveTarget({ tagName: 'TEXTAREA' })).toBe(false);
  });
});
