#!/usr/bin/env python3
"""Build Unicode Computer Modern from vendored original BaKoMa outlines.

Requires fonttools; run from any directory. --check verifies outputs.
"""
import argparse
import json
import string
from pathlib import Path

from fontTools import agl
from fontTools.fontBuilder import FontBuilder
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib import TTFont

ROOT = Path(__file__).resolve().parents[1] / 'assets/fonts/computer-modern'
SOURCES = ('cmr10', 'cmmi10', 'cmsy10', 'cmex10', 'cmb10')
STYLES = ('Regular', 'Italic', 'Bold')
COMPOSITES = {
    '≠': [('cmr10', 'equal', 0, 0), ('cmsy10', 'negationslash', 0, 0)],
    '∉': [('cmsy10', 'element', 0, 0), ('cmsy10', 'negationslash', -114, 0)],
    'ℏ': [('cmmi10', 'h', 0, 0), ('cmr10', 'macron', 0, 0)],
    '∬': [('cmsy10', 'integral', 0, 0), ('cmsy10', 'integral', 630, 0)],
    '∭': [('cmsy10', 'integral', 0, 0), ('cmsy10', 'integral', 630, 0),
           ('cmsy10', 'integral', 1260, 0)],
}
COMMON = '≠∉ℏ∬∭' + string.ascii_letters + string.digits + 'αβΓ∂∇∫∑∞≤≥×+=−(){}[]<>|√'


def unicode_mapping(fonts, style):
    # Legacy cmap bytes are NOT Unicode. Start from semantic glyph names only.
    mapping = {}
    for source in SOURCES[:-1]:
        for glyph in fonts[source].getGlyphOrder():
            text = agl.toUnicode(glyph)
            if len(text) == 1:
                mapping.setdefault(ord(text), (source, glyph))
    commands = json.loads((ROOT / 'source/unicode-map.json').read_text())
    for code, (source, legacy_code) in commands.items():
        if source in fonts:
            mapping[int(code)] = (source, fonts[source].getBestCmap()[legacy_code])
    # TeX-sized delimiters are laid out by HOP, not by this font's cmap.
    for char, glyph in {'{': 'braceleft', '}': 'braceright', '|': 'bar',
                        '∫': 'integral', '√': 'radical',
                        '⟨': 'angbracketleft', '⟩': 'angbracketright'}.items():
        mapping[ord(char)] = ('cmsy10', glyph)
    for char, glyph in {'∑': 'summationtext', '∏': 'producttext',
                        '∐': 'coproducttext', '∮': 'contintegraltext'}.items():
        mapping[ord(char)] = ('cmex10', glyph)
    mapping[0x03F5] = ('cmmi10', 'epsilon1')
    mapping[0x03D6] = ('cmmi10', 'pi1')
    for char in string.ascii_letters:
        mapping[ord(char)] = ('cmmi10' if style == 'Italic' else 'cmr10', char)
    if style == 'Bold':
        for code, (source, glyph) in list(mapping.items()):
            if source == 'cmr10' and glyph in fonts['cmb10'].getGlyphOrder():
                mapping[code] = ('cmb10', glyph)
    return mapping


def build(style, fonts):
    mapping = unicode_mapping(fonts, style)
    parts = {(source, name) for pieces in COMPOSITES.values() for source, name, _, _ in pieces}
    pairs = sorted(set(mapping.values()) | parts | {('cmr10', '.notdef')})
    names = {pair: '.notdef' if pair[1] == '.notdef' else '_'.join(pair) for pair in pairs}
    order = ['.notdef'] + sorted(set(names.values()) - {'.notdef'})
    glyphs, metrics = {}, {}
    for source, name in pairs:
        font = fonts[source]
        original = font['glyf'][name]
        pen = TTGlyphPen(None)
        # CMEX glyphs use TeX's top-origin convention. Translate, never rescale,
        # so their centre sits on CM's 250/1000 em mathematical axis in SVG/PDF.
        offset = 0
        if source == 'cmex10' and original.numberOfContours:
            offset = round(512 - (original.yMin + original.yMax) / 2)
        font.getGlyphSet()[name].draw(TransformPen(pen, (1, 0, 0, 1, 0, offset)))
        output_name = names[source, name]
        glyphs[output_name] = pen.glyph()
        metrics[output_name] = font['hmtx'][name]
    for char, pieces in COMPOSITES.items():
        pen = TTGlyphPen(glyphs)
        for source, name, x, y in pieces:
            pen.addComponent(names[source, name], (1, 0, 0, 1, x, y))
        name = 'uni%04X' % ord(char)
        glyphs[name] = pen.glyph()
        base_source, base_name, _, _ = pieces[0]
        advance, lsb = fonts[base_source]['hmtx'][base_name]
        metrics[name] = (advance + (pieces[-1][2] if char in '∬∭' else 0), lsb)
        order.append(name)
    fb = FontBuilder(2048, isTTF=True)
    fb.setupGlyphOrder(order)
    cmap = {code: names[pair] for code, pair in mapping.items()}
    cmap.update({ord(char): 'uni%04X' % ord(char) for char in COMPOSITES})
    fb.setupCharacterMap(cmap)
    fb.setupGlyf(glyphs)
    fb.setupHorizontalMetrics(metrics)
    fb.setupHorizontalHeader(ascent=1638, descent=-410)
    license_text = (ROOT / 'LICENSE.txt').read_text()
    fb.setupNameTable({
        'familyName': 'Computer Modern', 'styleName': style,
        'uniqueFontIdentifier': 'HOP-ComputerModern-' + style + '-1.0',
        'fullName': 'Computer Modern ' + style,
        'psName': 'ComputerModern-' + style, 'version': 'Version 1.000',
        'copyright': 'Copyright (C) 1994, 1995, Basil K. Malyshev. All Rights Reserved.',
        'manufacturer': 'Basil K. Malyshev; Unicode packaging by HOP',
        'description': 'Original BaKoMa Computer Modern outlines, Unicode remapped.',
        'licenseDescription': license_text,
        'licenseInfoURL': 'https://ctan.org/tex-archive/fonts/cm/ps-type1/bakoma',
    })
    fb.setupOS2(sTypoAscender=1638, sTypoDescender=-410, sTypoLineGap=0,
                usWinAscent=max(1638, max(g.yMax for g in glyphs.values() if g.numberOfContours)),
                usWinDescent=max(410, -min(g.yMin for g in glyphs.values() if g.numberOfContours)),
                usWeightClass=700 if style == 'Bold' else 400,
                fsSelection={'Regular': 64, 'Italic': 1, 'Bold': 32}[style], fsType=0)
    fb.setupPost(italicAngle=-14 if style == 'Italic' else 0)
    fb.font['head'].macStyle={'Regular': 0, 'Italic': 2, 'Bold': 1}[style]
    fb.font['head'].created = fb.font['head'].modified = 2082844800
    fb.font.recalcTimestamp = False
    fb.save(ROOT / ('ComputerModern-' + style + '.ttf'))


def check():
    fonts = {name: TTFont(ROOT / 'source' / (name + '.ttf')) for name in SOURCES}
    for style in STYLES:
        font = TTFont(ROOT / ('ComputerModern-' + style + '.ttf'))
        cmap = font.getBestCmap()
        assert all(ord(char) in cmap for char in COMMON), style
        assert font['name'].getDebugName(1) == 'Computer Modern'
        assert font['name'].getDebugName(2) == style
        assert cmap[ord('=')].endswith('_equal')
        assert cmap[ord('<')] == 'cmmi10_less'
        assert cmap[ord('{')] == 'cmsy10_braceleft'
        assert cmap[ord('α')] == 'cmmi10_alpha'
        source = 'cmmi10' if style == 'Italic' else ('cmb10' if style == 'Bold' else 'cmr10')
        assert cmap[ord('x')] == source + '_x'
        assert cmap[ord('0')] == ('cmb10' if style == 'Bold' else 'cmr10') + '_zero'
        for code, (source, name) in unicode_mapping(fonts, style).items():
            original = fonts[source]['glyf'][name]
            derived = font['glyf'][cmap[code]]
            assert font['hmtx'][cmap[code]] == fonts[source]['hmtx'][name]
            old, _, _ = original.getCoordinates(fonts[source]['glyf'])
            new, _, _ = derived.getCoordinates(font['glyf'])
            assert len(old) == len(new)
            if old:
                dy = new[0][1] - old[0][1]
                assert all(x == a and y == b + dy for (x, y), (a, b) in zip(new, old))
                assert dy == 0 or source == 'cmex10'
        for char, pieces in COMPOSITES.items():
            expected = []
            for source, name, dx, dy in pieces:
                points, _, _ = fonts[source]['glyf'][name].getCoordinates(fonts[source]['glyf'])
                expected.extend((x + dx, y + dy) for x, y in points)
            points, _, _ = font['glyf'][cmap[ord(char)]].getCoordinates(font['glyf'])
            assert list(points) == expected, (style, char)
        print(style + ': ' + str(len(cmap)) + ' Unicode characters; original outlines/widths and common math verified')


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    if not args.check:
        fonts = {name: TTFont(ROOT / 'source' / (name + '.ttf')) for name in SOURCES}
        for style in STYLES:
            build(style, fonts)
    check()
