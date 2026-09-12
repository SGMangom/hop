# Searchable PDF text fidelity

## Problem

The renderer emits one SVG `<text>` node per grapheme so screen placement can
follow HWP/HWPX glyph advances precisely. When the same SVG is converted to PDF,
PDF text extractors interpret many of those independent glyph positions as word
gaps. A real Hancom Office 2024 HWPX sample consequently copied/extracted Korean
as text such as `한 /글 시 작` instead of `한/글 시작`.

## Goal

Keep screen SVG output unchanged, but give PDF export a searchable text surface
whose ordinary runs remain logical strings. Preserve the measured run width with
SVG `textLength`, split around tabs and special vector glyphs, and retain the
existing per-glyph path for complex runs where grouping could change semantics.

## Safety / non-goals

- No OCR and no text reconstructed from rendered pixels.
- No change to normal editor SVG output.
- Old-Hangul, ratio-scaled, shadowed, leader and special middle-dot runs stay on
  the conservative existing rendering path.
- This does not claim typographic identity for fonts unavailable on the host.

## Acceptance gates

1. Ordinary PDF-only text runs are emitted as logical strings.
2. Tabs split grouping instead of disabling the rest of the run.
3. Special vector middle-dot rendering remains unchanged.
4. HOP desktop PDF export uses the searchable SVG surface.
5. On the real Hancom 2024 sample, `pdftotext` recovers representative Korean
   sentences without artificial inter-glyph spaces.
6. Screen SVG remains byte/behavior compatible because grouping is opt-in and
   disabled by default.
