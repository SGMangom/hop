import { describe, expect, it, vi } from 'vitest';
import { readDocumentInfoSnapshot } from './document-info-dialog';

describe('readDocumentInfoSnapshot', () => {
  it('reports only authoritative current-session document facts', () => {
    const services = {
      wasm: {
        fileName: 'sample.hwpx',
        getSourceFormat: vi.fn(() => 'hwpx'),
        getDocumentInfo: vi.fn(() => ({
          version: '5.1.1.0',
          sectionCount: 3,
          pageCount: 12,
          encrypted: false,
          fallbackFont: '함초롬바탕',
          fontsUsed: ['함초롬바탕', '맑은 고딕'],
        })),
      },
      getContext: vi.fn(() => ({ sourceFormat: 'hwpx', isDirty: true })),
    };

    expect(readDocumentInfoSnapshot(services as never)).toEqual({
      fileName: 'sample.hwpx',
      sourcePath: null,
      sourceFormat: 'HWPX',
      version: '5.1.1.0',
      pageCount: 12,
      sectionCount: 3,
      encrypted: false,
      fallbackFont: '함초롬바탕',
      fontsUsed: ['함초롬바탕', '맑은 고딕'],
      dirty: true,
    });
  });

  it('reports the desktop source path when the bridge exposes it', () => {
    const services = {
      wasm: {
        fileName: 'sample.hwpx',
        getSourcePath: vi.fn(() => '/tmp/sample.hwpx'),
        getSourceFormat: vi.fn(() => 'hwpx'),
        getDocumentInfo: vi.fn(() => ({
          version: '5.1.1.0',
          sectionCount: 1,
          pageCount: 2,
          encrypted: false,
          fallbackFont: '함초롬바탕',
          fontsUsed: [],
        })),
      },
      getContext: vi.fn(() => ({ sourceFormat: 'hwpx', isDirty: false })),
    };

    expect(readDocumentInfoSnapshot(services as never).sourcePath).toBe('/tmp/sample.hwpx');
  });

  it('falls back to the bridge source format and neutral display values', () => {
    const services = {
      wasm: {
        fileName: '',
        getSourceFormat: vi.fn(() => 'hwp'),
        getDocumentInfo: vi.fn(() => ({
          version: '',
          sectionCount: 1,
          pageCount: 1,
          encrypted: true,
          fallbackFont: '',
          fontsUsed: [],
        })),
      },
      getContext: vi.fn(() => ({ sourceFormat: undefined, isDirty: false })),
    };

    expect(readDocumentInfoSnapshot(services as never)).toMatchObject({
      fileName: '문서',
      sourceFormat: 'HWP',
      version: '-',
      fallbackFont: '-',
      encrypted: true,
      dirty: false,
    });
  });
});
