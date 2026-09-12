import { describe, expect, it, vi } from 'vitest';
import { documentInfoCommands, installDocumentInfoMenuEntry } from './document-info';

describe('document info command contribution', () => {
  it('is available only while a document is loaded', () => {
    const command = documentInfoCommands[0];
    expect(command.id).toBe('file:document-info');
    expect(command.canExecute?.({ hasDocument: true } as never)).toBe(true);
    expect(command.canExecute?.({ hasDocument: false } as never)).toBe(false);
  });

  it('adds one delegated file-menu entry before product info', () => {
    const about = fakeElement();
    const menu = {
      existing: null as unknown,
      querySelector: vi.fn((selector: string) => {
        if (selector === '[data-cmd="file:document-info"]') return menu.existing;
        if (selector === '[data-cmd="file:about"]') return about;
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

    installDocumentInfoMenuEntry(doc as unknown as Document);
    installDocumentInfoMenuEntry(doc as unknown as Document);

    expect(menu.insertBefore).toHaveBeenCalledTimes(1);
    const item = created[0];
    expect(item.className).toBe('md-item');
    expect(item.dataset.cmd).toBe('file:document-info');
    expect(item.children[1]?.textContent).toBe('문서 정보');
    expect(menu.insertBefore).toHaveBeenCalledWith(item, about);
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
  return {
    className: '',
    dataset: {},
    textContent: '',
    children: [],
    append(...children: FakeElement[]) {
      this.children.push(...children);
    },
  };
}
