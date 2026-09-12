import { describe, expect, it, vi } from 'vitest';
import { documentStatisticsCommands, installDocumentStatisticsMenuEntry } from './document-statistics';

describe('document statistics command contribution', () => {
  it('is read-only and available only with a loaded document', () => {
    const command = documentStatisticsCommands[0];
    expect(command.id).toBe('tools:document-statistics');
    expect(command.canExecute?.({ hasDocument: true } as never)).toBe(true);
    expect(command.canExecute?.({ hasDocument: false } as never)).toBe(false);
  });

  it('adds one delegated tools-menu entry without editing index.html', () => {
    const options = fakeElement();
    const menu = {
      existing: null as unknown,
      querySelector: vi.fn((selector: string) => {
        if (selector === '[data-cmd="tools:document-statistics"]') return menu.existing;
        if (selector === '[data-cmd="tool:options"]') return options;
        return null;
      }),
      insertBefore: vi.fn((item: unknown) => { menu.existing = item; }),
      appendChild: vi.fn(),
    };
    const created: ReturnType<typeof fakeElement>[] = [];
    const doc = {
      querySelector: vi.fn(() => menu),
      createElement: vi.fn(() => {
        const element = fakeElement();
        created.push(element);
        return element;
      }),
    };

    installDocumentStatisticsMenuEntry(doc as unknown as Document);
    installDocumentStatisticsMenuEntry(doc as unknown as Document);

    expect(menu.insertBefore).toHaveBeenCalledTimes(1);
    expect(created[0].dataset.cmd).toBe('tools:document-statistics');
    expect(created[0].children[1]?.textContent).toBe('문서 통계');
    expect(menu.insertBefore).toHaveBeenCalledWith(created[0], options);
  });
});

interface FakeElement {
  className: string;
  dataset: Record<string, string>;
  textContent: string;
  children: FakeElement[];
  append(...children: FakeElement[]): void;
}
function fakeElement(): FakeElement {
  return { className: '', dataset: {}, textContent: '', children: [], append(...children) { this.children.push(...children); } };
}
