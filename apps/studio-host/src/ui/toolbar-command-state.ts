type CommandStateEventBus = {
  on(event: string, listener: () => void): void;
};

type CommandStateDispatcher = {
  isEnabled(commandId: string): boolean;
};

/**
 * Keep the icon/header-footer toolbars visually aligned with command canExecute.
 *
 * CommandDispatcher already rejects disabled commands, but a normal `<button>`
 * remained visually clickable because only menu items refreshed their state.
 * This helper updates the actual disabled property so mouse, keyboard focus and
 * accessibility state all tell the same truth.
 */
export function syncToolbarCommandStates(
  root: ParentNode,
  dispatcher: CommandStateDispatcher,
): void {
  root.querySelectorAll<HTMLButtonElement>(
    '#icon-toolbar .tb-btn[data-cmd], #hf-toolbar .tb-btn[data-cmd]',
  ).forEach((button) => {
    const commandId = button.dataset.cmd;
    if (!commandId) return;
    const enabled = dispatcher.isEnabled(commandId);
    button.disabled = !enabled;
    button.setAttribute('aria-disabled', enabled ? 'false' : 'true');
  });
}

export function installToolbarCommandStateSync(
  root: ParentNode,
  eventBus: CommandStateEventBus,
  dispatcher: CommandStateDispatcher,
): () => void {
  const refresh = () => syncToolbarCommandStates(root, dispatcher);
  eventBus.on('command-state-changed', refresh);
  refresh();
  return refresh;
}
