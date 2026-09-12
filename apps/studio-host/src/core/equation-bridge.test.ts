import { describe, expect, it, vi } from 'vitest';
import { HwpDocument, initSync } from '@wasm/rhwp.js';
import { EquationBridge } from './equation-bridge';

const { readFileSync } = await vi.importActual<{
  readFileSync: { (path: URL): Uint8Array; (path: URL, encoding: 'utf8'): string };
}>('node:fs');

initSync({ module: readFileSync(new URL('../../../../target-local/rhwp-wasm/rhwp_bg.wasm', import.meta.url)) });

describe('Computer Modern equations', () => {
  it('exposes the existing insert command in both menu and toolbar', () => {
    const html = readFileSync(new URL('../../index.html', import.meta.url), 'utf8');
    expect(html.match(/data-cmd="insert:equation"/g)).toHaveLength(2);
  });

  it('preserves editable equation content and font through HWP save/reopen and undo', () => {
    const bridge = new EquationBridge();
    bridge.createNewDocument();
    const before = bridge.saveSnapshot();
    const script = '{a+b} over {c+d} + int _0 ^infinity e^{-x} dx';
    const result = bridge.insertEquation(0, 0, 0, script, 1400, 0);
    expect(result.ok).toBe(true);
    expect(bridge.renderPageSvg(0)).toContain(`font-family="'Computer Modern', serif"`);
    expect(bridge.renderPageSvgWithProfile(0, 'screen')).not.toContain('Latin Modern Math');
    expect(bridge.renderEquationPreview(script, 1400, 0)).toContain(`font-family="'Computer Modern', serif"`);

    const restored = new HwpDocument(bridge.exportHwp());
    const equation = JSON.parse(restored.getControls()).find((control: { ctrlId: string }) => control.ctrlId === 'eqed');
    expect(equation).toBeDefined();
    expect(JSON.parse(restored.getEquationProperties(0, equation.para, equation.controlIndex, -1, -1))).toMatchObject({
      script, fontName: 'Computer Modern', fontSize: 1400,
    });
    restored.free();
    bridge.restoreSnapshot(before);
    expect(bridge.renderPageSvg(0)).not.toContain(`font-family="'Computer Modern', serif"`);
    bridge.releaseDocument();
  });

  it('applies font family, size and bold to header/footer text and preserves them through HWPX reopen', () => {
    const bridge = new EquationBridge();
    bridge.createNewDocument();
    bridge.createHeaderFooter(0, true, 0);
    bridge.insertTextInHeaderFooter(0, true, 0, 0, 0, '머리말');
    const pretendardId = bridge.findOrCreateFontId('Pretendard');
    bridge.applyCharFormatInHf(
      { sectionIdx: 0, isHeader: true, applyTo: 0, paraIdx: 0 },
      0,
      3,
      { fontId: pretendardId, fontSize: 1800, bold: true },
    );

    expect(bridge.getCharPropertiesInHf({
      sectionIdx: 0, isHeader: true, applyTo: 0, paraIdx: 0, charOffset: 1,
    })).toMatchObject({ fontFamily: 'Pretendard', fontSize: 1800, bold: true });

    const reopened = new HwpDocument(bridge.exportHwpx());
    expect(JSON.parse(reopened.getCharPropertiesInHf(0, true, 0, 0, 1))).toMatchObject({
      fontFamily: 'Pretendard', fontSize: 1800, bold: true,
    });
    reopened.free();
    bridge.releaseDocument();
  });

  it('applies caret-pending font and size to newly typed header/footer text', () => {
    const bridge = new EquationBridge();
    bridge.createNewDocument();
    bridge.createHeaderFooter(0, false, 0);
    const fontId = bridge.findOrCreateFontId('NanumGothic');
    bridge.stageHeaderFooterPendingCharFormat(
      { sectionIdx: 0, isHeader: false, applyTo: 0, paraIdx: 0, charOffset: 0 },
      { fontId, fontSize: 1500 },
    );
    bridge.insertTextInHeaderFooter(0, false, 0, 0, 0, '꼬리말');

    expect(bridge.getCharPropertiesInHf({
      sectionIdx: 0, isHeader: false, applyTo: 0, paraIdx: 0, charOffset: 1,
    })).toMatchObject({ fontFamily: 'NanumGothic', fontSize: 1500 });
    bridge.releaseDocument();
  });

  it('applies mirrored two-column width presets with the requested spacing', () => {
    const bridge = new EquationBridge();
    bridge.createNewDocument();

    const left = bridge.setTwoColumnPreset(0, true, 2268);
    expect(left.ok).toBe(true);
    expect(left.leftWidth).toBeLessThan(left.rightWidth);
    expect(left.spacing).toBe(2268);
    const leftColumns = bridge.getPageInfo(0).columns!;
    expect(leftColumns).toHaveLength(2);
    expect(leftColumns[0].width).toBeLessThan(leftColumns[1].width);

    const right = bridge.setTwoColumnPreset(0, false, 2268);
    expect(right.ok).toBe(true);
    expect(right.leftWidth).toBe(left.rightWidth);
    expect(right.rightWidth).toBe(left.leftWidth);
    expect(right.spacing).toBe(2268);
    const rightColumns = bridge.getPageInfo(0).columns!;
    expect(rightColumns).toHaveLength(2);
    expect(rightColumns[0].width).toBeGreaterThan(rightColumns[1].width);
    expect(rightColumns[0].width).toBeCloseTo(leftColumns[1].width, 5);
    expect(rightColumns[1].width).toBeCloseTo(leftColumns[0].width, 5);

    bridge.releaseDocument();
  });

  it('applies font family/size/general character formatting to HF ranges and keeps pending formatting for new input', () => {
    const bridge = new EquationBridge();
    bridge.createNewDocument();
    bridge.createHeaderFooter(0, true, 0);
    bridge.insertTextInHeaderFooter(0, true, 0, 0, 0, 'ABCD');

    const fontId = bridge.findOrCreateFontId('HOP HF Test Font');
    expect(fontId).toBeGreaterThanOrEqual(0);
    bridge.applyCharFormatInHf(
      { sectionIdx: 0, isHeader: true, applyTo: 0, paraIdx: 0 },
      1,
      3,
      { fontId, fontSize: 1800, bold: true, italic: true },
    );

    const untouched = bridge.getCharPropertiesInHf({
      sectionIdx: 0, isHeader: true, applyTo: 0, paraIdx: 0, charOffset: 0,
    });
    const selected = bridge.getCharPropertiesInHf({
      sectionIdx: 0, isHeader: true, applyTo: 0, paraIdx: 0, charOffset: 1,
    });
    expect(selected).toMatchObject({
      fontFamily: 'HOP HF Test Font',
      fontSize: 1800,
      bold: true,
      italic: true,
    });
    expect(untouched.fontSize).not.toBe(1800);

    bridge.stageHeaderFooterPendingCharFormat(
      { sectionIdx: 0, isHeader: true, applyTo: 0, paraIdx: 0, charOffset: 4 },
      { fontId, fontSize: 2200, bold: true },
    );
    bridge.insertTextInHeaderFooter(0, true, 0, 0, 4, 'Z');
    expect(bridge.getCharPropertiesInHf({
      sectionIdx: 0, isHeader: true, applyTo: 0, paraIdx: 0, charOffset: 4,
    })).toMatchObject({ fontFamily: 'HOP HF Test Font', fontSize: 2200, bold: true });

    bridge.insertTextInHeaderFooter(0, true, 0, 0, 5, 'Y');
    expect(bridge.getCharPropertiesInHf({
      sectionIdx: 0, isHeader: true, applyTo: 0, paraIdx: 0, charOffset: 5,
    })).toMatchObject({ fontFamily: 'HOP HF Test Font', fontSize: 2200, bold: true });

    bridge.releaseDocument();
  });

});
