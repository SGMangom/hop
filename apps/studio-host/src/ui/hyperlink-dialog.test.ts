import { describe, expect, it } from 'vitest';
import { normalizeHyperlinkUrl } from './hyperlink-dialog';

describe('normalizeHyperlinkUrl', () => {
  it('keeps a real URL payload while trimming surrounding whitespace', () => {
    expect(normalizeHyperlinkUrl('  https://example.com/docs?q=한글&x=1  '))
      .toBe('https://example.com/docs?q=한글&x=1');
  });

  it('rejects empty/control-character payloads', () => {
    expect(normalizeHyperlinkUrl('   ')).toBeNull();
    expect(normalizeHyperlinkUrl('https://example.com/\nnext')).toBeNull();
  });
});
