import { describe, expect, it, vi } from 'vitest';
import type { WasmBridge } from '@/upstream/core';
import { insertBodyHyperlink } from './hyperlink-insert';

describe('insertBodyHyperlink', () => {
  it('passes a real URL and display text to the generated engine API', () => {
    const insertHyperlink = vi.fn(() => JSON.stringify({
      ok: true,
      fieldId: 7,
      startOffset: 3,
      charOffset: 10,
      controlIdx: 2,
    }));
    const wasm = { doc: { insertHyperlink } } as unknown as WasmBridge;

    expect(insertBodyHyperlink(
      wasm,
      0,
      4,
      3,
      'https://example.com/docs?q=한글',
      '문서 열기',
    )).toEqual({
      ok: true,
      fieldId: 7,
      startOffset: 3,
      charOffset: 10,
      controlIdx: 2,
    });
    expect(insertHyperlink).toHaveBeenCalledWith(
      0,
      4,
      3,
      'https://example.com/docs?q=한글',
      '문서 열기',
    );
  });

  it('fails clearly when the generated overlay API is absent', () => {
    expect(() => insertBodyHyperlink(
      {} as WasmBridge,
      0,
      0,
      0,
      'https://example.com',
      'Example',
    )).toThrow(/지원하지 않습니다/);
  });
});
