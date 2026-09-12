import { describe, expect, it, vi } from 'vitest';
import { installToolbarCommandStateSync, syncToolbarCommandStates } from './toolbar-command-state';

describe('toolbar command state', () => {
  it('disables toolbar buttons whose commands cannot execute', () => {
    const copy = fakeButton('edit:copy');
    const image = fakeButton('insert:image');
    const root = fakeRoot(copy, image);
    const dispatcher = {
      isEnabled: vi.fn((id: string) => id === 'edit:copy'),
    };

    syncToolbarCommandStates(root, dispatcher);

    expect(copy.disabled).toBe(false);
    expect(copy.attributes.get('aria-disabled')).toBe('false');
    expect(image.disabled).toBe(true);
    expect(image.attributes.get('aria-disabled')).toBe('true');
  });

  it('refreshes when command state changes', () => {
    const button = fakeButton('page:headerfooter-close');
    const root = fakeRoot(button);
    let enabled = false;
    let listener: (() => void) | undefined;
    const eventBus = {
      on: vi.fn((_event: string, next: () => void) => { listener = next; }),
    };
    const dispatcher = { isEnabled: vi.fn(() => enabled) };

    installToolbarCommandStateSync(root, eventBus, dispatcher);
    expect(button.disabled).toBe(true);

    enabled = true;
    listener?.();
    expect(button.disabled).toBe(false);
  });
});

function fakeButton(commandId: string) {
  return {
    dataset: { cmd: commandId },
    disabled: false,
    attributes: new Map<string, string>(),
    setAttribute(name: string, value: string) {
      this.attributes.set(name, value);
    },
  };
}

function fakeRoot(...buttons: ReturnType<typeof fakeButton>[]) {
  return {
    querySelectorAll: () => buttons,
  } as unknown as ParentNode;
}
