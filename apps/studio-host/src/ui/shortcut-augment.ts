type ShortcutDispatcher = {
  dispatch(commandId: string): boolean;
  isEnabled(commandId: string): boolean;
};

type KeyboardLike = Pick<KeyboardEvent,
  'key' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey' | 'target' | 'preventDefault' | 'stopPropagation'>;

const HOP_CTRL_K_CHORDS: Record<string, string> = {
  b: 'insert:bookmark',
  'ㅠ': 'insert:bookmark',
  n: 'format:para-num-shape',
  'ㅜ': 'format:para-num-shape',
  h: 'insert:hyperlink',
  'ㅗ': 'insert:hyperlink',
  e: 'insert:field',
  'ㄷ': 'insert:field',
};

export class SupplementalShortcutHandler {
  private pendingCtrlK = false;

  constructor(private readonly dispatcher: ShortcutDispatcher) {}

  handle(event: KeyboardLike): boolean {
    if (isTextEntryTarget(event.target)) return false;

    const primary = event.ctrlKey || event.metaKey;
    const key = event.key.toLowerCase();

    if (this.pendingCtrlK) {
      this.pendingCtrlK = false;
      const commandId = HOP_CTRL_K_CHORDS[key];
      if (!commandId) return false;
      event.preventDefault();
      event.stopPropagation();
      this.dispatcher.dispatch(commandId);
      return true;
    }

    if (primary && !event.shiftKey && !event.altKey && (key === 'k' || key === 'ㅏ')) {
      // Own the complete Ctrl/Cmd+K chord family so the upstream editor does not
      // retain a second, stale pending-chord bit after a HOP-only H/E chord.
      this.pendingCtrlK = true;
      event.preventDefault();
      event.stopPropagation();
      return true;
    }

    if (primary && event.shiftKey && !event.altKey && key === 's') {
      // Ctrl/Cmd+Shift+S is context-sensitive in the editor: a valid table target uses
      // the block-sum command, otherwise it is Save As. Own the key in capture phase so
      // the static shortcut table cannot choose the wrong duplicate mapping afterward.
      const commandId = this.dispatcher.isEnabled('table:block-sum')
        ? 'table:block-sum'
        : 'file:save-as';
      event.preventDefault();
      event.stopPropagation();
      this.dispatcher.dispatch(commandId);
      return true;
    }

    return false;
  }
}

export function installSupplementalShortcuts(
  root: Document,
  dispatcher: ShortcutDispatcher,
): SupplementalShortcutHandler {
  const handler = new SupplementalShortcutHandler(dispatcher);
  root.addEventListener('keydown', (event) => handler.handle(event), true);
  return handler;
}

function isTextEntryTarget(target: EventTarget | null): boolean {
  const closest = (target as { closest?: (selector: string) => Element | null } | null)?.closest;
  if (typeof closest !== 'function') return false;
  if (closest.call(target, '.canvas-page, #editor-area, #scroll-container')) return false;
  return Boolean(closest.call(target, 'input, textarea, select, [contenteditable="true"]'));
}
