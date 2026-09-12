import { describe, expect, it, vi } from 'vitest';
import { initSync } from '@wasm/rhwp.js';
import { EquationBridge } from './equation-bridge';

const { readFileSync } = await vi.importActual<{
  readFileSync: (path: URL) => Uint8Array;
}>('node:fs');

initSync({ module: readFileSync(new URL('../../../../target-local/rhwp-wasm/rhwp_bg.wasm', import.meta.url)) });

describe('document statistics bridge', () => {
  it('reads live document statistics from the generated engine API', () => {
    const bridge = new EquationBridge();
    bridge.createNewDocument();
    bridge.insertText(0, 0, 0, '안녕 world');

    expect(bridge.getDocumentStatistics()).toEqual({
      paragraphCount: 1,
      characterCountWithSpaces: 8,
      characterCountWithoutSpaces: 7,
      wordCount: 2,
    });

    bridge.releaseDocument();
  });
});
