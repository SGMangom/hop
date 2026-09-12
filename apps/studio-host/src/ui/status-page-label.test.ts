import { describe, expect, it } from 'vitest';
import { formatStatusPageLabel } from './status-page-label';

describe('status page label', () => {
  it('uses the compact physical label when logical numbering matches', () => {
    expect(formatStatusPageLabel(2, 5, 3)).toBe('3 / 5 쪽');
  });

  it('surfaces restarted logical numbering while retaining physical position', () => {
    expect(formatStatusPageLabel(2, 5, 10)).toBe('10 쪽 · 3 / 5');
  });

  it('falls back to physical numbering when the engine omits the logical number', () => {
    expect(formatStatusPageLabel(0, 1)).toBe('1 / 1 쪽');
  });
});
