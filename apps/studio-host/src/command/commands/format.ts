import { formatCommands as upstreamFormatCommands } from '@/upstream/commands';
import type { CommandDef } from '@/upstream/commands';
import { CharShapeDialog } from '@/upstream/ui';
import { replaceUpstreamCommands } from '../replace-upstream-commands';

const upstreamCharShape = upstreamFormatCommands.find((command) => command.id === 'format:char-shape');
if (!upstreamCharShape) throw new Error('Missing upstream format:char-shape command');

/**
 * 한컴처럼 선택이 없을 때도 글자 모양 대화상자를 열고, 확인한 속성을 다음 입력 런에 예약한다.
 * upstream InputHandler의 applyCharFormat이 실제 선택/캐럿 대기 서식의 단일 구현이다.
 */
const charShapeCommand: CommandDef = {
  ...upstreamCharShape,
  execute(services) {
    const inputHandler = services.getInputHandler();
    if (!inputHandler) return;

    const charProps = inputHandler.getCharProperties();
    const savedSelection = inputHandler.hasSelection() ? inputHandler.getSelection() : null;
    const dialog = new CharShapeDialog(services.wasm, services.eventBus);
    dialog.onApply = (mods) => {
      if (mods.fontName) {
        const fontId = services.wasm.findOrCreateFontId(mods.fontName);
        if (fontId >= 0) mods.fontId = fontId;
        delete mods.fontName;
      }

      if (savedSelection) {
        inputHandler.applyCharPropsToRange(savedSelection.start, savedSelection.end, mods);
      } else {
        const applyAtCaret = (inputHandler as unknown as {
          applyCharFormat?: (props: typeof mods) => void;
        }).applyCharFormat;
        if (typeof applyAtCaret !== 'function') {
          throw new Error('현재 편집기는 캐럿 글자 모양 예약을 지원하지 않습니다.');
        }
        applyAtCaret.call(inputHandler, mods);
      }
    };
    dialog.onClose = () => inputHandler.focus();
    dialog.show(charProps);
  },
};

export const formatCommands: CommandDef[] = replaceUpstreamCommands(
  upstreamFormatCommands,
  [charShapeCommand],
);
