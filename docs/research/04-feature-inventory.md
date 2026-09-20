# 04 — Complete feature inventory of Xara Xtreme (Xara LX)

> **Clean-room notice.** This document describes the *behaviour* and the *data
> formats* of Xara Xtreme (GPL-2.0-only) for interoperability purposes: it is an
> inventory of observable functionality and of its reimplementation cost. It
> does not reproduce source code from the original; the `file:line` references
> point at the reference tree in `xara-xtreme/` and serve only to locate the
> logic described. Xarast is implemented from this specification, not by
> translating the original.

> **Purpose**: functional-parity backlog for the Rust reimplementation (the *Xarast* project).
> **Source**: original code tree `/home/user/xara-xtreme` (Xara LX / Xara Xtreme for Linux, GPLv2, © 1993–2006 Xara Group Ltd).
> **Size of the original**: ~825 `.cpp` + ~915 `.h`, spread across `Kernel/` (document model, operations, filters, galleries), `tools/` (interactive tools), `wxOil/` (the OIL layer = *OS Interface Layer*, wxWidgets), `GDraw/` (the proprietary CDraw rasterisation engine), `filters/SVGFilter/` (external SVG filter).
> **Date of analysis**: 2026-09-19.

## How to read this document

- **Reimplementation complexity**: `Low` (< 1 person-week), `Medium` (1–4 weeks), `High` (1–3 months), `Very high` (> 3 months, or requires algorithmic research).
- **Priority**: `P0` = MVP of the first Linux release in Rust; `P1` = second wave (usable day to day); `P2` = advanced parity; `P3` = legacy / niche / probably droppable.
- The C++ files cited are paths **relative to the root of the original tree**.

---

## 1. Master feature table

### 1.1 Document, pages, spreads and layers

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Document | Document model as a node tree | The whole document is a `Node` tree (typed, with attributes as children). The basis of rendering, undo, hit-testing and serialisation. | `Kernel/node.cpp`, `Kernel/nodedoc.cpp`, `Kernel/document.cpp`, `Kernel/basedoc.cpp` | Very high | P0 |
| Document | Document components (`DocComponent`) | Pluggable document sections: colours, units, info, views, printing, fonts… each serialised separately. | `Kernel/doccomp.cpp`, `Kernel/colcomp.cpp`, `Kernel/unitcomp.cpp`, `Kernel/infocomp.cpp`, `Kernel/viewcomp.cpp`, `Kernel/princomp.cpp`, `Kernel/fontcomp.cpp` | Medium | P0 |
| Document | New document (drawing) | Creates an empty document from the default template. | `Kernel/menuops.cpp` (`OPTOKEN_FILENEW_DRAWING`), `Kernel/tmpltmgr`/`wxOil/tmplmngr.cpp` | Low | P0 |
| Document | New animation document | Document in animation mode (layers = frames). | `Kernel/frameops.cpp`, `Kernel/menuops.cpp` (`OPTOKEN_FILENEW_ANIMATION`) | Medium | P2 |
| Document | Templates (`FileNewTemplate` 1..10, "Save as default") | Template menu; save the current document as a template / as the default. | `Kernel/tmpltdlg.cpp`, `Kernel/tmpltarg.cpp`, `Kernel/tmpltatr.cpp`, `wxOil/stemplate.cpp`, `wxOil/tmplmngr.cpp`, `Templates/` | Low | P2 |
| Document | Open / Save / Save as / Save all | File life cycle, 9-entry MRU. | `Kernel/menuops.cpp`, `Kernel/filelist.cpp`, `wxOil/camdoc.cpp`, `wxOil/filedlgs.cpp` | Low | P0 |
| Document | Recent file list (MRU) | 9 entries persisted in preferences. | `Kernel/filelist.cpp`, `wxOil/camprofile.cpp` | Low | P1 |
| Document | Document information (`FileInfo`) | Dialog of document metadata/statistics. | `Kernel/finfodlg.cpp`, `Kernel/infocomp.cpp` | Low | P2 |
| Page | Page size and setup (`PageSetupDlg`) | Predefined sizes (`DEFAULT_PAGESIZES.res`), custom size, orientation, margins. | `Kernel/pagesize.cpp`, `Kernel/page.cpp`, `Kernel/npaper.cpp`, `Kernel/optspage.cpp`, `wxOil/xrc/DEFAULT_PAGESIZES.res` | Medium | P0 |
| Page | Multi-page / spreads | A document contains *spreads*; each spread contains pages; the spread origin is adjustable. | `Kernel/spread.cpp`, `Kernel/page.cpp`, `Kernel/chapter.cpp` (`OPTOKEN_SPREADORIGIN`, `OPTOKEN_RESETSPREADORIGIN`) | High | P1 |
| Page | Page background / paper colour | Renderable paper object, background colour, background deletion. | `Kernel/paper.cpp`, `Kernel/backgrnd.cpp` (`OPTOKEN_BACKGROUND`, `OPTOKEN_DELETEPAGEBACKGROUND`) | Low | P1 |
| Page | Visible print borders | Shows the printable/bleed area. | `Kernel/viewmenu.cpp` (`OPTOKEN_SHOWPRINTBORDERS`) | Low | P2 |
| Layers | Layer model | A layer is a container node with a name, visibility, lock, printable flag, snapping and highlight colour. | `Kernel/layer.cpp`, `Kernel/layermgr.cpp` | Medium | P0 |
| Layers | Layer gallery/panel | Create, delete, rename, reorder, show/hide, lock, multi-layer. | `Kernel/layergal.cpp`, `Kernel/sglayer.cpp`, `Kernel/layerdlg.cpp`, `Kernel/layerprp.cpp`, `Kernel/prpslyrs.cpp` | Medium | P0 |
| Layers | Layer properties (tabbed) | Tabbed layer-properties dialog (name, state, web). | `Kernel/layerprp.cpp` (`OPTOKEN_LAYERPROPERTYTABS`), `Kernel/layerdlg.cpp` | Low | P1 |
| Layers | Move selection to the active layer | Operation that moves objects between layers. | `Kernel/lattrops.cpp` (`OPTOKEN_MOVE_SEL_TO_ACTIVE_LAYER`) | Low | P1 |
| Layers | Move object one layer up/down | `Ctrl+Shift+U` / `Ctrl+Shift+D`. | `Kernel/zordops.cpp` (`OPTOKEN_MOVELAYERINFRONT`, `OPTOKEN_MOVELAYERBEHIND`) | Low | P1 |
| Layers | Combine layers into a frame layer | Conversion from layers to an animation frame. | `Kernel/frameops.cpp` (`OPTOKEN_COMBINELAYERSTOFRAMELAYER`) | Medium | P3 |
| Document | Guides as a special layer | Guides live on a dedicated non-printable layer. | `Kernel/guides.cpp`, `Kernel/prpsgds.cpp` | Low | P1 |

### 1.2 Shape drawing

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Shapes | Rectangle tool (`TOOL5`) | Parametrically editable rectangles and rounded rectangles (they are not converted to paths). | `tools/rectangl.cpp`, `Kernel/noderect.cpp`, `Kernel/rechrect.cpp` | Medium | P0 |
| Shapes | Interactive rounded corners | Dragging a radius handle on the canvas. | `tools/rectangl.cpp`, `Kernel/noderect.cpp` | Medium | P0 |
| Shapes | Ellipse tool (`TOOL12`) | Parametric ellipses/circles with handles. | `tools/eliptool.cpp`, `Kernel/nodeelip.cpp`, `Kernel/rechellp.cpp` | Medium | P0 |
| Shapes | QuickShape tool (`TOOL18`) | Regular polygons and stars: number of sides (3–10+), stellation, curvature, radius/diameter, ellipse/polygon mode. | `tools/regshape.cpp`, `tools/oprshape.cpp`, `Kernel/nodershp.cpp`, `Kernel/rechrshp.cpp`, `Kernel/opsmpshp.cpp` | High | P1 |
| Shapes | QuickShape: stellation and curvature | Live parameters `OPTOKEN_TOGGLESTELLATION`, `OPTOKEN_TOGGLECURVATURE`, `OPTOKEN_TOGGLEELIPPOLY`. | `tools/regshape.cpp`, `Kernel/nodershp.cpp` | Medium | P1 |
| Shapes | QuickShape: direct number of sides | Shortcuts `OPTOKEN_QUICKSHAPE_NUMBERSIDES3..10`. | `tools/regshape.cpp` | Low | P2 |
| Shapes | Reshaping a QuickShape edge | Dragging one side deforms the whole regular figure. | `tools/oprshape.cpp` (`OPTOKEN_RESHAPESHAPEEDGE`) | High | P2 |
| Shapes | Aspect constraint (Ctrl) while creating | Perfect square/circle, constrained angles. | `tools/rectangl.cpp`, `tools/eliptool.cpp`, `Kernel/input.cpp` | Low | P0 |
| Shapes | Create from the centre | Drag modifier. | `tools/rectangl.cpp`, `tools/eliptool.cpp` | Low | P1 |
| Shapes | Convert to shapes (`Ctrl+Shift+S`) | Converts a rectangle/ellipse/QuickShape/text into editable paths. | `Kernel/shapeops.cpp` (`OPTOKEN_ARRANGEMAKESHAPES`, `OPTOKEN_MAKE_SHAPES`) | Medium | P0 |
| Shapes | Convert path to shapes | Outline (line width) → fill. | `tools/opcntr.cpp` (`OPTOKEN_CONVERTPATHTOSHAPES`) | High | P1 |
| Shapes | Convert to bitmap (`Ctrl+Shift+C`) | Rasterises the selection into an embedded bitmap at a chosen DPI/depth. | `Kernel/makebmp.cpp`, `Kernel/bmpsdlg.cpp` | Medium | P1 |
| Shapes | Blank tool (skeleton) | Reference template tool for developers. | `tools/blnktool.cpp` | Low | P3 |

