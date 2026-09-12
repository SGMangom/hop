import { describe, expect, it, vi } from 'vitest';

const dialogState = vi.hoisted(() => ({
  instance: null as null | { onApply: ((mods: Record<string, unknown>) => void) | null },
}));

vi.mock('@/upstream/ui', () => ({
  CharShapeDialog: class {
    onApply: ((mods: Record<string, unknown>) => void) | null = null;
    onClose: (() => void) | null = null;
    constructor() { dialogState.instance = this; }
    show = vi.fn();
  },
}));

vi.mock('@/upstream/commands', () => ({
  formatCommands: [{
    id: 'format:char-shape',
    label: '글자 모양',
    canExecute: () => true,
    execute: vi.fn(),
  }],
}));

import { formatCommands } from './format';

describe('format:char-shape HOP override', () => {
  it('stages character properties at the caret when no range is selected', () => {
    const command = formatCommands.find((item) => item.id === 'format:char-shape')!;
    const applyCharFormat = vi.fn();
    const inputHandler = {
      getCharProperties: vi.fn(() => ({ bold: false })),
      hasSelection: vi.fn(() => false),
      getSelection: vi.fn(() => null),
      applyCharFormat,
      focus: vi.fn(),
    };
    const wasm = { findOrCreateFontId: vi.fn(() => 11) };

    command.execute({ getInputHandler: () => inputHandler, wasm, eventBus: {} } as never);
    dialogState.instance!.onApply!({ bold: true, fontName: '함초롬바탕' });

    expect(applyCharFormat).toHaveBeenCalledWith({ bold: true, fontId: 11 });
  });

  it('keeps the originally selected range as the apply target', () => {
    const command = formatCommands.find((item) => item.id === 'format:char-shape')!;
    const selection = {
      start: { sectionIndex: 0, paragraphIndex: 1, charOffset: 2 },
      end: { sectionIndex: 0, paragraphIndex: 1, charOffset: 5 },
    };
    const applyCharPropsToRange = vi.fn();
    const inputHandler = {
      getCharProperties: vi.fn(() => ({})),
      hasSelection: vi.fn(() => true),
      getSelection: vi.fn(() => selection),
      applyCharPropsToRange,
      focus: vi.fn(),
    };

    command.execute({
      getInputHandler: () => inputHandler,
      wasm: { findOrCreateFontId: vi.fn(() => 0) },
      eventBus: {},
    } as never);
    dialogState.instance!.onApply!({ italic: true });

    expect(applyCharPropsToRange).toHaveBeenCalledWith(selection.start, selection.end, { italic: true });
  });
});
