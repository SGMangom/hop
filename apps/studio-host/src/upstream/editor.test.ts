import { describe, expect, it } from 'vitest';
import { InputHandler as LocalInputHandler } from '@/engine/input-handler';
import { InputHandler as ExportedInputHandler } from './editor';

describe('HOP editor override wiring', () => {
  it('exports the local extended InputHandler used by main.ts', () => {
    expect(ExportedInputHandler).toBe(LocalInputHandler);
  });
});
