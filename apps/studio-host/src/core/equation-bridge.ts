import { WasmBridge } from '@/upstream/core';
import type { CharProperties } from '@/upstream/core';

export const EQUATION_FONT = 'Computer Modern';

export interface DocumentStatistics {
  paragraphCount: number;
  characterCountWithSpaces: number;
  characterCountWithoutSpaces: number;
  wordCount: number;
}

interface DocumentStatisticsWasmDocument {
  getDocumentStatistics(): string;
}

export interface TwoColumnPresetResult {
  ok: boolean;
  leftWidth: number;
  rightWidth: number;
  spacing: number;
}

interface ColumnPresetWasmDocument {
  setTwoColumnPreset(sectionIndex: number, narrowLeft: boolean, spacingHu: number): string;
}

export interface HeaderFooterTextPosition {
  sectionIdx: number;
  isHeader: boolean;
  applyTo: number;
  paraIdx: number;
  charOffset: number;
}

export interface HeaderFooterSelectionRect {
  pageIndex: number;
  x: number;
  y: number;
  width: number;
  height: number;
}

interface HeaderFooterEditingWasmDocument {
  getCharPropertiesInHf(
    sectionIdx: number,
    isHeader: boolean,
    applyTo: number,
    paraIdx: number,
    charOffset: number,
  ): string;
  applyCharFormatInHf(
    sectionIdx: number,
    isHeader: boolean,
    applyTo: number,
    paraIdx: number,
    startOffset: number,
    endOffset: number,
    propsJson: string,
  ): string;
  getSelectionRectsInHeaderFooter(
    pageIndex: number,
    isHeader: boolean,
    startParaIdx: number,
    startCharOffset: number,
    endParaIdx: number,
    endCharOffset: number,
  ): string;
}

interface PendingHeaderFooterFormat {
  target: Omit<HeaderFooterTextPosition, 'charOffset'>;
  anchorOffset: number;
  nextOffset: number;
  props: Partial<CharProperties>;
}

/** Persist the equation font in the editable HWP record. */
export class EquationBridge extends WasmBridge {
  private pendingHeaderFooterFormat: PendingHeaderFooterFormat | null = null;

  private headerFooterDocument(): HeaderFooterEditingWasmDocument {
    const document = (this as unknown as { doc: HeaderFooterEditingWasmDocument | null }).doc;
    if (!document) throw new Error('문서가 로드되지 않았습니다.');
    return document;
  }

  getCharPropertiesInHf(position: HeaderFooterTextPosition): CharProperties {
    const parsed = JSON.parse(this.headerFooterDocument().getCharPropertiesInHf(
      position.sectionIdx,
      position.isHeader,
      position.applyTo,
      position.paraIdx,
      position.charOffset,
    )) as CharProperties;
    const pending = this.getHeaderFooterPendingCharFormat(position);
    return pending ? { ...parsed, ...pending } : parsed;
  }

  applyCharFormatInHf(
    position: Omit<HeaderFooterTextPosition, 'charOffset'>,
    startOffset: number,
    endOffset: number,
    props: Partial<CharProperties>,
  ): void {
    this.headerFooterDocument().applyCharFormatInHf(
      position.sectionIdx,
      position.isHeader,
      position.applyTo,
      position.paraIdx,
      startOffset,
      endOffset,
      JSON.stringify(props),
    );
  }

  getSelectionRectsInHeaderFooter(
    pageIndex: number,
    isHeader: boolean,
    startParaIdx: number,
    startCharOffset: number,
    endParaIdx: number,
    endCharOffset: number,
  ): HeaderFooterSelectionRect[] {
    return JSON.parse(this.headerFooterDocument().getSelectionRectsInHeaderFooter(
      pageIndex,
      isHeader,
      startParaIdx,
      startCharOffset,
      endParaIdx,
      endCharOffset,
    )) as HeaderFooterSelectionRect[];
  }

  stageHeaderFooterPendingCharFormat(
    position: HeaderFooterTextPosition,
    props: Partial<CharProperties>,
  ): void {
    const current = this.pendingHeaderFooterFormat;
    const sameTarget = current
      && current.target.sectionIdx === position.sectionIdx
      && current.target.isHeader === position.isHeader
      && current.target.applyTo === position.applyTo
      && current.target.paraIdx === position.paraIdx
      && (position.charOffset === current.anchorOffset || position.charOffset === current.nextOffset);
    this.pendingHeaderFooterFormat = {
      target: {
        sectionIdx: position.sectionIdx,
        isHeader: position.isHeader,
        applyTo: position.applyTo,
        paraIdx: position.paraIdx,
      },
      anchorOffset: sameTarget ? current.anchorOffset : position.charOffset,
      nextOffset: position.charOffset,
      props: { ...(sameTarget ? current.props : undefined), ...props },
    };
  }

  getHeaderFooterPendingCharFormat(
    position: HeaderFooterTextPosition,
  ): Partial<CharProperties> | undefined {
    const pending = this.pendingHeaderFooterFormat;
    if (!pending) return undefined;
    const matches = pending.target.sectionIdx === position.sectionIdx
      && pending.target.isHeader === position.isHeader
      && pending.target.applyTo === position.applyTo
      && pending.target.paraIdx === position.paraIdx
      && (position.charOffset === pending.anchorOffset || position.charOffset === pending.nextOffset);
    return matches ? pending.props : undefined;
  }

  clearHeaderFooterPendingCharFormat(): void {
    this.pendingHeaderFooterFormat = null;
  }

  override insertTextInHeaderFooter(
    sec: number,
    isHeader: boolean,
    applyTo: number,
    hfParaIdx: number,
    charOffset: number,
    text: string,
  ): string {
    const pending = this.pendingHeaderFooterFormat;
    const canApplyPending = Boolean(
      pending
      && pending.target.sectionIdx === sec
      && pending.target.isHeader === isHeader
      && pending.target.applyTo === applyTo
      && pending.target.paraIdx === hfParaIdx
      && (charOffset === pending.anchorOffset || charOffset === pending.nextOffset),
    );
    const result = super.insertTextInHeaderFooter(sec, isHeader, applyTo, hfParaIdx, charOffset, text);
    if (canApplyPending && pending && text.length > 0) {
      const parsed = JSON.parse(result) as { charOffset?: number };
      const nextOffset = Number.isSafeInteger(parsed.charOffset)
        ? parsed.charOffset as number
        : charOffset + [...text].length;
      if (nextOffset > charOffset) {
        this.applyCharFormatInHf(
          { sectionIdx: sec, isHeader, applyTo, paraIdx: hfParaIdx },
          charOffset,
          nextOffset,
          pending.props,
        );
        pending.nextOffset = nextOffset;
      }
    }
    return result;
  }

  getDocumentStatistics(): DocumentStatistics {
    const document = (this as unknown as { doc: DocumentStatisticsWasmDocument | null }).doc;
    if (!document || typeof document.getDocumentStatistics !== 'function') {
      throw new Error('문서 통계 API를 사용할 수 없습니다.');
    }
    const parsed = JSON.parse(document.getDocumentStatistics()) as Partial<DocumentStatistics>;
    for (const value of [
      parsed.paragraphCount,
      parsed.characterCountWithSpaces,
      parsed.characterCountWithoutSpaces,
      parsed.wordCount,
    ]) {
      if (!Number.isSafeInteger(value) || (value as number) < 0) {
        throw new Error('문서 통계 응답이 올바르지 않습니다.');
      }
    }
    return parsed as DocumentStatistics;
  }

  setTwoColumnPreset(
    sectionIndex: number,
    narrowLeft: boolean,
    spacingHu: number,
  ): TwoColumnPresetResult {
    const document = (this as unknown as { doc: ColumnPresetWasmDocument | null }).doc;
    if (!document || typeof document.setTwoColumnPreset !== 'function') {
      throw new Error('2단 너비 프리셋 API를 사용할 수 없습니다.');
    }
    const parsed = JSON.parse(
      document.setTwoColumnPreset(sectionIndex, narrowLeft, spacingHu),
    ) as Partial<TwoColumnPresetResult>;
    if (
      parsed.ok !== true
      || !Number.isSafeInteger(parsed.leftWidth)
      || !Number.isSafeInteger(parsed.rightWidth)
      || !Number.isSafeInteger(parsed.spacing)
      || (parsed.leftWidth as number) <= 0
      || (parsed.rightWidth as number) <= 0
      || (parsed.spacing as number) < 0
    ) {
      throw new Error('2단 너비 프리셋 응답이 올바르지 않습니다.');
    }
    return parsed as TwoColumnPresetResult;
  }

  override insertEquation(...args: Parameters<WasmBridge['insertEquation']>): ReturnType<WasmBridge['insertEquation']> {
    const result = super.insertEquation(...args);
    if (result.ok) {
      const updated = this.setEquationProperties(args[0], result.paraIdx, result.controlIdx, undefined, undefined, {
        fontName: EQUATION_FONT,
      });
      if (!updated.ok) throw new Error('수식 글꼴을 설정하지 못했습니다.');
    }
    return result;
  }
}
