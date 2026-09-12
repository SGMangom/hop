import type { CommandDef } from '@/upstream/commands';
import {
  DocumentStatisticsDialog,
  readDocumentStatisticsSnapshot,
} from '../../ui/document-statistics-dialog';

export const documentStatisticsCommands: CommandDef[] = [
  {
    id: 'tools:document-statistics',
    label: '문서 통계',
    canExecute: (ctx) => ctx.hasDocument,
    execute(services) {
      new DocumentStatisticsDialog(readDocumentStatisticsSnapshot(services)).show();
    },
  },
];

/** index.html을 직접 수정하지 않고 도구 메뉴에 HOP 전용 통계 진입점을 추가한다. */
export function installDocumentStatisticsMenuEntry(doc: Document = document): void {
  if (!doc || typeof doc.querySelector !== 'function' || typeof doc.createElement !== 'function') return;
  const menu = doc.querySelector('.menu-item[data-menu="tool"] .menu-dropdown');
  if (!menu || menu.querySelector('[data-cmd="tools:document-statistics"]')) return;

  const item = doc.createElement('div');
  item.className = 'md-item';
  item.dataset.cmd = 'tools:document-statistics';
  const icon = doc.createElement('span');
  icon.className = 'md-icon';
  const label = doc.createElement('span');
  label.className = 'md-label';
  label.textContent = '문서 통계';
  item.append(icon, label);

  const options = menu.querySelector('[data-cmd="tool:options"]');
  if (options) menu.insertBefore(item, options);
  else menu.appendChild(item);
}
