import { describe, expect, it, vi } from 'vitest';
import { readDocumentStatisticsSnapshot } from './document-statistics-dialog';

describe('readDocumentStatisticsSnapshot', () => {
  it('reads the authoritative engine snapshot without mutating the document', () => {
    const getDocumentStatistics = vi.fn(() => ({
      paragraphCount: 12,
      characterCountWithSpaces: 345,
      characterCountWithoutSpaces: 301,
      wordCount: 76,
    }));
    const services = { wasm: { getDocumentStatistics } };
    expect(readDocumentStatisticsSnapshot(services as never)).toEqual({
      paragraphCount: 12,
      characterCountWithSpaces: 345,
      characterCountWithoutSpaces: 301,
      wordCount: 76,
    });
    expect(getDocumentStatistics).toHaveBeenCalledTimes(1);
  });

  it('fails explicitly when the engine surface is unavailable', () => {
    expect(() => readDocumentStatisticsSnapshot({ wasm: {} } as never)).toThrow(/문서 통계 API/);
  });
});
