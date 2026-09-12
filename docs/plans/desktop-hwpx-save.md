# Desktop HWPX save parity

## Problem

HOP can open and edit HWPX documents, and the pinned `rhwp` engine already
exposes `exportHwpx()`. The desktop integration nevertheless blocks direct HWPX
saves and removes the upstream `file:save-as-hwpx` command. This forces an HWPX
document through HWP when saving, which is both surprising and a compatibility
regression compared with Hancom Office.

## Goal

Make desktop save preserve the source format, add explicit native “save as HWP”
and “save as HWPX” commands, and keep the existing staged/atomic write,
revision, external-modification, recent-document and cleanup guarantees.

## Non-goals

- No changes to `third_party/rhwp`.
- No DRM/licensing/activation behavior.
- No copying of Hancom proprietary assets or UI resources.
- No weakening of the existing HWP save path or PDF staging path.

## Acceptance gates

1. HWP and HWPX sources both save back to their original format.
2. Save As preserves the current source format by default.
3. Explicit HWP/HWPX Save As commands use the requested extension and serializer.
4. Native commit validates the staged document and records the resulting format.
5. Revision/external-overwrite guards and staging cleanup remain intact.
6. Focused Studio and Rust tests cover HWP and HWPX paths.
7. `test:studio`, `test:desktop`, `test:upstream` remain green before release build.
