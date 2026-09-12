import { pageCommands as upstreamPageCommands } from '@/upstream/commands';
import type { CommandDef } from '@/upstream/commands';
import type { TwoColumnPresetResult } from '../../core/equation-bridge';
import { replaceUpstreamCommands } from '../replace-upstream-commands';

const TWO_COLUMN_SPACING_HU = 2268;

type TwoColumnPresetBridge = {
  setTwoColumnPreset(
    sectionIndex: number,
    narrowLeft: boolean,
    spacingHu: number,
  ): TwoColumnPresetResult;
};

function presetCommand(id: 'page:col-left' | 'page:col-right', narrowLeft: boolean): CommandDef {
  const upstream = upstreamPageCommands.find((command) => command.id === id);
  if (!upstream) throw new Error(`Missing upstream ${id} command`);
  return {
    ...upstream,
    execute(services) {
      const inputHandler = services.getInputHandler();
      if (!inputHandler) return;
      const position = inputHandler.getPosition();
      inputHandler.executeOperation({
        kind: 'snapshot',
        operationType: 'setColumnDef',
        operation: (wasm) => {
          const bridge = wasm as unknown as Partial<TwoColumnPresetBridge>;
          if (typeof bridge.setTwoColumnPreset !== 'function') {
            throw new Error('2단 너비 프리셋 API를 사용할 수 없습니다.');
          }
          const result = bridge.setTwoColumnPreset(
            position.sectionIndex,
            narrowLeft,
            TWO_COLUMN_SPACING_HU,
          );
          if (!result.ok) throw new Error(`${id} 적용에 실패했습니다.`);
          return position;
        },
        meta: { actionId: id, domain: 'page', refresh: 'full', dirtyScope: 'document' },
      });
    },
  };
}

export const pageCommands: CommandDef[] = replaceUpstreamCommands(
  upstreamPageCommands,
  [
    presetCommand('page:col-left', true),
    presetCommand('page:col-right', false),
  ],
);
