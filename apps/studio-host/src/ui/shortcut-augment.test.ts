import { describe, expect, it, vi } from 'vitest';
import { SupplementalShortcutHandler } from './shortcut-augment';

describe('supplemental command shortcuts', () => {
  it.each([
    ['h', 'insert:hyperlink'],
    ['e', 'insert:field'],
    ['b', 'insert:bookmark'],
    ['n', 'format:para-num-shape'],
  ])('routes Ctrl+K then %s to %s', (secondKey, commandId) => {
    const dispatcher = fakeDispatcher();
    const handler = new SupplementalShortcutHandler(dispatcher);

    expect(handler.handle(keyEvent('k', { ctrlKey: true }))).toBe(true);
    expect(handler.handle(keyEvent(secondKey))).toBe(true);
    expect(dispatcher.dispatch).toHaveBeenCalledWith(commandId);
  });

  it('lets an unknown Ctrl+K second key continue normally and clears the chord', () => {
    const dispatcher = fakeDispatcher();
    const handler = new SupplementalShortcutHandler(dispatcher);

    handler.handle(keyEvent('k', { ctrlKey: true }));
    expect(handler.handle(keyEvent('x'))).toBe(false);
    expect(handler.handle(keyEvent('h'))).toBe(false);
    expect(dispatcher.dispatch).not.toHaveBeenCalled();
  });

  it('uses Ctrl+Shift+S for Save As when table block sum is unavailable', () => {
    const dispatcher = fakeDispatcher(false);
    const handler = new SupplementalShortcutHandler(dispatcher);

    expect(handler.handle(keyEvent('s', { ctrlKey: true, shiftKey: true }))).toBe(true);
    expect(dispatcher.dispatch).toHaveBeenCalledWith('file:save-as');
  });

  it('routes Ctrl+Shift+S to table block sum when that command is enabled', () => {
    const dispatcher = fakeDispatcher(true);
    const handler = new SupplementalShortcutHandler(dispatcher);

    expect(handler.handle(keyEvent('s', { ctrlKey: true, shiftKey: true }))).toBe(true);
    expect(dispatcher.dispatch).toHaveBeenCalledWith('table:block-sum');
  });
});

function fakeDispatcher(blockSumEnabled = false) {
  return {
    dispatch: vi.fn(() => true),
    isEnabled: vi.fn((id: string) => id === 'table:block-sum' && blockSumEnabled),
  };
}

function keyEvent(
  key: string,
  overrides: Partial<KeyboardLike> = {},
): KeyboardLike {
  return {
    key,
    ctrlKey: false,
    metaKey: false,
    shiftKey: false,
    altKey: false,
    target: null,
    preventDefault: vi.fn(),
    stopPropagation: vi.fn(),
    ...overrides,
  };
}

type KeyboardLike = Pick<KeyboardEvent,
  'key' | 'ctrlKey' | 'metaKey' | 'shiftKey' | 'altKey' | 'target' | 'preventDefault' | 'stopPropagation'>;
