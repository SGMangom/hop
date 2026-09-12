import { describe, expect, it } from 'vitest';
import { createRendererSession } from './renderer-session';

describe('createRendererSession', () => {
  it('uses auto CanvasKit policy but safely stays on Canvas2D when preflight is ineligible', async () => {
    const session = createRendererSession();
    session.beginDocument('document-a');

    const selection = await session.resolve({
      getCanvasKitDocumentPreflight: () => ({
        schemaVersion: 1,
        mode: 'default',
        profile: 'screen',
        status: 'ineligible',
        eligible: false,
        complete: true,
        pageCount: 1,
        scannedPages: 1,
        scannedWorkUnits: 1,
        blockers: [{ code: 'unsupported', pageIndex: 0, opType: 'test', detail: 'test-blocker' }],
        summary: {
          totalItems: 1,
          directItems: 0,
          directRequiredItems: 0,
          compatOverlayItems: 0,
          textFallbackItems: 0,
          unsupportedItems: 1,
          hiddenOverlayViolations: 0,
        },
        limits: {
          maxPages: 100,
          maxWorkUnits: 1000,
          maxBlockers: 100,
          maxRequiredFontFamilies: 100,
        },
        requiredFontFamilies: [],
        capabilityDigest: 'ineligible-document',
      }),
    });

    expect(selection.backend).toBe('canvas2d');
    expect(selection.diagnostics.request.backend).toBe('auto');
    expect(selection.diagnostics.selectionReason).toBe('autoIneligible');
    expect(selection.diagnostics.documentDigest).toBe('document-a');
  });

  it('folds an unavailable font into auto preflight instead of attempting an unsafe CanvasKit replay', async () => {
    const session = createRendererSession();
    session.beginDocument('document-with-unknown-font');

    const selection = await session.resolve({
      getCanvasKitDocumentPreflight: () => ({
        schemaVersion: 1,
        mode: 'default',
        profile: 'screen',
        status: 'eligible',
        eligible: true,
        complete: true,
        pageCount: 1,
        scannedPages: 1,
        scannedWorkUnits: 1,
        blockers: [],
        summary: {
          totalItems: 1,
          directItems: 1,
          directRequiredItems: 1,
          compatOverlayItems: 0,
          textFallbackItems: 0,
          unsupportedItems: 0,
          hiddenOverlayViolations: 0,
        },
        limits: {
          maxPages: 100,
          maxWorkUnits: 1000,
          maxBlockers: 100,
          maxRequiredFontFamilies: 100,
        },
        requiredFontFamilies: ['Definitely Missing HOP Font'],
        capabilityDigest: 'eligible-before-font-plan',
      }),
    });

    expect(selection.backend).toBe('canvas2d');
    expect(selection.diagnostics.selectionReason).toBe('autoIneligible');
    expect(selection.diagnostics.preflight?.blockers).toEqual(expect.arrayContaining([
      expect.objectContaining({ detail: 'fontUnavailable:Definitely Missing HOP Font' }),
    ]));
  });
});
