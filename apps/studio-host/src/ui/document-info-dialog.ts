import type { CommandServices } from '@/upstream/commands';
import type { DocumentInfo } from '@/upstream/core';
import { ModalDialog } from './dialog';

export interface DocumentInfoSnapshot {
  fileName: string;
  sourcePath: string | null;
  sourceFormat: string;
  version: string;
  pageCount: number;
  sectionCount: number;
  encrypted: boolean;
  fallbackFont: string;
  fontsUsed: string[];
  dirty: boolean;
}

/**
 * 현재 편집 세션에서 공개 API로 확인 가능한 객관 정보만 수집한다.
 * title/author/subject는 안정적인 공용 engine API가 없으므로 추측하지 않는다.
 */
export function readDocumentInfoSnapshot(services: CommandServices): DocumentInfoSnapshot {
  const info: DocumentInfo = services.wasm.getDocumentInfo();
  const context = services.getContext();
  const getSourcePath = (services.wasm as unknown as { getSourcePath?: () => string | null }).getSourcePath;
  return {
    fileName: services.wasm.fileName || '문서',
    sourcePath: typeof getSourcePath === 'function' ? getSourcePath.call(services.wasm) : null,
    sourceFormat: formatSourceFormat(context.sourceFormat ?? services.wasm.getSourceFormat()),
    version: info.version || '-',
    pageCount: info.pageCount,
    sectionCount: info.sectionCount,
    encrypted: info.encrypted,
    fallbackFont: info.fallbackFont || '-',
    fontsUsed: [...(info.fontsUsed ?? [])],
    dirty: context.isDirty,
  };
}

export class DocumentInfoDialog extends ModalDialog {
  constructor(private readonly snapshot: DocumentInfoSnapshot) {
    super('문서 정보', 520);
  }

  protected createBody(): HTMLElement {
    const body = document.createElement('div');
    body.append(
      this.buildSection('문서', [
        ['파일 이름', this.snapshot.fileName],
        ['파일 위치', this.snapshot.sourcePath ?? '저장되지 않은 문서'],
        ['파일 형식', this.snapshot.sourceFormat],
        ['문서 버전', this.snapshot.version],
        ['쪽', `${this.snapshot.pageCount}쪽`],
        ['구역', `${this.snapshot.sectionCount}개`],
        ['수정 상태', this.snapshot.dirty ? '저장되지 않은 변경 사항 있음' : '저장됨'],
        ['암호화', this.snapshot.encrypted ? '암호화 문서' : '암호화되지 않음'],
      ]),
      this.buildFontSection(),
    );
    return body;
  }

  protected onConfirm(): void {
    // 읽기 전용 정보 대화상자 — 확인 시 닫기만 한다.
  }

  private buildSection(title: string, rows: Array<[string, string]>): HTMLElement {
    const section = document.createElement('div');
    section.className = 'dialog-section';

    const heading = document.createElement('div');
    heading.className = 'dialog-section-title';
    heading.textContent = title;
    section.appendChild(heading);

    for (const [label, value] of rows) {
      section.appendChild(this.buildInfoRow(label, value));
    }
    return section;
  }

  private buildInfoRow(labelText: string, valueText: string): HTMLElement {
    const row = document.createElement('div');
    row.className = 'dialog-row';
    row.style.alignItems = 'flex-start';

    const label = document.createElement('span');
    label.className = 'dialog-label';
    label.style.flex = '0 0 92px';
    label.textContent = labelText;

    const value = document.createElement('span');
    value.style.flex = '1';
    value.style.minWidth = '0';
    value.style.userSelect = 'text';
    value.style.overflowWrap = 'anywhere';
    value.textContent = valueText;

    row.append(label, value);
    return row;
  }

  private buildFontSection(): HTMLElement {
    const section = document.createElement('div');
    section.className = 'dialog-section';

    const heading = document.createElement('div');
    heading.className = 'dialog-section-title';
    heading.textContent = `글꼴 (${this.snapshot.fontsUsed.length})`;
    section.appendChild(heading);
    section.appendChild(this.buildInfoRow('대체 글꼴', this.snapshot.fallbackFont));

    const fonts = document.createElement('div');
    fonts.style.marginTop = '6px';
    fonts.style.maxHeight = '150px';
    fonts.style.overflow = 'auto';
    fonts.style.padding = '8px 10px';
    fonts.style.border = '1px solid var(--color-border)';
    fonts.style.background = 'var(--color-surface)';
    fonts.style.userSelect = 'text';
    fonts.textContent = this.snapshot.fontsUsed.length > 0
      ? this.snapshot.fontsUsed.join(', ')
      : '문서에서 사용된 글꼴 정보가 없습니다.';
    section.appendChild(fonts);
    return section;
  }
}

function formatSourceFormat(format: string | undefined): string {
  switch ((format ?? '').toLowerCase()) {
    case 'hwp': return 'HWP';
    case 'hwpx': return 'HWPX';
    case 'hml': return 'HML';
    default: return format ? format.toUpperCase() : '-';
  }
}
