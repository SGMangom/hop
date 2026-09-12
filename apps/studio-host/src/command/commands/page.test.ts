import { describe, expect, it, vi } from 'vitest';
import { pageCommands } from './page';

function command(id: string) {
  return pageCommands.find((item) => item.id === id)!;
}

describe('two-column preset HOP overrides', () => {
  it.each([
    ['page:col-left', true],
    ['page:col-right', false],
  ] as const)('routes %s through the unequal-width engine preset', (id, narrowLeft) => {
    const position = { sectionIndex: 2, paragraphIndex: 4, charOffset: 1 };
    const setTwoColumnPreset = vi.fn(() => ({
      ok: true,
      leftWidth: narrowLeft ? 100 : 200,
      rightWidth: narrowLeft ? 200 : 100,
      spacing: 2268,
    }));
    const executeOperation = vi.fn((descriptor: {
      operation: (wasm: { setTwoColumnPreset: typeof setTwoColumnPreset }) => unknown;
    }) => {
      descriptor.operation({ setTwoColumnPreset });
    });
    const inputHandler = {
      getPosition: vi.fn(() => position),
      executeOperation,
    };

    command(id).execute({ getInputHandler: () => inputHandler } as never);

    expect(executeOperation).toHaveBeenCalledWith(expect.objectContaining({
      kind: 'snapshot',
      operationType: 'setColumnDef',
      meta: { actionId: id, domain: 'page', refresh: 'full', dirtyScope: 'document' },
    }));
    expect(setTwoColumnPreset).toHaveBeenCalledWith(2, narrowLeft, 2268);
  });
});
