import { RESUME_END_MARGIN_S, loadResume, saveResume } from './resume';

describe('resume position', () => {
  beforeEach(() => localStorage.clear());

  it('comes back where the viewer left off', () => {
    saveResume('abc', 42.37, 120);
    expect(loadResume('abc', 120)).toBe(42.3);
    expect(loadResume('other', 120)).toBeNull();
  });

  it('is forgotten near the end, so a finished video starts over', () => {
    saveResume('abc', 42, 120);
    saveResume('abc', 120 - RESUME_END_MARGIN_S + 1, 120);
    expect(loadResume('abc', 120)).toBeNull();
  });

  it('keeps a short video resumable until its last tenth', () => {
    saveResume('abc', 6.5, 10);
    expect(loadResume('abc', 10)).toBe(6.5);
    saveResume('abc', 9.5, 10);
    expect(loadResume('abc', 10)).toBeNull();
  });

  it('ignores the first seconds and a stored position past a shorter video', () => {
    saveResume('abc', 1, 120);
    expect(loadResume('abc', 120)).toBeNull();
    saveResume('abc', 90, 120);
    expect(loadResume('abc', 60)).toBeNull();
  });

  it('survives storage being unavailable', () => {
    const spy = vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('quota');
    });
    expect(() => saveResume('abc', 30, 120)).not.toThrow();
    spy.mockRestore();
  });
});
