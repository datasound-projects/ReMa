import { describe, expect, it } from 'vitest';

import { formatSize, isPast } from './documents';

describe('document helpers', () => {
  it('formats sizes', () => {
    expect(formatSize(512)).toBe('512 B');
    expect(formatSize(240 * 1024)).toBe('240 KB');
    expect(formatSize(3.5 * 1024 * 1024)).toBe('3.5 MB');
  });

  it('knows when a credential has expired (end of the stated period)', () => {
    const today = new Date(2026, 8, 26);
    expect(isPast('2026-08', today)).toBe(true);
    expect(isPast('2026-09', today)).toBe(false);
    expect(isPast('2026-09-25', today)).toBe(true);
    expect(isPast('2026', today)).toBe(false);
    expect(isPast('2025', today)).toBe(true);
    expect(isPast('not a date', today)).toBe(false);
  });
});
