import type { CommandDef } from '@/upstream/commands';
import { DocumentInfoDialog, readDocumentInfoSnapshot } from '../../ui/document-info-dialog';

export const documentInfoCommands: CommandDef[] = [
  {
    id: 'file:document-info',
    label: '문서 정보',
    canExecute: (ctx) => ctx.hasDocument,
    execute(services) {
      new DocumentInfoDialog(readDocumentInfoSnapshot(services)).show();
    },
  },
];

/**
 * index.html을 직접 소유하지 않고 기존 파일 메뉴에 HOP 전용 진입점만 동적으로 추가한다.
 * MenuBar는 하위 항목 클릭을 delegation으로 처리하므로 초기화 전에 한 번 넣으면 충분하다.
 */
export function installDocumentInfoMenuEntry(doc: Document = document): void {
  if (!doc || typeof doc.querySelector !== 'function' || typeof doc.createElement !== 'function') return;
  const menu = doc.querySelector('.menu-item[data-menu="file"] .menu-dropdown');
  if (!menu || menu.querySelector('[data-cmd="file:document-info"]')) return;

  const item = doc.createElement('div');
  item.className = 'md-item';
  item.dataset.cmd = 'file:document-info';

  const icon = doc.createElement('span');
  icon.className = 'md-icon';
  const label = doc.createElement('span');
  label.className = 'md-label';
  label.textContent = '문서 정보';
  item.append(icon, label);

  const about = menu.querySelector('[data-cmd="file:about"]');
  if (about) menu.insertBefore(item, about);
  else menu.appendChild(item);
}
