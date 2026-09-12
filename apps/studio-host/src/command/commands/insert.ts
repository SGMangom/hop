import { insertCommands as upstreamInsertCommands } from '@/upstream/commands';
import type { CommandDef, EditorContext } from '@/upstream/commands';
import { insertBodyHyperlink } from '../../core/hyperlink-insert';
import { HyperlinkDialog } from '../../ui/hyperlink-dialog';
import { replaceUpstreamCommands } from '../replace-upstream-commands';

const upstreamHyperlink = upstreamInsertCommands.find((command) => command.id === 'insert:hyperlink');
if (!upstreamHyperlink) throw new Error('Missing upstream insert:hyperlink command');
const upstreamField = upstreamInsertCommands.find((command) => command.id === 'insert:field');
if (!upstreamField) throw new Error('Missing upstream insert:field command');

type CaptionPlacement = {
  direction: 'Top' | 'Bottom' | 'Left' | 'Right';
  vertAlign: 'Top' | 'Center' | 'Bottom';
};

const captionPlacements = new Map<string, CaptionPlacement>([
  ['insert:caption-top', { direction: 'Top', vertAlign: 'Top' }],
  ['insert:caption-lt', { direction: 'Left', vertAlign: 'Top' }],
  ['insert:caption-lm', { direction: 'Left', vertAlign: 'Center' }],
  ['insert:caption-lb', { direction: 'Left', vertAlign: 'Bottom' }],
  ['insert:caption-rt', { direction: 'Right', vertAlign: 'Top' }],
  ['insert:caption-rm', { direction: 'Right', vertAlign: 'Center' }],
  ['insert:caption-rb', { direction: 'Right', vertAlign: 'Bottom' }],
  ['insert:caption-bottom', { direction: 'Bottom', vertAlign: 'Top' }],
]);

type PictureSelectionRef = {
  sec: number;
  ppi: number;
  ci: number;
  type: string;
  cellPath?: unknown[];
  headerFooter?: { kind: 'header' | 'footer'; outerParaIdx: number; outerControlIdx: number };
};

type HopObjectContext = EditorContext & {
  selectedPictureType?: string;
  selectedPictureTypes?: string[];
  selectedPictureCount?: number;
};

const captionCapableTypes = new Set(['image', 'shape', 'ole']);
const transformCapableTypes = new Set(['image', 'shape', 'ole']);

function selectedPictureType(ctx: EditorContext): string | undefined {
  return (ctx as HopObjectContext).selectedPictureType;
}

function overrideObjectCanExecute(
  id: string,
  predicate: (ctx: HopObjectContext) => boolean,
): CommandDef {
  const upstream = upstreamInsertCommands.find((command) => command.id === id);
  if (!upstream) throw new Error(`Missing upstream ${id} command`);
  return {
    ...upstream,
    canExecute: (ctx) => ctx.hasDocument
      && ctx.inPictureObjectSelection
      && ctx.isEditable
      && !ctx.isFormMode
      && predicate(ctx as HopObjectContext),
  };
}

/** Wire the existing HOP toolbar glyph before main.ts binds data-cmd buttons. */
export function installHyperlinkToolbarEntry(root: ParentNode = document): void {
  const icon = root.querySelector('.icon-hyperlink');
  const button = icon?.closest<HTMLButtonElement>('.tb-btn');
  if (button && !button.dataset.cmd) button.dataset.cmd = 'insert:hyperlink';
}

const hyperlinkCommand: CommandDef = {
  ...upstreamHyperlink,
  shortcutLabel: 'Ctrl+K,H',
  canExecute: (ctx) => (
    ctx.hasDocument
    && ctx.isEditable
    && !ctx.isFormMode
    && !ctx.inTable
    && !ctx.inField
    && !ctx.hasSelection
  ),
  execute(services) {
    const inputHandler = services.getInputHandler();
    if (!inputHandler) return;
    const pos = inputHandler.getPosition();
    const guardedPos = pos as typeof pos & {
      parentParaIndex?: number;
      cellIndex?: number;
      cellPath?: unknown[];
      isTextBox?: boolean;
    };

    // Initial implementation is body-only. Cell/textbox/nested coordinates have
    // a separate dirty/reflow contract and must not accidentally fall through.
    if (
      guardedPos.parentParaIndex !== undefined
      || guardedPos.cellIndex !== undefined
      || (guardedPos.cellPath?.length ?? 0) > 0
      || guardedPos.isTextBox
    ) return;

    const dialog = new HyperlinkDialog();
    dialog.onApply = ({ url, displayText }) => {
      inputHandler.executeOperation({
        kind: 'snapshot',
        operationType: 'insertHyperlink',
        operation: (wasm) => {
          const result = insertBodyHyperlink(
            wasm,
            pos.sectionIndex,
            pos.paragraphIndex,
            pos.charOffset,
            url,
            displayText,
          );
          if (!result.ok || result.charOffset === undefined) {
            throw new Error('하이퍼링크를 삽입하지 못했습니다.');
          }
          return { ...pos, charOffset: result.charOffset };
        },
      });
      inputHandler.focus();
    };
    dialog.show();
  },
};

const fieldCommand: CommandDef = {
  ...upstreamField,
  label: '누름틀',
  shortcutLabel: 'Ctrl+K,E',
};

function setSelectedObjectCaption(
  services: Parameters<CommandDef['execute']>[0],
  props: Record<string, unknown>,
): void {
  const inputHandler = services.getInputHandler();
  if (!inputHandler) return;
  const ref = inputHandler.getSelectedPictureRef() as PictureSelectionRef | null;
  if (!ref || ['equation', 'group', 'line'].includes(ref.type)) return;

  inputHandler.executeOperation({
    kind: 'snapshot',
    operationType: 'setObjectCaption',
    operation: (wasm) => {
      if (ref.type === 'shape') {
        if (ref.cellPath?.length) {
          wasm.setCellShapePropertiesByPath(ref.sec, ref.ppi, ref.cellPath as never, ref.ci, props);
        } else {
          wasm.setShapeProperties(ref.sec, ref.ppi, ref.ci, props);
        }
      } else if (ref.headerFooter) {
        wasm.setHeaderFooterPictureProperties(
          ref.sec,
          ref.headerFooter.outerParaIdx,
          ref.headerFooter.outerControlIdx,
          ref.ppi,
          ref.ci,
          props,
        );
      } else if (ref.cellPath?.length) {
        wasm.setCellPicturePropertiesByPath(ref.sec, ref.ppi, ref.cellPath as never, ref.ci, props);
      } else {
        wasm.setPictureProperties(ref.sec, ref.ppi, ref.ci, props);
      }
      return inputHandler.getCursorPosition();
    },
  });
}

const captionCommands: CommandDef[] = [
  ...Array.from(captionPlacements, ([id, placement]) => {
    const upstream = upstreamInsertCommands.find((command) => command.id === id);
    if (!upstream) throw new Error(`Missing upstream ${id} command`);
    return {
      ...upstream,
      canExecute: (ctx) => ctx.hasDocument
        && ctx.inPictureObjectSelection
        && ctx.isEditable
        && !ctx.isFormMode
        && captionCapableTypes.has(selectedPictureType(ctx) ?? ''),
      execute(services) {
        setSelectedObjectCaption(services, {
          hasCaption: true,
          captionDirection: placement.direction,
          captionVertAlign: placement.vertAlign,
        });
      },
    } satisfies CommandDef;
  }),
  (() => {
    const upstream = upstreamInsertCommands.find((command) => command.id === 'insert:caption-none');
    if (!upstream) throw new Error('Missing upstream insert:caption-none command');
    return {
      ...upstream,
      canExecute: (ctx) => ctx.hasDocument
        && ctx.inPictureObjectSelection
        && ctx.isEditable
        && !ctx.isFormMode
        && captionCapableTypes.has(selectedPictureType(ctx) ?? ''),
      execute(services) {
        setSelectedObjectCaption(services, { hasCaption: false });
      },
    } satisfies CommandDef;
  })(),
];

const objectSubtypeCommands: CommandDef[] = [
  ...['insert:arrange-front', 'insert:arrange-forward', 'insert:arrange-backward', 'insert:arrange-back']
    .map((id) => overrideObjectCanExecute(id, (ctx) => ctx.selectedPictureType === 'shape')),
  overrideObjectCanExecute('insert:ungroup-shapes', (ctx) => ctx.selectedPictureType === 'group'),
  overrideObjectCanExecute('insert:group-shapes', (ctx) => {
    const types = ctx.selectedPictureTypes ?? [];
    return (ctx.selectedPictureCount ?? types.length) >= 2
      && types.length >= 2
      && types.every((type) => type === 'shape' || type === 'line');
  }),
  ...['insert:rotate-cw', 'insert:rotate-ccw', 'insert:flip-horz', 'insert:flip-vert']
    .map((id) => overrideObjectCanExecute(id, (ctx) => transformCapableTypes.has(ctx.selectedPictureType ?? ''))),
  overrideObjectCanExecute('insert:equation-edit', (ctx) => ctx.selectedPictureType === 'equation'),
  overrideObjectCanExecute('insert:caption-toggle', (ctx) => captionCapableTypes.has(ctx.selectedPictureType ?? '')),
];

export const insertCommands: CommandDef[] = replaceUpstreamCommands(
  upstreamInsertCommands,
  [fieldCommand, hyperlinkCommand, ...captionCommands, ...objectSubtypeCommands],
);
