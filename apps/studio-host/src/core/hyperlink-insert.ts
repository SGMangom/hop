import type { WasmBridge } from '@/upstream/core';

export interface HyperlinkInsertResult {
  ok: boolean;
  fieldId?: number;
  startOffset?: number;
  charOffset?: number;
  controlIdx?: number;
}

interface HyperlinkDocumentExport {
  insertHyperlink(
    sectionIndex: number,
    paragraphIndex: number,
    charOffset: number,
    url: string,
    displayText: string,
  ): string | HyperlinkInsertResult;
}

/**
 * HOP-only bridge for the generated rhwp overlay API.
 *
 * Upstream WasmBridge intentionally does not know this HOP patch method. Keeping
 * the cast here makes the compatibility boundary explicit instead of spreading
 * private document access through command code.
 */
export function insertBodyHyperlink(
  wasm: WasmBridge,
  sectionIndex: number,
  paragraphIndex: number,
  charOffset: number,
  url: string,
  displayText: string,
): HyperlinkInsertResult {
  const doc = (wasm as unknown as { doc?: HyperlinkDocumentExport | null }).doc;
  if (!doc || typeof doc.insertHyperlink !== 'function') {
    throw new Error('현재 문서 엔진은 하이퍼링크 삽입을 지원하지 않습니다.');
  }
  const result = doc.insertHyperlink(
    sectionIndex,
    paragraphIndex,
    charOffset,
    url,
    displayText,
  );
  return typeof result === 'string' ? JSON.parse(result) as HyperlinkInsertResult : result;
}
