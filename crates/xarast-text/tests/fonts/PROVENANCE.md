# Test fonts: provenance and licences

These faces are the **pinned test font set** of `xarast-text`. Deterministic
tests load them from here and never touch the system font collection, so a test
result cannot depend on the machine's installed fonts.

| File | Upstream face | Upstream source | Licence | Subset |
|---|---|---|---|---|
| `NotoSans-Regular.subset.ttf` | Noto Sans Regular 2.004 | Debian `fonts-noto-core` 20201225-2 | OFL-1.1 | U+0020-007E, U+00A0-00FF, U+0300-036F, U+2010-2027, U+2030-203A |
| `NotoSans-Bold.subset.ttf` | Noto Sans Bold 2.004 | Debian `fonts-noto-core` 20201225-2 | OFL-1.1 | U+0020-007E |
| `NotoSans-Italic.subset.ttf` | Noto Sans Italic 2.004 | Debian `fonts-noto-core` 20201225-2 | OFL-1.1 | U+0020-007E |
| `NotoSansHebrew-Regular.subset.ttf` | Noto Sans Hebrew Regular 3.000 | Debian `fonts-noto-core` 20201225-2 | OFL-1.1 | U+0020, U+0591-05F4, U+FB1D-FB4F |
| `NotoSansArabic-Regular.subset.ttf` | Noto Sans Arabic Regular 2.005 | Debian `fonts-noto-core` 20201225-2 | OFL-1.1 | U+0020, U+060C, U+061B, U+061F, U+0621-0652, U+0660-0669 |
| `NotoSansCJKjp-Regular.subset.otf` | Noto Sans CJK JP Regular 2.004 (face 0 of `NotoSansCJK-Regular.ttc`, CFF outlines) | Debian `fonts-noto-cjk` 1:20230817+repack1-3 | OFL-1.1 | 28 ideographs/kana used by the tests plus U+3000-3003, U+300C-300D, U+FF01, U+FF08-FF09, U+FF0C |
| `XarastTestVariable.ttf` | none: synthetic, drawn by `make_variable.py` | this repository | MIT OR Apache-2.0 | n/a |

The licence was checked explicitly on 2026-09-23 in the fonts themselves (name
ID 13 of every Noto face reads "This Font Software is licensed under the SIL
Open Font License, Version 1.1"; name ID 14 points at the OFL) and in the
Debian copyright files of both packages. No face declares a **Reserved Font
Name** (name ID 0 carries only the copyright line), so the OFL allows a
modified version such as a subset to keep the original family name. The full
licence text and the copyright lines are in `OFL.txt`, which must travel with
these files (OFL condition 2).

The OFL covers font data, not code: bundling the faces as test fixtures places
no condition on Xarast's source, which stays `MIT OR Apache-2.0`. The fixtures
are never shipped in the AppImage.

## Regenerating

```sh
PYFTSUBSET=/path/to/pyftsubset sh make_subsets.sh \
    /usr/share/fonts/truetype/noto /usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc
python3 make_variable.py XarastTestVariable.ttf
```

Both scripts were run with fontTools 4.65.0. Golden shaping values in the tests
are pinned to these exact bytes; SHA-256 at the time of writing:

```
3bdc74697e49d85bf5deeba48a0f2359ba8744bc2a4bf814d15647fe011c8219  NotoSans-Regular.subset.ttf
a3dd81d89ea912c02233770a8146eacd7f119844bc6e445d9e41d0965c5687d7  NotoSans-Bold.subset.ttf
490ec0b46a5f7fea52879492a8e88e633eae2d9a078187a7f91401fd8211db8b  NotoSans-Italic.subset.ttf
d77049b1cc9ab09e065768bee31b71a832a87dceca5ca48e36c2e9be9d8fec1b  NotoSansHebrew-Regular.subset.ttf
c76dc9670c3c61fe496eee92342155e4c8b7ef54d25a8081833210f79ab62a8f  NotoSansArabic-Regular.subset.ttf
06952e44347601d5bc4d10eabfa37d5d0056b79836438d9627c8026350e02989  NotoSansCJKjp-Regular.subset.otf
d00bcda46bd672ffc1528c0297d7ff10f9ae92422d862b00cecfb8825bc7bed4  XarastTestVariable.ttf
```
