import { beforeEach, describe, expect, it, vi } from 'vitest';

const mocks = vi.hoisted(() => ({
  lastDialog: null as { onApply: ((values: { url: string; displayText: string }) => void) | null; show: ReturnType<typeof vi.fn> } | null,
  insertBodyHyperlink: vi.fn(),
}));

vi.mock('../../core/hyperlink-insert', () => ({
  insertBodyHyperlink: mocks.insertBodyHyperlink,
}));
vi.mock('../../ui/hyperlink-dialog', () => ({
  HyperlinkDialog: class {
    onApply: ((values: { url: string; displayText: string }) => void) | null = null;
    show = vi.fn();
    constructor() {
      mocks.lastDialog = this;
    }
  },
}));

import { insertCommands } from './insert';

const command = insertCommands.find((item) => item.id === 'insert:hyperlink')!;
const baseContext = {
  hasDocument: true,
  isEditable: true,
  isFormMode: false,
  inTable: false,
  inField: false,
  hasSelection: false,
};

describe('insert:hyperlink HOP override', () => {
  beforeEach(() => {
    mocks.lastDialog = null;
    mocks.insertBodyHyperlink.mockReset();
    mocks.insertBodyHyperlink.mockReturnValue({ ok: true, charOffset: 7, fieldId: 1 });
  });

  it('is explicitly body-only', () => {
    expect(command.canExecute?.(baseContext as never)).toBe(true);
    for (const blocked of [
      { inTable: true },
      { inField: true },
      { hasSelection: true },
      { isFormMode: true },
      { isEditable: false },
      { hasDocument: false },
    ]) {
      expect(command.canExecute?.({ ...baseContext, ...blocked } as never)).toBe(false);
    }
  });

  it('routes insertion through snapshot undo and moves after the display text', () => {
    const position = { sectionIndex: 0, paragraphIndex: 2, charOffset: 1 };
    const focus = vi.fn();
    const executeOperation = vi.fn((descriptor: { operation: (wasm: unknown) => unknown }) => {
      descriptor.operation({});
    });
    const inputHandler = {
      getPosition: vi.fn(() => position),
      executeOperation,
      focus,
    };
    const services = { getInputHandler: () => inputHandler } as never;

    command.execute(services);
    expect(mocks.lastDialog?.show).toHaveBeenCalledTimes(1);
    mocks.lastDialog?.onApply?.({
      url: 'https://example.com/guide',
      displayText: '사용 설명서',
    });

    expect(executeOperation).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'snapshot',
      operationType: 'insertHyperlink',
    }));
    expect(mocks.insertBodyHyperlink).toHaveBeenCalledWith(
      {},
      0,
      2,
      1,
      'https://example.com/guide',
      '사용 설명서',
    );
    expect(focus).toHaveBeenCalledTimes(1);
  });

  it('double-guards nested/cell positions even if dispatched directly', () => {
    const inputHandler = {
      getPosition: vi.fn(() => ({
        sectionIndex: 0,
        paragraphIndex: 1,
        charOffset: 0,
        parentParaIndex: 1,
        cellIndex: 0,
      })),
      executeOperation: vi.fn(),
      focus: vi.fn(),
    };

    command.execute({ getInputHandler: () => inputHandler } as never);
    expect(mocks.lastDialog).toBeNull();
    expect(inputHandler.executeOperation).not.toHaveBeenCalled();
  });
});

describe('insert command labels', () => {
  it('describes the implemented ClickHere field honestly and uses sequential chord notation', () => {
    const field = insertCommands.find((item) => item.id === 'insert:field')!;
    const hyperlink = insertCommands.find((item) => item.id === 'insert:hyperlink')!;
    expect(field.label).toBe('누름틀');
    expect(field.shortcutLabel).toBe('Ctrl+K,E');
    expect(hyperlink.shortcutLabel).toBe('Ctrl+K,H');
  });
});

describe('installHyperlinkToolbarEntry', () => {
  it('attaches insert:hyperlink to the existing toolbar icon', async () => {
    vi.resetModules();
    const { installHyperlinkToolbarEntry } = await import('./insert');
    const button = { dataset: {} as Record<string, string> };
    const icon = { closest: vi.fn(() => button) };
    const root = { querySelector: vi.fn(() => icon) } as unknown as ParentNode;

    installHyperlinkToolbarEntry(root);

    expect(button.dataset.cmd).toBe('insert:hyperlink');
    expect(icon.closest).toHaveBeenCalledWith('.tb-btn');
  });
});

describe('caption placement HOP overrides', () => {
  const captionContext = {
    hasDocument: true,
    inPictureObjectSelection: true,
    isEditable: true,
    isFormMode: false,
    selectedPictureType: 'image',
  };

  it('turns the selected picture caption on with the requested side/alignment through snapshot undo', () => {
    const command = insertCommands.find((item) => item.id === 'insert:caption-lm')!;
    expect(command.canExecute?.(captionContext as never)).toBe(true);

    const setPictureProperties = vi.fn(() => ({ ok: true }));
    const executeOperation = vi.fn((descriptor: { operation: (wasm: unknown) => unknown }) => {
      descriptor.operation({ setPictureProperties });
    });
    const inputHandler = {
      getSelectedPictureRef: vi.fn(() => ({ sec: 1, ppi: 2, ci: 3, type: 'image' })),
      getCursorPosition: vi.fn(() => ({ sectionIndex: 1, paragraphIndex: 2, charOffset: 0 })),
      executeOperation,
    };

    command.execute({ getInputHandler: () => inputHandler } as never);

    expect(executeOperation).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'snapshot',
      operationType: 'setObjectCaption',
    }));
    expect(setPictureProperties).toHaveBeenCalledWith(1, 2, 3, {
      hasCaption: true,
      captionDirection: 'Left',
      captionVertAlign: 'Center',
    });
  });

  it('removes an existing caption and preserves cell-path routing for shapes', () => {
    const command = insertCommands.find((item) => item.id === 'insert:caption-none')!;
    const cellPath = [{ controlIdx: 4, cellIdx: 1, cellParaIdx: 0 }];
    const setCellShapePropertiesByPath = vi.fn(() => ({ ok: true }));
    const executeOperation = vi.fn((descriptor: { operation: (wasm: unknown) => unknown }) => {
      descriptor.operation({ setCellShapePropertiesByPath });
    });
    const inputHandler = {
      getSelectedPictureRef: vi.fn(() => ({ sec: 0, ppi: 5, ci: 2, type: 'shape', cellPath })),
      getCursorPosition: vi.fn(() => ({ sectionIndex: 0, paragraphIndex: 5, charOffset: 0 })),
      executeOperation,
    };

    command.execute({ getInputHandler: () => inputHandler } as never);

    expect(setCellShapePropertiesByPath).toHaveBeenCalledWith(0, 5, cellPath, 2, { hasCaption: false });
  });

  it('keeps caption commands unavailable outside editable picture-object selection', () => {
    const command = insertCommands.find((item) => item.id === 'insert:caption-bottom')!;
    for (const blocked of [
      { hasDocument: false },
      { inPictureObjectSelection: false },
      { isEditable: false },
      { isFormMode: true },
    ]) {
      expect(command.canExecute?.({ ...captionContext, ...blocked } as never)).toBe(false);
    }
  });
});

describe('picture-object subtype command state', () => {
  const context = {
    hasDocument: true,
    inPictureObjectSelection: true,
    isEditable: true,
    isFormMode: false,
    selectedPictureType: 'image',
    selectedPictureTypes: ['image'],
    selectedPictureCount: 1,
  };

  it('does not enable commands that would immediately no-op for the selected subtype', () => {
    const arrange = insertCommands.find((item) => item.id === 'insert:arrange-front')!;
    const ungroup = insertCommands.find((item) => item.id === 'insert:ungroup-shapes')!;
    const equationEdit = insertCommands.find((item) => item.id === 'insert:equation-edit')!;
    const rotate = insertCommands.find((item) => item.id === 'insert:rotate-cw')!;

    expect(arrange.canExecute?.(context as never)).toBe(false);
    expect(arrange.canExecute?.({ ...context, selectedPictureType: 'shape' } as never)).toBe(true);
    expect(ungroup.canExecute?.(context as never)).toBe(false);
    expect(ungroup.canExecute?.({ ...context, selectedPictureType: 'group' } as never)).toBe(true);
    expect(equationEdit.canExecute?.(context as never)).toBe(false);
    expect(equationEdit.canExecute?.({ ...context, selectedPictureType: 'equation' } as never)).toBe(true);
    expect(rotate.canExecute?.({ ...context, selectedPictureType: 'equation' } as never)).toBe(false);
    expect(rotate.canExecute?.(context as never)).toBe(true);
  });

  it('requires at least two groupable shapes for object grouping', () => {
    const group = insertCommands.find((item) => item.id === 'insert:group-shapes')!;
    expect(group.canExecute?.(context as never)).toBe(false);
    expect(group.canExecute?.({
      ...context,
      selectedPictureType: 'shape',
      selectedPictureTypes: ['shape', 'line'],
      selectedPictureCount: 2,
    } as never)).toBe(true);
    expect(group.canExecute?.({
      ...context,
      selectedPictureTypes: ['shape', 'image'],
      selectedPictureCount: 2,
    } as never)).toBe(false);
  });
});
