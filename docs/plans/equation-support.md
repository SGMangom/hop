# Equation support

- Background: HOP 0.4.4 bundles rhwp 0.8.4 equation commands but does not expose equation insertion.
- Problem: Original font substitution alone leaves wrong glyph widths, clipped descenders and overlapping superscripts.
- Goal: Expose equation insertion/editing with original Computer Modern outlines and matching layout measurements.
- Non-goals: Unrelated UI changes, upstream update, or publishing a release.
- Constraints: Keep third_party/rhwp read-only; preserve editable HWP equation records and undo/save behavior.
- Implementation: Restore menu/toolbar/styles, bundle Unicode-encoded original BaKoMa CM fonts, and apply patches/rhwp-computer-modern.patch to a generated engine copy. Native and WASM builds share this source. The patch reads embedded TTF advances and ink bounds, aligns script baselines, and uses the original integral glyph in SVG, Canvas and Skia.
- Build: Install Rust with wasm32-unknown-unknown and wasm-pack. `pnpm run engine:wasm` creates target-local/rhwp-engine and target-local/rhwp-wasm. `pnpm run build:studio` runs this automatically. Native-only builds first run `pnpm run engine:prepare`. Source/font/patch and WASM artifact SHA-256 hashes are recorded beside generated outputs.
- Verification: Equation geometry tests, original-font outline checks, studio tests/typecheck/build, real HWP save/reopen and searchable PDF embedding, visible insertion/editing and preview inspection.
- Font limits: Original 10pt design, without optical sizing or a MATH table. Unsupported Unicode uses the existing fallback; see assets/fonts/computer-modern/README.md for source/style coverage.
- Recovery: Build separately and back up the installed app before replacement. Preserve user documents.
