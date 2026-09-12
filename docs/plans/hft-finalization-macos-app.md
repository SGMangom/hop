# HFT finalization and macOS app packaging

## Background

HOP already contains clean-room HFT discovery/decoding work, a derived SFNT cache,
desktop/local-font bridges, CanvasKit loading, PDF font paths, and header/footer editing
overrides. The worktree is intentionally dirty and must be preserved while the pieces are
integrated and verified.

## Problem

The implementation has not yet been proven end-to-end against all 387 locally owned HFT
files, nor packaged and smoke-tested as a macOS application.

## Goal

- Decode every HFT in the local `ENGGTI` directory without vendoring source font bytes.
- Materialize valid standard-font cache faces and expose them consistently to desktop,
  CanvasKit/webview, native rendering, and PDF export.
- Preserve the current HOP editor behavior while making header/footer editing and font
  controls work through the intended local override path.
- Produce and verify a runnable macOS `HOP.app`.

## Non-goals

- Do not modify `third_party/rhwp` directly.
- Do not redistribute proprietary HFT source files or derived cache files inside the app.
- Do not redesign unrelated editor UI or change document semantics outside the existing
  additive patches.

## Constraints

- Preserve all unrelated dirty worktree changes; no reset/clean/stash/history rewrite.
- Keep HFT conversion local at runtime and cache outputs under the app-private cache.
- Retain cross-platform code paths unless behavior is inherently macOS-specific.

## Implementation outline

1. Complete the HFT outline decoder and SFNT builder, including composite/special ranges.
2. Verify startup cache reuse and font catalog alias/normalization behavior.
3. Verify CanvasKit/local-font refresh plus toolbar and header/footer editing integration.
4. Run focused and repository-level tests, then build the studio, Quick Look extensions,
   and Tauri app bundle.
5. Verify bundle structure, code signature, launch, editor smoke behavior, and HFT runtime
   cache creation from the packaged app.

## Verification plan

- 387/387 local HFT full-sweep conversion test.
- External SFNT/parser validation where available and Rust fontdb acceptance.
- Studio Vitest suite and production Vite build.
- Desktop Rust tests/clippy and upstream boundary tests.
- Tauri `.app` build, `codesign --verify --deep --strict`, launch smoke test, and local font
  catalog/runtime-cache inspection.

## Rollback / recovery

No source HFT is modified. Derived files are cache-only and can be removed to force a clean
rebuild. All upstream changes remain represented by additive patches, so the vendored
submodule can stay read-only.
