# Computer Modern for HOP equations

These fonts contain the **original Computer Modern** outlines converted by
Basil K. Malyshev for BaKoMa. They are not Latin Modern or CMU substitutes.
Family name: `Computer Modern`. Files and subfamilies:

- `ComputerModern-Regular.ttf` — Regular
- `ComputerModern-Italic.ttf` — Italic
- `ComputerModern-Bold.ttf` — Bold

## Sources and modifications

`source/cm{r,mi,sy,ex,b}10.ttf` are byte-for-byte copies distributed with
Matplotlib 3.10.9. Original collection: BaKoMa-CM 1.1 (12 November 1994),
[CTAN BaKoMa](https://ctan.org/tex-archive/fonts/cm/ps-type1/bakoma).
`LICENSE.txt` contains the BaKoMa permission, author, copyright and source notice;
it is also embedded in each generated font's name table.

`source/unicode-map.json` extracts the command-to-font/code mapping from
Matplotlib 3.10.9 `_mathtext_data.py` (`latex_to_bakoma`, `tex2uni`).
Its applicable notices are in `source/LICENSE-matplotlib.txt`.

The generator replaces legacy TeX cmap encodings with semantic Unicode mappings,
combines glyphs, and assigns family/style metadata. Original contours and advance
widths are preserved. CMEX glyphs alone are translated vertically so their centres
lie on the original Computer Modern mathematical axis (250/1000 em); TeX's
original top-origin placement is unsuitable for a normal SVG/PDF text baseline.
TrueType hint programs and kerning tables are omitted; equation layout remains
HOP's renderer's responsibility. These are 10-point-design outlines, without an
OpenType MATH table or automatic optical-size substitution.

Italic uses original `cmmi10` Latin math italics with upright `cmr10` digits and
operators. Bold uses original `cmb10` Roman letters, digits and upright Greek;
symbols and lower-case Greek retain the regular original math outlines because
the source pack has no matching bold math faces. No synthetic emboldening is baked
into these files, and no Latin Modern fallback is embedded.

The added Unicode composites retain their original component outlines unchanged:
`≠` overlays CM's negation slash on `=`, `∉` overlays it on the membership sign,
`ℏ` overlays CM Roman's macron on the original math-italic `h`, and `∬`/`∭`
repeat the original integral with a 630/2048 em horizontal step. These additions
use translated components, with no outline scaling or redrawing. Their expanded
contour coordinates are also checked against the original components.

## Build and verify

With Python 3 and `fonttools` installed:

```sh
python3 scripts/build-computer-modern-fonts.py
python3 scripts/build-computer-modern-fonts.py --check
```

All source files are vendored, so generation needs no network or Matplotlib
installation. Output timestamps are fixed for reproducible builds. The check
verifies every mapped glyph's original contour coordinates and advance width,
allowed CMEX baseline translations, family/style names, representative Greek and
math symbols, and legacy ASCII collisions (`=`, `<`, `{`, digits, italic `x`).

Each face covers 341 Unicode characters, including
`α β Γ ∂ ∇ ∫ ∑ ∞ ≤ ≥ × + = − ( ) { } [ ] < > | √ ≠ ∉ ℏ ∬ ∭`.
This original font collection is not a complete Unicode math font. Examples
outside its current mapping include `ℝ ℂ ℤ ℕ ℚ ∴ ∵` and ASCII
underscore/double-quote/tilde. The host renderer must compose or otherwise handle
unsupported symbols; this asset does not claim those glyphs are Computer Modern.

## Source SHA-256

- `cmb10.ttf`: `074497b50c43ea57597181591f9893d38fc12a87e95104f5657fc2481f61a23a`
- `cmex10.ttf`: `af28f0c170723ac776a71bfa595aca09e98d15ae5c3d54ce19afd5619c96a905`
- `cmmi10.ttf`: `3092965b8811fd6a6765799664845181fc1dd1330b937f915808c485d316234d`
- `cmr10.ttf`: `4dd9761b058c009db9b2145f55ee6617d093c273dd3d6c5b3e43ff6110fa9bf6`
- `cmsy10.ttf`: `bb226ed932f3f100cd0e52f5e4912ee553b41b69e7bfdb8d3854db0eb6605232`
