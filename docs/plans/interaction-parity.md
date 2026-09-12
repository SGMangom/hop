# Desktop interaction parity

## Problem

Several HOP commands were functionally implemented but the desktop chrome did
not always tell the truth about them. Toolbar buttons stayed visually enabled
when `canExecute` was false, some Hancom-style Ctrl+K chords were displayed but
not handled, `Ctrl+Shift+S` collided with the table block-sum shortcut, and the
status bar showed physical page position even when HWP section numbering had a
different logical page number.

## Goal

Align visible desktop affordances, shortcuts, and page status with the command
engine without changing the document model or copying proprietary UI assets.

## Acceptance gates

1. Icon/header-footer toolbar buttons mirror `CommandDispatcher.isEnabled` and
   expose the same state through the native `disabled`/ARIA attributes.
2. Ctrl/Cmd+K chords preserve the existing bookmark/numbering mappings and add
   the displayed H/E hyperlink/field mappings without leaving a stale upstream
   chord state.
3. Ctrl/Cmd+Shift+S invokes table block sum only when that command is executable;
   otherwise it reaches File > Save As.
4. Logical HWP page numbering is surfaced in the status bar while physical
   position remains visible when the two differ.
5. Focused tests cover state refresh, chord routing/collision and page labels.
