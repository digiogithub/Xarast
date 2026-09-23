"""Builds `XarastTestVariable.ttf`, a tiny synthetic variable font.

Written from scratch for Xarast's tests: it contains no outline or table data
from any other font, so it is covered by the repository's own licence
(`MIT OR Apache-2.0`). Needs fontTools (tested with 4.65.0).

Glyphs (units per em 1000, one `wght` axis 100..400..900):

* `.notdef` - empty box.
* `space`   - advance 250, no outline.
* `I`       - a rectangle from (100, 0) to (200, 700) at the default weight
              (400). At wght=900 the right edge moves to x=300; at wght=100 it
              moves to x=150. The advance is 300 at every weight.
* `o`       - a closed TrueType contour with quadratic off-curve points, to
              exercise the pen's `quad_to` path. Not varied.

Usage: python3 make_variable.py <output.ttf>
"""

import sys

from fontTools.fontBuilder import FontBuilder
from fontTools.pens.ttGlyphPen import TTGlyphPen
from fontTools.ttLib.tables.TupleVariation import TupleVariation


def rect(x0, y0, x1, y1):
    pen = TTGlyphPen(None)
    pen.moveTo((x0, y0))
    pen.lineTo((x0, y1))
    pen.lineTo((x1, y1))
    pen.lineTo((x1, y0))
    pen.closePath()
    return pen.glyph()


def ring():
    pen = TTGlyphPen(None)
    pen.moveTo((250, 0))
    pen.qCurveTo((450, 0), (450, 250))
    pen.qCurveTo((450, 500), (250, 500))
    pen.qCurveTo((50, 500), (50, 250))
    pen.qCurveTo((50, 0), (250, 0))
    pen.closePath()
    return pen.glyph()


def main(out):
    fb = FontBuilder(1000, isTTF=True)
    order = [".notdef", "space", "I", "o"]
    fb.setupGlyphOrder(order)
    fb.setupCharacterMap({0x20: "space", 0x49: "I", 0x6F: "o"})
    empty = TTGlyphPen(None).glyph()
    fb.setupGlyf(
        {
            ".notdef": rect(50, 0, 450, 700),
            "space": empty,
            "I": rect(100, 0, 200, 700),
            "o": ring(),
        }
    )
    fb.setupHorizontalMetrics(
        {".notdef": (500, 50), "space": (250, 0), "I": (300, 100), "o": (500, 50)}
    )
    fb.setupHorizontalHeader(ascent=800, descent=-200)
    fb.setupNameTable(
        {
            "familyName": "Xarast Test Variable",
            "styleName": "Regular",
            "copyright": "Synthetic test font made for Xarast; MIT OR Apache-2.0",
            "licenseDescription": "MIT OR Apache-2.0",
        }
    )
    fb.setupOS2(sTypoAscender=800, sTypoDescender=-200, usWinAscent=800, usWinDescent=200)
    fb.setupPost()
    fb.setupFvar(axes=[("wght", 100, 400, 900, "Weight")], instances=[])
    # Point order of `I`: (100,0) (100,700) (200,700) (200,0), then four
    # phantom points. Only the right edge (points 2 and 3) moves.
    heavy = [(0, 0), (0, 0), (100, 0), (100, 0), (0, 0), (0, 0), (0, 0), (0, 0)]
    light = [(0, 0), (0, 0), (-50, 0), (-50, 0), (0, 0), (0, 0), (0, 0), (0, 0)]
    fb.setupGvar(
        {
            "I": [
                TupleVariation({"wght": (0.0, 1.0, 1.0)}, heavy),
                TupleVariation({"wght": (-1.0, -1.0, 0.0)}, light),
            ]
        }
    )
    fb.save(out)


if __name__ == "__main__":
    main(sys.argv[1])
