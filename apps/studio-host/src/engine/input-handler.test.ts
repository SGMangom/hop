import { describe, expect, it, vi } from 'vitest';
import { InputHandler, headerFooterRegionAtPoint, orderHeaderFooterSelection } from './input-handler';

const page = {
  width: 600,
  height: 800,
  marginTop: 50,
  marginBottom: 40,
  marginHeader: 30,
  marginFooter: 20,
};

describe('HOP header/footer input extension', () => {
  it('treats the full horizontal top/bottom margin bands as direct-edit regions', () => {
    expect(headerFooterRegionAtPoint(page, 1, 0)).toBe('header');
    expect(headerFooterRegionAtPoint(page, 599, 79)).toBe('header');
    expect(headerFooterRegionAtPoint(page, 300, 80)).toBeNull();
    expect(headerFooterRegionAtPoint(page, 300, 740)).toBeNull();
    expect(headerFooterRegionAtPoint(page, 1, 741)).toBe('footer');
    expect(headerFooterRegionAtPoint(page, 599, 799)).toBe('footer');
    expect(headerFooterRegionAtPoint(page, -1, 10)).toBeNull();
    expect(headerFooterRegionAtPoint(page, 601, 790)).toBeNull();
  });

  it('normalizes a dragged header/footer text selection in document order', () => {
    const late = { pageIndex: 2, paraIdx: 1, charOffset: 7 };
    const early = { pageIndex: 2, paraIdx: 0, charOffset: 3 };
    expect(orderHeaderFooterSelection(late, early)).toEqual({ start: early, end: late });
    expect(orderHeaderFooterSelection(early, early)).toBeNull();
    expect(orderHeaderFooterSelection(early, { ...late, pageIndex: 3 })).toBeNull();
  });

  it('routes caret font family/size changes to the header/footer pending format path', () => {
    const stagePending = vi.fn();
    const bodyApply = vi.fn();
    const emitCursorFormatState = vi.fn();
    const handler = Object.create(InputHandler.prototype) as Record<string, unknown>;
    handler.hopWasm = {
      stageHeaderFooterPendingCharFormat: stagePending,
    };
    handler.hfSelectionAnchor = null;
    handler.hfSelectionFocus = null;
    handler.cursor = {
      isInHeaderFooter: () => true,
      headerFooterMode: 'header',
      hfSectionIdx: 2,
      hfApplyTo: 1,
      hfParaIdx: 3,
      hfCharOffset: 4,
    };
    handler.applyCharFormat = bodyApply;
    handler.getCharPropertiesAtCursor = vi.fn();
    handler.emitCursorFormatState = emitCursorFormatState;

    const patch = (InputHandler.prototype as unknown as Record<string, (this: object) => void>)
      .patchCharacterFormattingForHeaderFooter;
    patch.call(handler);

    const props = { fontId: 42, fontSize: 1_200 };
    (handler.applyCharFormat as (value: typeof props) => void)(props);

    expect(bodyApply).not.toHaveBeenCalled();
    expect(stagePending).toHaveBeenCalledWith({
      sectionIdx: 2,
      isHeader: true,
      applyTo: 1,
      paraIdx: 3,
      charOffset: 4,
    }, props);
    expect(emitCursorFormatState).toHaveBeenCalledOnce();
  });
});
