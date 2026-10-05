import { TestBed } from '@angular/core/testing';

import { PrivacyPage } from './privacy-page';

describe('PrivacyPage', () => {
  function render(): HTMLElement {
    TestBed.configureTestingModule({ imports: [PrivacyPage] });
    const fixture = TestBed.createComponent(PrivacyPage);
    fixture.detectChanges();
    return fixture.nativeElement as HTMLElement;
  }

  it('is marked as a draft until the legal review is done', () => {
    const root = render();
    expect(root.querySelector('[data-testid="privacy-draft"]')?.textContent).toContain('Draft');
  });

  it('explains every permission the extension asks for', () => {
    const root = render();
    const listed = Array.from(
      root.querySelectorAll('[data-testid="privacy-permissions"] tbody tr td:first-child code'),
    ).map((el) => el.textContent);
    // Keep in step with extension/manifest.json (checked by the extension's manifest test).
    expect(listed.sort()).toEqual(['activeTab', 'offscreen', 'scripting', 'storage', 'tabCapture']);
  });

  it('says what the extension does not do', () => {
    const text = render().textContent ?? '';
    expect(text).toContain('downloads no code');
    expect(text).toContain('runs no analytics');
    expect(text).toContain('never shown');
  });
});
