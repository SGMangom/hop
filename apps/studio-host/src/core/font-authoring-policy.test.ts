import { describe, expect, it } from 'vitest';

import {
  filterAuthoringFontFamilies,
  isAuthoringFontFamilyAllowed,
  isAuthoringBlockedFontFamily,
  sanitizeAuthoringFontFamily,
} from './font-authoring-policy';

describe('font authoring policy', () => {
  it('blocks proprietary Hancom and Human authoring families', () => {
    expect(isAuthoringBlockedFontFamily('HY헤드라인M')).toBe(true);
    expect(isAuthoringBlockedFontFamily('HYHeadLine M')).toBe(true);
    expect(isAuthoringBlockedFontFamily('휴먼명조')).toBe(true);
    expect(isAuthoringBlockedFontFamily('HCI Poppy')).toBe(true);
    expect(isAuthoringBlockedFontFamily('Happiness Sans Regular')).toBe(false);
  });

  it('normalizes blocked authoring families to safe substitutes', () => {
    expect(sanitizeAuthoringFontFamily('HY헤드라인M')).toBe('함초롬돋움');
    expect(sanitizeAuthoringFontFamily('휴먼명조')).toBe('함초롬바탕');
    expect(filterAuthoringFontFamilies(['HY헤드라인M', '나눔고딕'])).toEqual(['나눔고딕']);
  });

  it('allows a restricted family only when an exact derived face is verified by the caller', () => {
    expect(isAuthoringFontFamilyAllowed('HCI Poppy')).toBe(false);
    expect(isAuthoringFontFamilyAllowed('HCI Poppy', true)).toBe(true);
    expect(sanitizeAuthoringFontFamily('HCI Poppy', true)).toBe('HCI Poppy');
    expect(filterAuthoringFontFamilies(
      ['HY헤드라인M', '휴먼명조', '나눔고딕'],
      (family) => family === '휴먼명조',
    )).toEqual(['휴먼명조', '나눔고딕']);
  });

});
