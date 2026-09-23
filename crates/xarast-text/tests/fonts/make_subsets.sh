#!/bin/sh
# Regenerates the subset test fixtures in this directory from the upstream
# Noto faces (SIL Open Font License 1.1, see OFL.txt and PROVENANCE.md).
#
# Usage: make_subsets.sh <noto-truetype-dir> <noto-cjk-ttc>
# Needs `pyftsubset` from fontTools (tested with fontTools 4.65.0).
# Hinting is dropped: Xarast draws unhinted outlines, and it halves the size.
set -eu
NOTO=${1:-/usr/share/fonts/truetype/noto}
CJK=${2:-/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc}
OUT=$(dirname "$0")
SUBSET="${PYFTSUBSET:-pyftsubset} --layout-features=* --no-hinting --name-IDs=* --name-languages=*"

$SUBSET "$NOTO/NotoSans-Regular.ttf" \
    --unicodes="U+0020-007E,U+00A0-00FF,U+0300-036F,U+2010-2027,U+2030-203A" \
    --output-file="$OUT/NotoSans-Regular.subset.ttf"
$SUBSET "$NOTO/NotoSans-Bold.ttf" --unicodes="U+0020-007E" \
    --output-file="$OUT/NotoSans-Bold.subset.ttf"
$SUBSET "$NOTO/NotoSans-Italic.ttf" --unicodes="U+0020-007E" \
    --output-file="$OUT/NotoSans-Italic.subset.ttf"
$SUBSET "$NOTO/NotoSansHebrew-Regular.ttf" \
    --unicodes="U+0020,U+0591-05F4,U+FB1D-FB4F" \
    --output-file="$OUT/NotoSansHebrew-Regular.subset.ttf"
$SUBSET "$NOTO/NotoSansArabic-Regular.ttf" \
    --unicodes="U+0020,U+060C,U+061B,U+061F,U+0621-064A,U+064B-0652,U+0660-0669" \
    --output-file="$OUT/NotoSansArabic-Regular.subset.ttf"
$SUBSET "$CJK" --font-number=0 \
    --text="日本語の文字列を改行するテストです漢字中文字体排版测试" \
    --unicodes="U+0020,U+3000-3003,U+300C-300D,U+FF01,U+FF08-FF09,U+FF0C" \
    --output-file="$OUT/NotoSansCJKjp-Regular.subset.otf"
