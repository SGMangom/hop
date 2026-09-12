import { UpstreamInputHandler } from '@/upstream/input-handler';
import type { CharProperties, PageInfo } from '@/upstream/core';
import type { EquationBridge, HeaderFooterTextPosition } from '../core/equation-bridge';

type HfMode = 'header' | 'footer';

export interface HeaderFooterSelectionPoint {
  pageIndex: number;
  paraIdx: number;
  charOffset: number;
}

export interface HeaderFooterSelectionRange {
  start: HeaderFooterSelectionPoint;
  end: HeaderFooterSelectionPoint;
}

/** 한컴처럼 머리말/꼬리말의 가로 전체 여백 띠를 편집 진입 영역으로 본다. */
export function headerFooterRegionAtPoint(
  page: Pick<PageInfo, 'width' | 'height' | 'marginTop' | 'marginBottom' | 'marginHeader' | 'marginFooter'>,
  x: number,
  y: number,
): HfMode | null {
  if (x < 0 || x > page.width || y < 0 || y > page.height) return null;
  const bodyTop = page.marginTop + page.marginHeader;
  const bodyBottom = page.height - page.marginBottom - page.marginFooter;
  if (y < bodyTop) return 'header';
  if (y > bodyBottom) return 'footer';
  return null;
}

function comparePoint(a: HeaderFooterSelectionPoint, b: HeaderFooterSelectionPoint): number {
  return a.paraIdx - b.paraIdx || a.charOffset - b.charOffset;
}

export function orderHeaderFooterSelection(
  anchor: HeaderFooterSelectionPoint,
  focus: HeaderFooterSelectionPoint,
): HeaderFooterSelectionRange | null {
  if (anchor.pageIndex !== focus.pageIndex || comparePoint(anchor, focus) === 0) return null;
  return comparePoint(anchor, focus) < 0
    ? { start: anchor, end: focus }
    : { start: focus, end: anchor };
}

type InternalCursor = {
  isInHeaderFooter(): boolean;
  headerFooterMode: 'none' | HfMode;
  hfSectionIdx: number;
  hfApplyTo: number;
  hfParaIdx: number;
  hfCharOffset: number;
  getPosition(): { sectionIndex: number; paragraphIndex: number; charOffset: number };
  enterHeaderFooterMode(isHeader: boolean, sectionIdx: number, applyTo: number, preferredPage?: number): void;
  setHfCursorPosition(paraIdx: number, charOffset: number): void;
};

type InternalHandler = {
  active: boolean;
  cursor: InternalCursor;
  textarea: HTMLTextAreaElement;
  selectionRenderer: {
    render(rects: Array<{ pageIndex: number; x: number; y: number; width: number; height: number }>, zoom: number): void;
    clear(): void;
  };
  updateCaret(): void;
  emitCursorFormatState(): void;
  applyCharFormat(props: Partial<CharProperties>): void;
  getCharPropertiesAtCursor(): CharProperties;
  executeOperation(desc: unknown): void;
};

type PagePoint = { pageIndex: number; pageX: number; pageY: number };

/**
 * HOP 전용 최소 확장. upstream RHWP는 건드리지 않고 HF 직접 편집/선택/글자 서식만 보완한다.
 */
export class InputHandler extends UpstreamInputHandler {
  private readonly hopContainer: HTMLElement;
  private readonly hopWasm: EquationBridge;
  private readonly hopEventBus: ConstructorParameters<typeof UpstreamInputHandler>[2];
  private readonly hopVirtualScroll: ConstructorParameters<typeof UpstreamInputHandler>[3];
  private readonly hopViewportManager: ConstructorParameters<typeof UpstreamInputHandler>[4];
  private hfSelectionAnchor: HeaderFooterSelectionPoint | null = null;
  private hfSelectionFocus: HeaderFooterSelectionPoint | null = null;
  private hfSelectionDragging = false;

  private readonly onDblClickCaptureBound = (event: MouseEvent) => this.onHeaderFooterDoubleClickCapture(event);
  private readonly onMouseDownCaptureBound = (event: MouseEvent) => this.onHeaderFooterMouseDownCapture(event);
  private readonly onMouseMoveCaptureBound = (event: MouseEvent) => this.onHeaderFooterMouseMoveCapture(event);
  private readonly onMouseUpCaptureBound = (event: MouseEvent) => this.onHeaderFooterMouseUpCapture(event);

  constructor(...args: ConstructorParameters<typeof UpstreamInputHandler>) {
    super(...args);
    [this.hopContainer, this.hopWasm, this.hopEventBus, this.hopVirtualScroll, this.hopViewportManager] = args as unknown as [
      HTMLElement,
      EquationBridge,
      ConstructorParameters<typeof UpstreamInputHandler>[2],
      ConstructorParameters<typeof UpstreamInputHandler>[3],
      ConstructorParameters<typeof UpstreamInputHandler>[4],
    ];

    this.patchCharacterFormattingForHeaderFooter();
    this.hopContainer.addEventListener('dblclick', this.onDblClickCaptureBound, true);
    this.hopContainer.addEventListener('mousedown', this.onMouseDownCaptureBound, true);
    document.addEventListener('mousemove', this.onMouseMoveCaptureBound, true);
    document.addEventListener('mouseup', this.onMouseUpCaptureBound, true);
    this.hopEventBus.on('headerFooterModeChanged', (mode) => {
      if (mode === 'none') this.clearHeaderFooterSelection(true);
    });
    this.hopEventBus.on('history-jumped', () => this.clearHeaderFooterSelection(true));
  }

  override dispose(): void {
    this.hopContainer.removeEventListener('dblclick', this.onDblClickCaptureBound, true);
    this.hopContainer.removeEventListener('mousedown', this.onMouseDownCaptureBound, true);
    document.removeEventListener('mousemove', this.onMouseMoveCaptureBound, true);
    document.removeEventListener('mouseup', this.onMouseUpCaptureBound, true);
    super.dispose();
  }

  private patchCharacterFormattingForHeaderFooter(): void {
    const internals = this as unknown as InternalHandler;
    const originalApply = internals.applyCharFormat.bind(this);
    const originalGet = internals.getCharPropertiesAtCursor.bind(this);

    // upstream applyCharFormat()은 HF 모드에서 즉시 return한다. 이벤트 배선은 그대로 두고
    // 인스턴스 메서드만 HOP에서 분기해 body 동작은 원본에 위임한다.
    internals.applyCharFormat = (props: Partial<CharProperties>): void => {
      if (!internals.cursor.isInHeaderFooter()) {
        originalApply(props);
        return;
      }
      const selection = this.getHeaderFooterSelection();
      if (selection) {
        this.applyCharacterFormatToHeaderFooterSelection(selection, props);
      } else {
        this.hopWasm.stageHeaderFooterPendingCharFormat(this.currentHeaderFooterPosition(), props);
      }
      internals.emitCursorFormatState();
    };

    // 툴바 토글 방향/현재 글꼴도 본문 커서가 아니라 실제 HF 캐럿(또는 선택 시작점)을 읽는다.
    internals.getCharPropertiesAtCursor = (): CharProperties => {
      if (!internals.cursor.isInHeaderFooter()) return originalGet();
      const point = this.getHeaderFooterSelection()?.start;
      const position = this.currentHeaderFooterPosition(point);
      const info = JSON.parse(this.hopWasm.getHeaderFooterParaInfo(
        position.sectionIdx,
        position.isHeader,
        position.applyTo,
        position.paraIdx,
      )) as { charCount?: number };
      const charCount = Number.isSafeInteger(info.charCount) ? Number(info.charCount) : 0;
      const queryOffset = point
        ? Math.min(point.charOffset, Math.max(0, charCount - 1))
        : position.charOffset > 0
          ? Math.min(position.charOffset - 1, Math.max(0, charCount - 1))
          : 0;
      return this.hopWasm.getCharPropertiesInHf({ ...position, charOffset: queryOffset });
    };
  }

  private currentHeaderFooterPosition(point?: HeaderFooterSelectionPoint): HeaderFooterTextPosition {
    const cursor = (this as unknown as InternalHandler).cursor;
    return {
      sectionIdx: cursor.hfSectionIdx,
      isHeader: cursor.headerFooterMode === 'header',
      applyTo: cursor.hfApplyTo,
      paraIdx: point?.paraIdx ?? cursor.hfParaIdx,
      charOffset: point?.charOffset ?? cursor.hfCharOffset,
    };
  }

  private getHeaderFooterSelection(): HeaderFooterSelectionRange | null {
    if (!this.hfSelectionAnchor || !this.hfSelectionFocus) return null;
    return orderHeaderFooterSelection(this.hfSelectionAnchor, this.hfSelectionFocus);
  }

  private applyCharacterFormatToHeaderFooterSelection(
    selection: HeaderFooterSelectionRange,
    props: Partial<CharProperties>,
  ): void {
    const internals = this as unknown as InternalHandler;
    const cursor = internals.cursor;
    const sectionIdx = cursor.hfSectionIdx;
    const isHeader = cursor.headerFooterMode === 'header';
    const applyTo = cursor.hfApplyTo;
    const cursorBefore = cursor.getPosition();
    this.hopWasm.clearHeaderFooterPendingCharFormat();

    internals.executeOperation({
      kind: 'snapshot',
      operationType: 'applyCharFormatInHf',
      editContext: {
        mode: 'headerFooter', sectionIdx, isHeader, applyTo,
        paraIdx: cursor.hfParaIdx, charOffset: cursor.hfCharOffset,
      },
      operation: (wasm: EquationBridge) => {
        for (let paraIdx = selection.start.paraIdx; paraIdx <= selection.end.paraIdx; paraIdx += 1) {
          const info = JSON.parse(wasm.getHeaderFooterParaInfo(sectionIdx, isHeader, applyTo, paraIdx)) as { charCount?: number };
          const charCount = Number.isSafeInteger(info.charCount) ? Number(info.charCount) : 0;
          const from = paraIdx === selection.start.paraIdx ? selection.start.charOffset : 0;
          const to = paraIdx === selection.end.paraIdx ? selection.end.charOffset : charCount;
          if (to > from) {
            wasm.applyCharFormatInHf({ sectionIdx, isHeader, applyTo, paraIdx }, from, to, props);
          }
        }
        return { ...cursorBefore };
      },
    });
    this.renderHeaderFooterSelection();
  }

  private pagePoint(event: MouseEvent): PagePoint | null {
    const scrollContent = this.hopContainer.querySelector('#scroll-content');
    if (!(scrollContent instanceof HTMLElement)) return null;
    const rect = scrollContent.getBoundingClientRect();
    const contentX = event.clientX - rect.left;
    const contentY = event.clientY - rect.top;
    const pageIndex = this.hopVirtualScroll.getPageAtPoint(contentX, contentY);
    if (pageIndex < 0) return null;
    const zoom = this.hopViewportManager.getZoom();
    return {
      pageIndex,
      pageX: (contentX - this.hopVirtualScroll.getPageLeftResolved(pageIndex, scrollContent.clientWidth)) / zoom,
      pageY: (contentY - this.hopVirtualScroll.getPageOffset(pageIndex)) / zoom,
    };
  }

  private resolveHeaderFooterRegion(point: PagePoint): { isHeader: boolean; sectionIndex: number; applyTo: number } | null {
    try {
      const exact = this.hopWasm.hitTestHeaderFooter(point.pageIndex, point.pageX, point.pageY);
      if (exact.hit && exact.isHeader !== undefined) {
        const target = this.hopWasm.getHeaderFooterEditTarget(point.pageIndex, exact.isHeader);
        return {
          isHeader: exact.isHeader,
          sectionIndex: exact.sectionIndex ?? target.sectionIndex,
          applyTo: exact.applyTo ?? target.applyTo,
        };
      }
      const mode = headerFooterRegionAtPoint(this.hopWasm.getPageInfo(point.pageIndex), point.pageX, point.pageY);
      if (!mode) return null;
      const isHeader = mode === 'header';
      const target = this.hopWasm.getHeaderFooterEditTarget(point.pageIndex, isHeader);
      return { isHeader, sectionIndex: target.sectionIndex, applyTo: target.applyTo };
    } catch {
      return null;
    }
  }

  private onHeaderFooterDoubleClickCapture(event: MouseEvent): void {
    if (event.button !== 0) return;
    const internals = this as unknown as InternalHandler;
    if (!internals.active) return;
    const point = this.pagePoint(event);
    if (!point) return;
    const region = this.resolveHeaderFooterRegion(point);
    if (!region) return;

    event.preventDefault();
    event.stopImmediatePropagation();
    try {
      const existing = JSON.parse(this.hopWasm.getHeaderFooter(region.sectionIndex, region.isHeader, region.applyTo)) as { exists?: boolean };
      if (!existing.exists) this.hopWasm.createHeaderFooter(region.sectionIndex, region.isHeader, region.applyTo);
      internals.cursor.enterHeaderFooterMode(region.isHeader, region.sectionIndex, region.applyTo, point.pageIndex);
      const hit = this.hopWasm.hitTestInHeaderFooter(point.pageIndex, region.isHeader, point.pageX, point.pageY);
      if (hit.hit && Number.isInteger(hit.paraIndex) && Number.isInteger(hit.charOffset)) {
        internals.cursor.setHfCursorPosition(hit.paraIndex as number, hit.charOffset as number);
      }
      this.clearHeaderFooterSelection(true);
      this.hopEventBus.emit('headerFooterModeChanged', region.isHeader ? 'header' : 'footer');
      internals.updateCaret();
      internals.emitCursorFormatState();
      internals.textarea.focus();
    } catch (error) {
      console.warn('[HOP] 머리말/꼬리말 더블클릭 편집 진입 실패:', error);
    }
  }

  private onHeaderFooterMouseDownCapture(event: MouseEvent): void {
    if (event.button !== 0) return;
    const internals = this as unknown as InternalHandler;
    const cursor = internals.cursor;
    if (!cursor.isInHeaderFooter()) return;
    const point = this.pagePoint(event);
    if (!point) return;
    const region = this.resolveHeaderFooterRegion(point);
    const isHeader = cursor.headerFooterMode === 'header';
    if (!region) return;
    // A real double-click first dispatches ordinary mouse events. When the
    // user switches directly between the header and footer bands, keep that
    // first click from making upstream leave HF mode before our dblclick
    // capture can enter the opposite band.
    if (region.isHeader !== isHeader) {
      event.preventDefault();
      event.stopImmediatePropagation();
      return;
    }

    try {
      const hit = this.hopWasm.hitTestInHeaderFooter(point.pageIndex, isHeader, point.pageX, point.pageY);
      if (!hit.hit || !Number.isInteger(hit.paraIndex) || !Number.isInteger(hit.charOffset)) return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const paraIdx = hit.paraIndex as number;
      const charOffset = hit.charOffset as number;
      const focus: HeaderFooterSelectionPoint = { pageIndex: point.pageIndex, paraIdx, charOffset };
      this.hfSelectionAnchor = event.shiftKey && this.hfSelectionAnchor ? this.hfSelectionAnchor : focus;
      this.hfSelectionFocus = focus;
      this.hfSelectionDragging = true;
      this.hopWasm.clearHeaderFooterPendingCharFormat();
      cursor.setHfCursorPosition(paraIdx, charOffset);
      internals.updateCaret();
      this.renderHeaderFooterSelection();
      internals.emitCursorFormatState();
      internals.textarea.focus();
    } catch {
      // 실패하면 upstream mousedown 처리에 맡긴다.
    }
  }

  private onHeaderFooterMouseMoveCapture(event: MouseEvent): void {
    if (!this.hfSelectionDragging || !this.hfSelectionAnchor) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    const internals = this as unknown as InternalHandler;
    const cursor = internals.cursor;
    if (!cursor.isInHeaderFooter()) return;
    const point = this.pagePoint(event);
    if (!point || point.pageIndex !== this.hfSelectionAnchor.pageIndex) return;
    try {
      const hit = this.hopWasm.hitTestInHeaderFooter(
        point.pageIndex,
        cursor.headerFooterMode === 'header',
        point.pageX,
        point.pageY,
      );
      if (!hit.hit || !Number.isInteger(hit.paraIndex) || !Number.isInteger(hit.charOffset)) return;
      const paraIdx = hit.paraIndex as number;
      const charOffset = hit.charOffset as number;
      this.hfSelectionFocus = { pageIndex: point.pageIndex, paraIdx, charOffset };
      cursor.setHfCursorPosition(paraIdx, charOffset);
      internals.updateCaret();
      this.renderHeaderFooterSelection();
      internals.emitCursorFormatState();
    } catch {
      // 마지막 유효 focus를 유지한다.
    }
  }

  private onHeaderFooterMouseUpCapture(event: MouseEvent): void {
    if (!this.hfSelectionDragging) return;
    event.preventDefault();
    event.stopImmediatePropagation();
    this.hfSelectionDragging = false;
    const internals = this as unknown as InternalHandler;
    internals.updateCaret();
    this.renderHeaderFooterSelection();
    internals.emitCursorFormatState();
    internals.textarea.focus();
  }

  private renderHeaderFooterSelection(): void {
    const internals = this as unknown as InternalHandler;
    const selection = this.getHeaderFooterSelection();
    if (!selection) {
      internals.selectionRenderer.clear();
      return;
    }
    try {
      const rects = this.hopWasm.getSelectionRectsInHeaderFooter(
        selection.start.pageIndex,
        internals.cursor.headerFooterMode === 'header',
        selection.start.paraIdx,
        selection.start.charOffset,
        selection.end.paraIdx,
        selection.end.charOffset,
      );
      internals.selectionRenderer.render(rects, this.hopViewportManager.getZoom());
    } catch {
      internals.selectionRenderer.clear();
    }
  }

  private clearHeaderFooterSelection(clearPending: boolean): void {
    this.hfSelectionAnchor = null;
    this.hfSelectionFocus = null;
    this.hfSelectionDragging = false;
    (this as unknown as InternalHandler).selectionRenderer.clear();
    if (clearPending) this.hopWasm.clearHeaderFooterPendingCharFormat();
  }
}
