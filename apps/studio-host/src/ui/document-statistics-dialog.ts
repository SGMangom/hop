import type { CommandServices } from '@/upstream/commands';
import type { DocumentStatistics } from '../core/equation-bridge';
import { ModalDialog } from './dialog';

export function readDocumentStatisticsSnapshot(services: CommandServices): DocumentStatistics {
  const read = (services.wasm as unknown as {
    getDocumentStatistics?: () => DocumentStatistics;
  }).getDocumentStatistics;
  if (typeof read !== 'function') throw new Error('문서 통계 API를 사용할 수 없습니다.');
  return read.call(services.wasm);
}

export class DocumentStatisticsDialog extends ModalDialog {
  constructor(private readonly statistics: DocumentStatistics) {
    super('문서 통계', 430);
  }

  protected createBody(): HTMLElement {
    const body = document.createElement('div');
    const section = document.createElement('div');
    section.className = 'dialog-section';

    const heading = document.createElement('div');
    heading.className = 'dialog-section-title';
    heading.textContent = '문서 분량';
    section.appendChild(heading);

    for (const [labelText, value] of [
      ['문단', this.statistics.paragraphCount],
      ['글자 (공백 포함)', this.statistics.characterCountWithSpaces],
      ['글자 (공백 제외)', this.statistics.characterCountWithoutSpaces],
      ['단어 (어림)', this.statistics.wordCount],
    ] as const) {
      const row = document.createElement('div');
      row.className = 'dialog-row';
      const label = document.createElement('span');
      label.className = 'dialog-label';
      label.style.flex = '0 0 130px';
      label.textContent = labelText;
      const count = document.createElement('span');
      count.style.userSelect = 'text';
      count.textContent = value.toLocaleString('ko-KR');
      row.append(label, count);
      section.appendChild(row);
    }

    const note = document.createElement('div');
    note.style.marginTop = '10px';
    note.style.color = 'var(--color-text-muted)';
    note.style.fontSize = '12px';
    note.textContent = '단어 수는 공백을 기준으로 계산한 어림값입니다.';
    section.appendChild(note);
    body.appendChild(section);
    return body;
  }

  protected onConfirm(): void {
    // 읽기 전용 대화상자.
  }
}
