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

### 1.3 Path editing (paths / Bézier)

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Paths | Path representation | Polylines with line/cubic-Bézier segments, verbs `MoveTo/LineTo/CurveTo/CloseFigure`, multiple subpaths. | `Kernel/paths.cpp`, `Kernel/pathpcs.cpp`, `Kernel/nodepath.cpp` | High | P0 |
| Paths | Bézier / Shape tool (`TOOL11`) | Node and handle editing: select, move, add, delete, smooth/cusp, line/curve. | `tools/beztool.cpp`, `Kernel/pathedit.cpp` | High | P0 |
| Paths | Pen tool (`TOOL14`) | Point-by-point drawing of lines and curves with a preview. | `tools/pentool.cpp`, `Kernel/penedit.cpp` | Medium | P0 |
| Paths | Freehand tool (`TOOL6`) | Free stroke with curve fitting and configurable smoothing. | `tools/freehand.cpp`, `tools/freeinfo.cpp`, `Kernel/fitcurve.cpp`, `Kernel/rsmooth.cpp` | High | P0 |
| Paths | Freehand rub-out | Erase part of the stroke just drawn by backtracking with the mouse. | `tools/freehand.cpp` (cursors `IDC_FREEHANDRUBOUTCUR`) | Medium | P1 |
| Paths | Retrofit / re-smoothing of an existing stroke | Redraw over an existing path to modify it. | `tools/opretro.cpp` (`OPTOKEN_RETROFIT`, `OPTOKEN_RETROSMOOTH`) | High | P2 |
| Paths | Smooth selection | Reduces nodes while preserving the shape (`OPTOKEN_SMOOTHSELECTION`). | `Kernel/opsmooth.cpp`, `Kernel/ndoptmz.cpp` | Medium | P1 |
| Paths | Add / delete points | `OPTOKEN_ADDENDPOINT`, `OPTOKEN_DELETEPOINTSOP`. | `tools/opbezier.cpp`, `Kernel/pathedit.cpp` | Medium | P0 |
| Paths | Make line / make curve | Changes the type of the selected segment. | `tools/opbezier.cpp` (`OPTOKEN_MAKELINESOP`, `OPTOKEN_MAKECURVESOP`) | Low | P0 |
| Paths | Smooth point / cusp point | Synchronises or breaks the handles. | `tools/opbezier.cpp`, `Kernel/pathedit.cpp` | Low | P0 |
| Paths | Close path / auto-close | `OPTOKEN_CLOSEPATHWITHPATH`, `OPTOKEN_AUTOCLOSEPATHS`. | `tools/opbezier.cpp`, `tools/beztool.cpp` | Low | P0 |
| Paths | Reverse path | `OPTOKEN_REVERSEPATH` (affects arrowheads and text on a path). | `tools/opbezier.cpp`, `Kernel/pathops.cpp` | Low | P1 |
| Paths | Break at points | `Ctrl`+button: split the path at the selected nodes. | `Kernel/opbreak.cpp`, `Kernel/nodepath.cpp` | Medium | P1 |
| Paths | Join shapes (`JoinShapes`) | Joins the endpoints of open paths into a single path. | `Kernel/pathops.cpp` (`OPTOKEN_ARRANGEJOINSHAPES`, `OPTOKEN_JOINSHAPEOP`) | Medium | P1 |
| Paths | Break shapes (`BreakShapes`) | Splits the subpaths of a compound path into separate objects. | `Kernel/pathops.cpp` (`OPTOKEN_ARRANGEBREAKSHAPES`, `OPTOKEN_BREAKSHAPEOP`) | Medium | P1 |
| Paths | Combine shapes: Add / Subtract / Intersect / Slice | Path boolean operations (`Ctrl+1..4`), with a winding rule. | `Kernel/combshps.cpp`, `Kernel/pathops.cpp`, `Kernel/gwinding.cpp`, `Kernel/clamp.cpp` | Very high | P1 |
| Paths | Fill rule (non-zero / even-odd) | The `AttrWindingRule` attribute. | `Kernel/lineattr.cpp/.h`, `Kernel/gwinding.cpp` | Low | P0 |
| Paths | Inset path (inner/outer path) | Parallel offsetting of a path; the basis of contours and bevels. | `Kernel/pathstrk.cpp`, `Kernel/pathtrap.cpp`, `tools/opcntr.cpp` (`OPTOKEN_TOGGLEINSETPATH`) | Very high | P2 |
| Paths | Nudging path points | Move nodes with the cursor keys at 6 granularities (1/5/10/one-fifth/pixel 1/pixel 10). | `Kernel/pathndge.cpp`, `Kernel/opnudge.cpp` | Low | P1 |
| Paths | Select / deselect all points | `OPTOKEN_SELECTALLPATHPOINTS`, `OPTOKEN_DESELECTALLPATHPOINTS`. | `Kernel/pathedit.cpp`, `tools/beztool.cpp` | Low | P0 |
| Paths | Path geometry utilities | Length, point at parameter, tangent, exact bounding box, flattening. | `Kernel/pathutil.cpp`, `Kernel/pathproc.cpp`, `Kernel/paths.cpp` | High | P0 |
| Paths | Double path (`NodeDoublePath`) | A path with two tracks, for variable-width effects. | `Kernel/ndbldpth.cpp` | Medium | P3 |

### 1.4 Selection and transformation

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Selection | Unified Selector tool (`TOOL7`) | Select, move, scale, rotate, shear and edit fills — all in a single mode. This is Xara's flagship tool. | `tools/selector.cpp`, `tools/selinfo.cpp`, `tools/rendsel.cpp` | Very high | P0 |
| Selection | Dual scale/rotation state | A second click on the selection toggles the handles between scale ⇄ rotate/shear (with a draggable centre of rotation). | `tools/selector.cpp`, `tools/rotate.h`, `tools/oprotate.cpp` | High | P0 |
| Selection | Marquee selection (drag box) | Selection rectangle, with "touch" vs "enclose" modes. | `tools/selector.cpp`, `Kernel/opdragbx.cpp` (`OPTOKEN_SELECTOR_DRAGBOX`) | Low | P0 |
| Selection | Additive / toggled selection | `Shift`+click adds/removes; a plain click on a selected object toggles its state. | `tools/selector.cpp`, `Kernel/selop.cpp` | Low | P0 |
| Selection | "Inside group" selection (leaf/under) | `Ctrl`+click selects the inner object without ungrouping; `Alt`+click selects the one beneath. Dedicated cursors. | `tools/selector.cpp`, `Kernel/hittest.cpp`, `wxOil/xrc/IDCSR_SEL_LEAF.cur`, `IDCSR_SEL_UNDER.cur` | High | P0 |
| Selection | Select all / none | `Ctrl+A` / `Esc`. | `Kernel/selall.cpp`, `Kernel/selop.cpp` | Low | P0 |
| Selection | Selection range and state | `SelRange`, cache of the selection's state and common attributes. | `Kernel/range.cpp`, `Kernel/selstate.cpp`, `Kernel/editsel.cpp` | High | P0 |
| Selection | Configurable blobs (handles) | Show object, outline, fill and bounding-box blobs independently. | `tools/rendsel.cpp`, `Kernel/blobs.cpp`, `wxOil/handles.cpp` | Medium | P0 |
| Transformation | Translation | Drag / nudge; affine matrix. | `tools/tranlate.cpp`, `Kernel/matrix.cpp`, `Kernel/trans2d.cpp` | Low | P0 |
| Transformation | Scale | With or without aspect ratio, with or without scaling line widths. | `tools/opscale.cpp`, `tools/opscale2.cpp`, `Kernel/tranform.cpp` | Medium | P0 |
| Transformation | Rotation | About an arbitrary centre, with a numeric angle in the infobar. | `tools/oprotate.cpp`, `Kernel/tranform.cpp` | Medium | P0 |
| Transformation | Shear | Side handles and a numeric field. | `tools/opshear.cpp` | Medium | P0 |
| Transformation | Squash (free non-uniform scaling) | Deformation using the 4 corner handles independently. | `tools/opsquash.cpp` | Medium | P1 |
| Transformation | Flip horizontal / vertical | Selector infobar buttons. | `tools/opflip.cpp` | Low | P0 |
| Transformation | Copy and transform (`Copy+drag`) | Dragging with `+`/the secondary button leaves a copy behind. | `Kernel/transop.cpp` (`OPTOKEN_COPYANDTRANSFORM`), `tools/selector.cpp` | Low | P0 |
| Transformation | Numeric X/Y/W/H/angle/shear infobar | Editable fields with bump buttons and 9 anchor points (NW..SE grid). | `tools/selinfo.cpp`, resources `IDC_SEL_GRID_*`, `IDC_SEL_BUMP_*` | Medium | P0 |
| Transformation | Toggleable line scaling | The "scale line width with the object" option. | `tools/selinfo.cpp` (`IDC_SEL_SCALELINES`), `Kernel/linwthop.cpp` | Low | P1 |
| Transformation | Aspect lock | Aspect-ratio padlock in the infobar. | `tools/selinfo.cpp` (`IDC_SEL_PADLOCK`) | Low | P0 |
| Transformation | Transformations inside groups | Correct propagation of matrices to children and to fills. | `Kernel/grptrans.cpp`, `Kernel/group.cpp` | High | P0 |
| Transformation | Keyboard nudge (24 variants) | Cursor keys ×{1, 5, 10, 1/5, pixel 1, pixel 10} × 4 directions. | `Kernel/opnudge.cpp`, `wxOil/xrc/STANDARD_HOTKEYS.res` | Low | P0 |
| Structure | Group / ungroup | `Ctrl+G` / `Ctrl+U`; nested groups; special ungroup (debug). | `Kernel/group.cpp`, `Kernel/groupops.cpp` | Medium | P0 |
| Structure | Full Z order | Bring to front, send to back, forwards/backwards one step, forwards/backwards one layer. | `Kernel/zordops.cpp` | Low | P0 |
| Structure | Alignment and distribution | Alignment dialog with 9 anchors + distribution on both axes. | `Kernel/aligndlg.cpp` (`OPTOKEN_OPALIGN`, `OPTOKEN_ARRANGEALIGNMENT`) | Medium | P0 |
| Structure | Duplicate (`Ctrl+D`) | Copy with a configurable offset. | `Kernel/cutop.cpp` (`OPTOKEN_DUPLICATE`), `Kernel/optsedit.cpp` (offset) | Low | P0 |
| Structure | Clone (`Ctrl+K`) | Copy at exactly the same position. | `Kernel/cutop.cpp` (`OPTOKEN_CLONE`) | Low | P0 |
| Structure | Cut / Copy / Paste / Paste in place | `Ctrl+X/C/V`, `Ctrl+Shift+V`. | `Kernel/cutop.cpp`, `wxOil/natclipm.cpp`, `wxOil/clipext.cpp` | Medium | P0 |
| Structure | Paste attributes (`Ctrl+Shift+A`) | Applies only the clipboard's attributes to the selection. | `Kernel/cutop.cpp` (`OPTOKEN_PASTEATTRIBUTES`) | Medium | P1 |
| Structure | Delete | `Del` / `Backspace`. | `Kernel/cutop.cpp` (`OPTOKEN_DELETE`) | Low | P0 |
| Structure | Drag and drop onto the document | Drag & drop of colours, bitmaps, files and gallery items onto the canvas. | `Kernel/sgdrag.cpp`, `Kernel/draginfo.cpp`, `wxOil/dragmgr.cpp`, `wxOil/dragtrgt.cpp`, `wxOil/dragcol.cpp`, `wxOil/dragbmp.cpp` | High | P1 |
| Structure | Precise hit-testing | Tested against real geometry (includes fill, outline, text and bitmaps with alpha). | `Kernel/hittest.cpp`, `Kernel/clicarea.cpp`, `wxOil/grndclik.cpp` | High | P0 |

### 1.5 Attributes and colour

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Attributes | Attribute system as nodes | Attributes are sibling nodes that affect the nodes that follow them; inheritance by tree traversal. | `Kernel/nodeattr.cpp`, `Kernel/attrmgr.cpp`, `Kernel/attrappl.cpp`, `Kernel/attraggl.cpp`, `Kernel/attrmap.cpp` | Very high | P0 |
| Attributes | Current / per-tool attributes | Each tool keeps its own "current" attributes for new objects. | `Kernel/attrmgr.cpp`, `Kernel/isetattr.cpp` | Medium | P0 |
| Attributes | Apply attribute interactively / repeat | `OPTOKEN_APPLYATTRINTERACTIVE`, `OPTOKEN_REPEATAPPLYATTRIB`. | `Kernel/attrappl.cpp` | Medium | P1 |
| Colour | Colour models: RGB, CMYK, HSV, greyscale, CIE | `COLOURMODEL_RGBT / CMYK / HSVT / GREYT / CIET`. Conversion via colour contexts. | `Kernel/colmodel.h`, `Kernel/colcontx.cpp`, `Kernel/colormgr.cpp` | High | P0 |
| Colour | Named document colours | Document colour list, naming, renaming, global editing (changes every use). | `Kernel/doccolor.cpp`, `Kernel/colcomp.cpp`, `Kernel/cnamecol.cpp`, `Kernel/newcol.cpp` | High | P0 |
| Colour | Linked colours (tint, shade, hue, link) | A colour can be derived from another as a tint/shade/hue; editing the parent changes all its children. **A key Xara differentiator.** | `Kernel/doccolor.cpp`, `Kernel/coldlog.cpp`, `Kernel/colourix.cpp` | High | P1 |
| Colour | Advanced colour editor | Dialog with a 2D picker + slider, numeric entry per model, colour type (normal/spot/tint…). | `Kernel/coldlog.cpp`, `wxOil/colpick.cpp`, `wxOil/colourmat.cpp`, `Kernel/colcontx.cpp` | High | P0 |
| Colour | Colour bar (on-screen palette) | Horizontal palette below the document; click = fill, right-click/`Shift` = line; context menu. | `wxOil/ccolbar.cpp`, `Kernel/colmenu.cpp`, `Kernel/palmenu.cpp`, `Kernel/collist.cpp`, `Kernel/colclist.cpp` | Medium | P0 |
| Colour | Eyedropper / colour picker (`Ctrl+E`) | Picks a colour from any pixel in the document or on screen. | `wxOil/dragpick.cpp`, `wxOil/colpick.cpp`, `Kernel/menuops.cpp` (`OPTOKEN_UTILCOLOUR`), cursors `IDC_COLOURPICKERCURSOR*` | Medium | P1 |
| Colour | No colour / null fill | Special "no colour" entry in the palette and gallery. | `Kernel/colgal.cpp`, `wxOil/ccolbar.cpp` (`IDC_EDIT_NOCOLOUR`) | Low | P0 |
| Colour | Palettes: sort by hue/luminance/use | `OPTOKEN_PALETTE_SORT_BY_HUE / LUMINANCE / USE`. | `Kernel/palmenu.cpp`, `Kernel/colgal.cpp` | Low | P2 |
| Colour | Web-safe palette and transparent colour | `OPTOKEN_PALETTE_WEB_SAFE`, `OPTOKEN_PALETTE_TRANSPARENT(_BACKGROUND)`. | `Kernel/palmenu.cpp`, `wxOil/gpalopt.cpp`, `wxOil/palman.cpp` | Medium | P2 |
| Colour | Spot colours and separation | Spot-ink support for printing. | `Kernel/colplate.cpp`, `Kernel/xsepsops.cpp` | High | P3 |
| Colour | Colour management / correction | Per-device colour contexts, monitor/printer calibration. | `Kernel/colormgr.cpp`, `Kernel/colcontx.cpp`, `Kernel/colcomp.cpp` | High | P3 |
| Line | Line width | `AttrLineWidth`, with a "hairline" width and optional scaling. | `Kernel/lineattr.cpp`, `Kernel/linwthop.cpp`, `Kernel/linedef.cpp` | Low | P0 |
| Line | Line colour and line transparency | `AttrStrokeColour`, `AttrStrokeTransp`. | `Kernel/lineattr.cpp` | Low | P0 |
| Line | Dash patterns (`AttrDashPattern`) | Gallery of dashed-line styles. | `Kernel/lineattr.cpp`, `Kernel/sgline.cpp`, `Kernel/sgline2.cpp` | Medium | P1 |
| Line | Caps and joins + mitre limit | `AttrStartCap`, `AttrJoinType`, `AttrMitreLimit`. | `Kernel/lineattr.cpp` | Low | P0 |
| Line | Start and end arrowheads | `AttrStartArrow`, `AttrEndArrow` with an arrowhead gallery. | `Kernel/arrows.cpp`, `Kernel/lineattr.cpp`, `Kernel/sgline.cpp` | Medium | P1 |
| Line | Stroke types | Vector strokes applied to the path (line gallery). | `Kernel/strkattr.cpp`, `Kernel/strkcomp.cpp`, `Kernel/mkstroke.cpp`, `Kernel/ppstroke.cpp`, `Kernel/ppvecstr.cpp`, `Kernel/sgstroke.cpp` | High | P2 |
| Line | Variable width / pressure | Width profile along the stroke, tablet input. | `Kernel/strkattr.cpp`, `Kernel/pressure.cpp`, `Kernel/brpress.cpp`, `wxOil/tablet.cpp` | High | P2 |
| Line | Brushes | Brushes based on objects repeated along the path, with spacing, rotation, scale, pressure and randomness. | `Kernel/brshattr.cpp`, `Kernel/brushop.cpp`, `Kernel/brushdlg.cpp`, `Kernel/brshdata.cpp`, `Kernel/brshcomp.cpp`, `Kernel/ppbrush.cpp`, `Kernel/ndbrshmk.cpp`, `Kernel/sgbrush.cpp`, `tools/opdrbrsh.cpp` | Very high | P2 |
| Styles | Styles / named attributes | `AttrStyle`, application of named attribute sets. | `Kernel/styles.cpp`, `Kernel/userattr.cpp` | Medium | P3 |

### 1.6 Fills and transparencies

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Fills | Flat fill | `AttrFlatColourFill`. | `Kernel/fillattr.cpp` | Low | P0 |
| Fills | Linear fill (gradient) | `AttrLinearColourFill` with start/end handles on the canvas. | `Kernel/fillattr.cpp`, `tools/filltool.cpp` | Medium | P0 |
| Fills | Radial / circular fill | `AttrRadialColourFill`, `AttrCircularColourFill` with a draggable centre and radii. | `Kernel/fillattr.cpp`, `tools/filltool.cpp` | Medium | P0 |
| Fills | Conical fill | `AttrConicalColourFill`. | `Kernel/fillattr.cpp` | Medium | P1 |
| Fills | Square fill | `AttrSquareColourFill` (gradient across 4 corners). | `Kernel/fillattr.cpp` | Medium | P2 |
| Fills | Three- and four-colour fills | `AttrThreeColColourFill`, `AttrFourColColourFill`: simple mesh gradients. **A Xara differentiator.** | `Kernel/fillattr.cpp`, `tools/filltool.cpp` | High | P2 |
| Fills | Multi-stop ramps | Several intermediate colour stops in a gradient, draggable on the canvas. | `Kernel/fillramp.cpp`, `Kernel/gradtbl.cpp`, `tools/filltool.cpp` | High | P1 |
| Fills | Bitmap fill | `AttrBitmapColourFill` with scale/rotation/shear handles for the bitmap, and tiling. | `Kernel/fillattr.cpp`, `Kernel/bitmapfx.cpp`, `tools/filltool.cpp` | High | P1 |
| Fills | Fractal and noise fills | `AttrFractalColourFill`, `AttrNoiseColourFill`, procedural textures with grain and seed. | `Kernel/fracfill.cpp`, `Kernel/fraclist.cpp`, `Kernel/noise1.cpp`, `Kernel/noisebas.cpp`, `Kernel/noisef.cpp` | High | P3 |
| Fills | Fill mapping: linear / sine | `AttrFillMappingLinear`, `AttrFillMappingSin` — the interpolation profile. | `Kernel/fillattr.cpp` | Low | P2 |
| Fills | Fill effects: fade / rainbow / alt rainbow | Interpolation through RGB or around the colour wheel in either direction. **A Xara differentiator.** | `Kernel/fillattr.cpp` (`AttrFillEffectFade/Rainbow/AltRainbow`) | Medium | P1 |
| Fills | Fill profile (bias/gain) | Acceleration curve of the gradient, editable with an interactive gadget. | `Kernel/biasgain.cpp`, `Kernel/biasdlg.cpp`, `Kernel/biasgdgt.cpp` (`OPTOKEN_FILLPROFILE`) | Medium | P1 |
| Fills | Editing the fill **on the canvas** | Gradient arrows and blobs directly on the object; dragging a palette colour onto an endpoint changes that stop. **Xara's most characteristic differentiator.** | `tools/filltool.cpp`, `Kernel/opgrad.cpp`, `Kernel/fillndge.cpp`, cursors `IDC_CANDROPONFILL*` | High | P0 |
| Fills | Keyboard fill nudge | 24 variants, `OPTOKEN_FILLNUDGE*`. | `Kernel/fillndge.cpp` | Low | P2 |
| Fills | Mutate fill (change type while keeping the colours) | `OPTOKEN_MUTATEFILL`. | `Kernel/fillattr.cpp`, `tools/filltool.cpp` | Medium | P1 |
| Fills | Perspective fills inside moulds | The fill is deformed along with the envelope/perspective. | `Kernel/nodemold.cpp` (`RemovePerspectiveFills`), `Kernel/gmould.cpp` | High | P3 |
| Transp. | Transparency tool (`TOOL17`) | The same geometric types as fills, but applied to the alpha channel, editable on the canvas. | `tools/filltool.cpp` (`TranspTool`), `Kernel/fillattr.cpp` | High | P0 |
| Transp. | Flat transparency | `AttrFlatTranspFill`. | `Kernel/fillattr.cpp` | Low | P0 |
| Transp. | Graduated transparency (linear/radial/conical/square/3–4 colour/bitmap/fractal) | Every fill geometry replicated for transparency. | `Kernel/fillattr.cpp` | High | P1 |
| Transp. | Mix types: Mix, Stained Glass, Bleach | The base compositing modes for transparency. | `Kernel/fillval.h` (`TranspType`), `Kernel/fillattr.cpp`, `GDraw/gdraw.h` | Medium | P0 |
| Transp. | Advanced modes: Contrast, Saturation, Darken, Lighten, Brightness, Luminosity, Hue, Bevel | *Blend mode* style compositing modes, in both flat and graduated variants. **A Xara differentiator.** | `Kernel/fillval.h`, `GDraw/gdraw.h`, `wxOil/grndrgn.cpp` | High | P2 |
| Transp. | Group transparency | Applying transparency to a group as a unit (`OPTOKEN_GROUPTRANSP`, `OPTOKEN_UNGROUPTRANSP`). | `Kernel/group.cpp`, `Kernel/fillattr.cpp` | High | P1 |
| Transp. | Transparency profile | `OPTOKEN_TRANSPFILLPROFILE` (bias/gain on alpha). | `Kernel/biasgain.cpp`, `tools/filltool.cpp` | Low | P2 |
| Transp. | Feather (edge fade) | Softening of the object's edge with a width and a profile. **A Xara differentiator.** | `Kernel/opfeathr.cpp`, `Kernel/fthrattr.cpp`, `Kernel/fthrconv.cpp`, `wxOil/fthelper.cpp` | High | P1 |

### 1.7 Text

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Text | Text tool (`TOOL21`, `F8`) | Simple (point) text, column text (fixed width) and text on a path. | `tools/texttool.cpp`, `tools/textinfo.cpp`, `tools/textops.cpp` | Very high | P0 |
| Text | Text model (story/line/char) | `NodeTextStory` → `NodeTextLine` → `TextChar` tree. | `Kernel/nodetext.cpp`, `Kernel/nodetxtl.cpp`, `Kernel/nodetxts.cpp` | High | P0 |
| Text | Text on a path | `Fit Text to Curve`, with path reversal (`Ctrl+Shift+R`). | `Kernel/ndtxtpth.cpp`, `tools/textops.cpp` (`OPTOKEN_FITTEXTTOPATH`, `OPTOKEN_REVERSESTORYPATH`) | High | P1 |
| Text | On-canvas editing with a caret | Insertion, selection with mouse and keyboard, selection dragging. | `tools/texttool.cpp`, `Kernel/textacts.cpp` (`OPTOKEN_TEXTSELECTION`) | High | P0 |
| Text | Typeface and size | `AttrTxtFontTypeface`, `AttrTxtFontSize`. | `Kernel/txtattr.cpp`, `Kernel/fontman.cpp`, `Kernel/fontlist.cpp` | Medium | P0 |
| Text | Bold / italic / underline | `AttrTxtBold`, `AttrTxtItalic`, `AttrTxtUnderline`. | `Kernel/txtattr.cpp` | Low | P0 |
| Text | Left/centre/right/full justification | `AttrTxtJustification` + 4 optokens. | `Kernel/txtattr.cpp`, `tools/textops.cpp` | Low | P0 |
| Text | Tracking and kerning (incl. auto-kern) | Global spacing adjustment and per-pair kerning; manual X/Y kerning. | `Kernel/txtattr.cpp`, `tools/textops.cpp` (`OPTOKEN_KERNTEXT`, `OPTOKEN_AUTOKERNTEXT`) | Medium | P1 |
| Text | Line spacing | `AttrTxtLineSpace`, absolute or proportional. | `Kernel/txtattr.cpp` | Low | P0 |
| Text | Glyph aspect ratio | `AttrTxtAspectRatio` (horizontal stretch). | `Kernel/txtattr.cpp` | Low | P2 |
| Text | Superscript / subscript / baseline | `AttrTxtScript`, `AttrTxtBaseLine`. | `Kernel/txtattr.cpp` | Low | P1 |
| Text | Margins, first-line indent and tab stops | `AttrTxtLeftMargin`, `AttrTxtRightMargin`, `AttrTxtFirstIndent`, `AttrTxtRuler` with L/C/R/decimal tab stops draggable on the ruler. | `Kernel/txtattr.cpp`, `tools/textinfo.cpp`, cursors `IDCSR_TEXT_*TAB.cur` | High | P2 |
| Text | Interactive text ruler | Contextual ruler with margins and tab stops while editing. | `tools/texttool.cpp`, `Kernel/rulers.cpp`, `wxOil/oilruler.cpp` | High | P2 |
| Text | Font management (FreeType / TrueType / ATM) | Enumeration, caching, substitution of missing fonts, panose matching. | `Kernel/fontman.cpp`, `Kernel/fntcache.cpp`, `Kernel/ccpanose.cpp`, `Kernel/fttyplis.cpp`, `wxOil/ftfonts.cpp`, `wxOil/ttfonts.cpp`, `wxOil/atmfonts.cpp`, `wxOil/fontbase.cpp` | High | P0 |
| Text | Font gallery with preview | Per-font thumbnails, generated in the background. | `wxOil/sgfonts.cpp`, `wxOil/sgdfonts.cpp`, `wxOil/fontpgen.cpp`, `Kernel/crthumb.cpp` | Medium | P1 |
| Text | Converting text to paths | Via "Convert to shapes". | `Kernel/shapeops.cpp`, `Kernel/nodetext.cpp` | Medium | P0 |
| Text | Paste text as text | `OPTOKEN_TEXTPASTE`. | `tools/textops.cpp`, `wxOil/natclipm.cpp` | Low | P1 |
| Text | Unicode and encodings | Unicode string handling, encoding conversion. | `wxOil/unicdman.cpp`, `Kernel/impstr.cpp` | Medium | P0 |

### 1.8 Bitmaps and photos

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Bitmaps | Bitmap node and embedded bitmaps | Bitmaps stored in the document with a reference list and a use count. | `Kernel/nodebmp.cpp`, `Kernel/bitmap.cpp`, `Kernel/bmplist.cpp`, `wxOil/oilbitmap.cpp` | High | P0 |
| Bitmaps | Bitmap and node caches | Per-node render cache, with FIFO/random/weak policies and a memory limit. | `Kernel/bitmapcache.cpp`, `Kernel/bitmapcachekey.cpp`, `Kernel/nodecach.cpp`, `Kernel/cache.cpp`, `Kernel/cachfifo.cpp`, `Kernel/cachrand.cpp`, `Kernel/cachweak.cpp` | High | P1 |
| Bitmaps | Bitmap gallery | List of the document's bitmaps with thumbnails, deletion, replacement and info. | `Kernel/sgbitmap.cpp`, `Kernel/bmplist.cpp` | Medium | P0 |
| Bitmaps | Properties and depth conversion | 1/4/8/24/32 bpp, indexed palette, alpha. | `Kernel/bmpcomp.cpp`, `wxOil/dibconv.cpp`, `wxOil/dibutil.cpp`, `wxOil/cbmpdata.cpp` | Medium | P1 |
| Bitmaps | Bitmap effects (Bfx): brightness/contrast | Dialog with a preview. | `Kernel/bfxdlg.cpp`, `Kernel/bfxbase.cpp`, `Kernel/bfxop.cpp`, `wxOil/bfxalu.cpp`, `wxOil/bfxpixop.cpp` | Medium | P2 |
| Bitmaps | Bfx: colour depth / palette | Colour reduction with dithering. | `Kernel/bfxdlg.cpp` (`OPTOKEN_BFXDLG_COLOURDEPTH`), `wxOil/gpalopt.cpp` | Medium | P2 |
| Bitmaps | Bfx: flip and rotate | 90/180/270 rotation and flips of the bitmap. | `Kernel/bfxdlg.cpp` (`OPTOKEN_BFXDLG_FLIPROTATE`) | Low | P2 |
| Bitmaps | Bfx: resize | Resolution change with resampling. | `Kernel/bfxdlg.cpp` (`OPTOKEN_BFXDLG_RESIZE`) | Medium | P2 |
| Bitmaps | Bfx: special effects (menu of 10+) | Blur, sharpen, emboss, etc. | `Kernel/bfxdlg.cpp` (`OPTOKEN_BFX_SPECIALEFFECTS`), `Kernel/bfxatom.cpp`, `Kernel/bfxitem.cpp`, `Kernel/bfxmngr.cpp` | Medium | P3 |
| Bitmaps | Bitmap-effect plug-ins (Photoshop style) | Manager for external plug-ins, with undo. | `Kernel/plugin.cpp`, `Kernel/plugmngr.cpp`, `Kernel/plugop.cpp`, `Kernel/plugopun.cpp`, `Kernel/bfxopun.cpp`, `Kernel/optsplug.cpp` | High | P3 |
| Bitmaps | Bitmap tracer (auto-trace) | Converts a bitmap into vectors (dialog with tracing parameters). | `Kernel/tracedlg.cpp`, `Kernel/tracectl.cpp`, `Kernel/tracergn.cpp` | Very high | P2 |
| Bitmaps | Bitmap fill with handles | See §1.6. | `tools/filltool.cpp` | High | P1 |
| Bitmaps | Bitmap transparency (mask) | `AttrBitmapTranspFill` as a procedural alpha channel. | `Kernel/fillattr.cpp`, `Kernel/maskedrr.cpp`, `wxOil/maskfilt.cpp` | High | P2 |
| Bitmaps | Bitmap preview / thumbnails | Thumbnails for the galleries and for the native format. | `Kernel/bmapprev.cpp`, `Kernel/crthumb.cpp`, `wxOil/thumb.cpp`, `Kernel/prvwflt.cpp` | Medium | P1 |
| Bitmaps | Bitmaps in "sequence" mode (animation) | Bitmap sequences for animated GIF. | `Kernel/bmpseq.cpp`, `Kernel/bmpsrc.cpp` | Medium | P3 |
| Bitmaps | Asynchronous bitmap import | Background loading with a progress bar. | `Kernel/impbmp.cpp` (`OPTOKEN_ASYNCHBITMAPIMPORT`), `wxOil/progress.cpp` | Medium | P2 |

### 1.9 Live Effects and post-processing

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Live FX | Live Effects tool (`TOOL26`) | Applies **non-destructive** bitmap effects to vector objects; the vector stays editable. **A Xara differentiator.** | `tools/liveeffectstool.cpp`, `tools/liveeffectsinfo.cpp`, `tools/opliveeffects.cpp` | Very high | P3 |
| Live FX | Effects stack | Several chained effects per object, reorderable, with a lock and a resolution per effect. | `Kernel/effects_stack.cpp`, `Kernel/nodeliveeffect.cpp`, `Kernel/nodepostpro.cpp` | High | P3 |
| Live FX | Add / edit / insert / remove / remove all | `IDC_CCBUTTON_LE_*` buttons in the infobar. | `tools/opliveeffects.cpp` (`OPTOKEN_APPLY_LIVEEFFECT`, `OPTOKEN_EDIT_LIVEEFFECT`, `OPTOKEN_DELETE_LIVEEFFECT`, `OPTOKEN_DELETEALL_LIVEEFFECT`) | Medium | P3 |
| Live FX | Effect lock and resolution | `OPTOKEN_CHANGE_EFFECT_LOCK`, `..._LOCKALL`, `..._RES`. | `tools/opliveeffects.cpp`, `Kernel/effects_stack.cpp` | Medium | P3 |
| Live FX | XPE (Xara Picture Editor) bridge | External filter/plug-in that receives Xar data in order to edit the effect. | `Kernel/xpfilter.cpp`, `Kernel/xpfcaps.cpp`, `Kernel/xpfrgn.cpp`, `wxOil/xpoilflt.cpp` (`OPTOKEN_XPE_EDIT`) | High | P3 |
| Live FX | Legacy effects | Compatibility with effects from earlier versions. | `tools/opliveeffects.cpp` (`OPTOKEN_EDIT_LEGACYEFFECT`) | Low | P3 |
| Live FX | Offscreen attribute / rendering to an intermediate bitmap | Render-to-auxiliary-surface infrastructure for effects and group transparency. | `Kernel/offattr.cpp`, `wxOil/offscrn.cpp`, `Kernel/pmaskrgn.cpp` | High | P2 |

### 1.10 Blend, Mould, Contour, Shadow, Bevel, ClipView

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Blend | Blend tool (`TOOL16`, `F7`) | Interpolation between two or more objects: shape, colour, attributes and position. **A Xara differentiator.** | `tools/blndtool.cpp`, `Kernel/nodeblnd.cpp`, `Kernel/gblend.cpp`, `Kernel/blndhelp.cpp` | Very high | P2 |
| Blend | Number of steps / distance between steps | Two ways of controlling the blend. | `tools/blndtool.cpp` (`OPTOKEN_CHANGEBLENDSTEPS`, `OPTOKEN_CHANGEBLENDDISTANCE`) | Medium | P2 |
| Blend | Blend profiles (object and attributes) | Independent bias/gain curves for position and for attributes. | `tools/blndtool.cpp` (`OPTOKEN_CHANGEBLENDPROFILE`), `Kernel/biasgain.cpp` | Medium | P2 |
| Blend | Blend along a path | Attach/detach a guide path, tangential mode, one-to-one, antialias. | `tools/blndtool.cpp` (`OPTOKEN_ADDBLENDPATH`, `OPTOKEN_DETACHBLENDPATH`, `OPTOKEN_BLENDTANGENTIAL`, `OPTOKEN_BLENDONETOONE`, `OPTOKEN_BLENDANTIALIAS`) | High | P2 |
| Blend | Blend node remapping | Adjusting the correspondence of points between the two ends. | `tools/blndtool.cpp`, cursor `IDC_BLENDABLEREMAPCURSOR.cur` | High | P3 |
| Blend | Edit the blend's end objects | `OPTOKEN_EDITBLENDENDOBJECT`; live editing with the blend recomputing. | `tools/blndtool.cpp` | High | P2 |
| Blend | Remove blend | `OPTOKEN_REMOVEBLEND`. | `Kernel/nodeblnd.cpp` | Low | P2 |
| Mould | Mould tool (`TOOL19`, `F6`) | Envelopes and perspectives applied to any object or group. **A Xara differentiator.** | `tools/moldtool.cpp`, `Kernel/nodemold.cpp`, `Kernel/moldshap.cpp`, `Kernel/moldedit.cpp` | Very high | P2 |
| Mould | Rectangular envelope and presets | Default, banner, circular, concave and elliptical envelopes. | `Kernel/moldenv.cpp`, resources `IDC_BTN_*ENVELOPE` | High | P2 |
| Mould | Rectangular perspective and presets | Default, floor, roof, left and right perspectives; draggable vanishing point. | `Kernel/moldpers.cpp` (`OPTOKEN_DRAGVANISHPOINT`) | High | P2 |
| Mould | Copy / paste envelope and perspective | `OPTOKEN_COPYMOULD`, `OPTOKEN_PASTEENVELOPE`, `OPTOKEN_PASTEPERSPECTIVE`. | `tools/moldtool.cpp` | Medium | P3 |
| Mould | Rotate / detach / remove mould, mould grid | `OPTOKEN_ROTATEMOULD`, `OPTOKEN_DETACHMOULD`, `OPTOKEN_REMOVEMOULD`, `OPTOKEN_TOGGLEMOULDGRID`. | `tools/moldtool.cpp`, `Kernel/gmould.cpp` | Medium | P3 |
| Mould | Moulded nodes (group, ink, path) | Infrastructure for deformed nodes. | `Kernel/ndmldgrp.cpp`, `Kernel/ndmldink.cpp`, `Kernel/ndmldpth.cpp`, `Kernel/nodemldr.cpp` | High | P2 |
| Contour | Contour tool (`TOOL24`, `Ctrl+F7`) | Inner/outer contours with N steps, distance, join type and colour transition. | `tools/cntrtool.cpp`, `tools/opcntr.cpp`, `Kernel/nodecntr.cpp`, `Kernel/ncntrcnt.cpp` | Very high | P2 |
| Contour | Contour parameters | Steps, distance/width, inner/outer, object and attribute profiles, join type (mitre/round/bevel), colour type. | `tools/cntrtool.cpp` (`OPTOKEN_CHANGECONTOUR*`) | High | P2 |
| Shadow | Shadow tool (`TOOL22`, `Ctrl+F2`) | Real-time soft shadows: wall, floor, glow and feather. **A Xara differentiator.** | `tools/shadtool.cpp`, `tools/opshadow.cpp`, `tools/shadinfo.cpp`, `tools/ShadowTl.cpp`, `Kernel/nodeshad.cpp`, `Kernel/bshadow.cpp` | Very high | P1 |
| Shadow | Shadow parameters | Position, angle, height, scale, penumbra (blur), darkness, profile. | `tools/opshadow.cpp` (`OPTOKEN_SHADOWANGLE`, `SHADOWHEIGHT`, `SHADOWPENUMBRA`, `SHADOWDARKNESS`, `SHADOWPROFILE`, `SHADOWSCALE`) | High | P1 |
| Shadow | Glow | Halo-style shadow with its own width. | `tools/opshadow.cpp` (`OPTOKEN_GLOWWIDTH`) | Medium | P2 |
| Bevel | Bevel tool (`TOOL23`, `Ctrl+F3`) | 3D inner/outer bevels with a profile type, light angle and tilt, and contrast. | `tools/bevtool.cpp`, `tools/opbevel.cpp`, `tools/bevinfo.cpp`, `Kernel/nodebev.cpp`, `Kernel/beveler.cpp`, `Kernel/attrbev.cpp`, `Kernel/bevfill.cpp`, `Kernel/nbevcont.cpp`, `Kernel/ppbevel.cpp`, `Kernel/bevtrap.cpp` | Very high | P2 |
| Bevel | Bevel parameters | Indent, light angle, tilt, contrast, type, joins (mitre/round/bevel), inner/outer. | `Kernel/attrbev.cpp` (`AttrBevelIndent`, `AttrBevelLightAngle`, `AttrBevelContrast`, `AttrBevelType`, `AttrBevelLightTilt`) | High | P2 |
| ClipView | Apply / remove ClipView | Clipping objects by the topmost shape (live vector mask). | `tools/opclip.cpp` (`OPTOKEN_APPLY_CLIPVIEW`, `OPTOKEN_REMOVE_CLIPVIEW`), `Kernel/nodeclip.cpp`, `Kernel/clipattr.cpp`, `Kernel/ndclpcnt.cpp`, `Kernel/clipint.cpp` | High | P1 |
| Feather | Feather on any object | See §1.6; includes profile and size on a dedicated infobar (`IDD_BUTTBAR_FEATHER`). | `Kernel/opfeathr.cpp` (`OPTOKEN_FEATHER`, `OPTOKEN_UNFEATHER`, `OPTOKEN_FEATHERSIZE`, `OPTOKEN_FEATHERPROFILE`) | High | P1 |

### 1.11 Galleries and panels

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Galleries | Gallery infrastructure | Generic dockable panel with a group tree, search, sorting, options and dragging. | `Kernel/sgallery.cpp`, `Kernel/sgbase.cpp`, `Kernel/sgtree.cpp`, `Kernel/sgmenu.cpp`, `Kernel/sginit.cpp`, `Kernel/gallery.cpp`, `wxOil/galbar.cpp` | High | P1 |
| Galleries | Colour gallery (`F9`) | Document colours and palettes; create/edit/delete/rename, drag onto objects. | `Kernel/sgcolour.cpp`, `Kernel/colgal.cpp` | Medium | P0 |
| Galleries | Layer gallery (`F10`) | See §1.1. | `Kernel/sglayer.cpp`, `Kernel/layergal.cpp` | Medium | P0 |
| Galleries | Bitmap gallery (`F11`) | See §1.8. | `Kernel/sgbitmap.cpp` | Medium | P0 |
| Galleries | Line gallery (`F12`) | Line styles, dashes, arrowheads, stroke types and brushes. | `Kernel/sgline.cpp`, `Kernel/sgline2.cpp`, `Kernel/sgstroke.cpp`, `Kernel/sgbrush.cpp`, `wxOil/sglinepr.cpp` | Medium | P1 |
| Galleries | Font gallery (`Shift+F9`) | See §1.7. | `wxOil/sgfonts.cpp`, `wxOil/sgdfonts.cpp` | Medium | P1 |
| Galleries | Clipart gallery (`Shift+F10`) | On-disk indexed library with thumbnails and web download. | `Kernel/sglcart.cpp`, `Kernel/sglib.cpp`, `Kernel/sglbase.cpp`, `Kernel/sgscan.cpp`, `wxOil/sgliboil.cpp`, `wxOil/sgindgen.cpp` | High | P3 |
| Galleries | Fill gallery (`Shift+F11`) | Library of predefined textures/gradients. | `Kernel/sglfills.cpp` | Medium | P3 |
| Galleries | Frame gallery (`Shift+F12`) | Animation frames: new, copy, delete, move, properties, preview. | `Kernel/sgframe.cpp`, `Kernel/frameops.cpp` | Medium | P3 |
| Galleries | Name gallery (`Ctrl+Shift+F9`) | Names/sets applied to objects: selection by name, exportability, web triggers. The basis of the rollover and slice model. | `Kernel/sgname.cpp`, `Kernel/ngcore.cpp`, `Kernel/ngdialog.cpp`, `Kernel/ngitem.cpp`, `Kernel/ngiter.cpp`, `Kernel/ngprop.cpp`, `Kernel/ngscan.cpp`, `Kernel/ngsentry.cpp`, `Kernel/ngsetop.cpp`, `Kernel/ngdrag.cpp` | High | P2 |
| Galleries | Web library download | Add web folders/libraries, download thumbnails and items. | `Kernel/inetop.cpp` (`OPTOKEN_OPADDWEBFOLDERS`, `OPTOKEN_OPDOWNLOAD`, `OPTOKEN_OPTHUMBDOWNLOAD`), `wxOil/camnet.cpp`, `wxOil/lddirect.cpp` | High | P3 |
| Galleries | Gallery search / sort / options | `OPTOKEN_SGSEARCHDLG`, `OPTOKEN_SGSORTDLG`, `OPTOKEN_SGOPTIONSDLG`. | `Kernel/sgmenu.cpp`, `Kernel/sgscanf.cpp` | Medium | P3 |
| Panels | Dockable, customisable bars | Configurable button bars (`ToolbarDlg`), 11 predefined bars + status bar, rulers and colour bar. | `Kernel/bars.cpp`, `Kernel/stdbars.cpp`, `Kernel/barcreationdlg.cpp`, `wxOil/basebar2.cpp`, `wxOil/dockbar.cpp`, `wxOil/cstatbar.cpp` | High | P2 |
| Panels | Contextual per-tool infobar | Every tool publishes its own options bar. | `Kernel/infobar.cpp`, `tools/*info.cpp`, `wxOil/xrc/*bar*.xrc` | High | P0 |
| Panels | Status bar with indicators | Coordinates, snapping, render quality, print mode, transparency. | `Kernel/statline.cpp`, `wxOil/cstatbar.cpp`, resources `IDB_SL_*` | Medium | P1 |
| Panels | Context menus | Context-sensitive right-button menu (object, gallery, palette, ruler). | `Kernel/contmenu.cpp`, `Kernel/colmenu.cpp`, `Kernel/palmenu.cpp`, `Kernel/prvwmenu.cpp`, `wxOil/oilmenus.cpp`, `Kernel/menuitem.cpp` | Medium | P0 |
| Panels | Bubble help / status help | Per-gadget help text in the status bar and as a tooltip. | `wxOil/bblwnd.cpp`, `wxOil/ctrlhelp.cpp`, `wxOil/helptabs.cpp` | Low | P2 |

### 1.12 Import

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Import | Filter architecture | Filter registry, families (vector/bitmap/text/palette), detection by content and extension, per-filter options. | `Kernel/filters.cpp`, `Kernel/filtrmgr.cpp`, `Kernel/camfiltr.cpp`, `Kernel/impexpop.cpp`, `wxOil/oilfltrs.cpp` | High | P0 |
| Import | Native `.xar` format (v2, compressed) | Reading the Xar format: tree of typed records, zlib compression, references, embedded bitmaps. | `Kernel/native.cpp`, `Kernel/cxfile.cpp`, `Kernel/cxftree.cpp`, `Kernel/cxfrec.cpp`, `Kernel/cxftfile.cpp`, `Kernel/zinflate.cpp`, `Kernel/zstream.cpp`, `Kernel/rech*.cpp` | Very high | P0 |
| Import | `.web` format (minimal native web format) | A reduced variant of the native format. | `Kernel/webfiltr.cpp` | Medium | P2 |
| Import | SVG | External SVG filter (imports geometry, styles and gradients). | `filters/SVGFilter/svgimporter.cpp`, `import.cpp`, `gradients.cpp`, `styles.cpp`, `svgfilter.cpp` | Very high | P0 |
| Import | PNG | Native import with alpha. | `wxOil/pngfiltr.cpp`, `wxOil/pngutil.cpp`, `Kernel/pngfuncs.cpp` | Low | P0 |
| Import | JPEG | Native import (libjpeg). | `Kernel/imjpeg.cpp`, `Kernel/jpgsrc.cpp`, `Kernel/jpgermgr.cpp`, `Kernel/jpgprgrs.cpp` | Low | P0 |
| Import | GIF (incl. animated) | Import with transparency and sequences. | `wxOil/giffiltr.cpp`, `wxOil/gifutil.cpp`, `Kernel/bmpseq.cpp` | Medium | P1 |
| Import | BMP / DIB | Via ImageMagick and a bespoke filter. | `wxOil/imgmgkft.cpp`, `wxOil/dibutil.cpp`, `Kernel/bitfilt.cpp` | Low | P1 |
| Import | TIFF, PSD, PDF, PICT, PNM/PPM, XPM, ICO, PCD | Delegated to the external **ImageMagick** binary (`convert`). | `wxOil/imgmgkft.cpp`, `Kernel/filters.cpp` (the `ImageMagickFilter*` block) | Medium | P2 |
| Import | Native PPM / PGM / PBM | Bespoke filters. | `wxOil/ppmfiltr.cpp` | Low | P3 |
| Import | Generic EPS + Adobe Illustrator (AI, AI5, AI8) | Partial PostScript interpreter with a stack and operator handlers. | `Kernel/epsfiltr.cpp`, `Kernel/epsstack.cpp`, `Kernel/epsclist.cpp`, `Kernel/epssitem.cpp`, `Kernel/epscdef.cpp`, `Kernel/ai_eps.cpp`, `Kernel/ai5_eps.cpp`, `Kernel/ai8_eps.cpp`, `Kernel/ai_grad.cpp`, `Kernel/ai_layer.cpp`, `Kernel/ai_bmp.cpp`, `Kernel/ai_epsrr.cpp` | Very high | P2 |
| Import | EPS from Photoshop, FreeHand, CorelDRAW 3/4, ArtWorks | Variants of the EPS interpreter. | `Kernel/coreleps.cpp`, `Kernel/freeeps.cpp`, `Kernel/aw_eps.cpp`, `Kernel/cameleps.cpp`, `Kernel/nativeps.cpp` | High | P3 |
| Import | CorelDRAW `.cdr` | Native CDR filter (fill, outline, text). | `Kernel/cdrfiltr.cpp`, `Kernel/cdrfill.cpp`, `Kernel/cdroutl.cpp`, `Kernel/cdrtext.cpp`, `wxOil/cdrbitm.cpp` | Very high | P3 |
| Import | Corel CMX 16/32 bit | Complete CMX filter with a record tree. | `Kernel/cmxifltr.cpp`, `Kernel/cmxfiltr.cpp`, `Kernel/cmxicmds.cpp`, `Kernel/cmxibits.cpp`, `Kernel/cmxirefs.cpp`, `Kernel/cmxistut.cpp`, `Kernel/cmxrendr.cpp`, `Kernel/cmxtree.cpp`, `Kernel/cmxdcobj.cpp` | Very high | P3 |
| Import | WMF / EMF (Windows metafiles) | Metafile filter. | `wxOil/metafilt.cpp`, `wxOil/metaview.cpp`, `wxOil/clipmap.cpp` | High | P3 |
| Import | Acorn Draw, Sprite (RISC OS) | Historic formats. | `Kernel/drawfltr.cpp`, `wxOil/` (SpriteFilter) | Medium | P3 |
| Import | Palettes: MS, PaintShop Pro, Corel, Adobe (ACO/ACT), JCW | Colour-palette filters. | `Kernel/impcol.cpp`, `Kernel/expcol.cpp`, `wxOil/oilfltrs.cpp` | Low | P3 |
| Import | ANSI / Unicode / RTF text | Text filters (conditional). | `Kernel/textfltr.cpp`, `Kernel/impstr.cpp` | Medium | P3 |
| Import | Import from a URL (`Ctrl+Shift+Y`) | Downloads and imports a remote resource. | `Kernel/urlimp.cpp`, `Kernel/inetop.cpp`, `wxOil/camnet.cpp` | Medium | P3 |
| Import | Import onto layers / options | Preferences: import with layers, open with layers, bitmaps onto layers, unnamed colours. | `Kernel/filters.cpp` (prefs `ImportWithLayers`, `OpenWithLayers`, `ImportBitmapsOntoLayers`, `AddUnnamedColours`) | Low | P1 |
| Import | Drop a file onto the window | `OPTOKEN_DROPPEDFILE`. | `Kernel/impexpop.cpp`, `wxOil/dragtrgt.cpp` | Low | P1 |

### 1.13 Export

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Export | Export dialog with preview | Preview with option comparison, estimated file size, zoom. | `Kernel/bmpsdlg.cpp`, `Kernel/bmpexdoc.cpp`, `Kernel/bmpexprw.cpp`, `Kernel/prevwdlg.cpp`, `wxOil/filesize.cpp` | High | P1 |
| Export | Export selection / page / drawing | Configurable export scope, with DPI and antialiasing. | `Kernel/expbmp.cpp`, `Kernel/impexpop.cpp` | Medium | P0 |
| Export | PNG (with alpha, interlacing, palette) | Native exporter. | `wxOil/outptpng.cpp`, `wxOil/pngfiltr.cpp` | Low | P0 |
| Export | JPEG (quality, progressive) | Native exporter. | `Kernel/exjpeg.cpp`, `Kernel/jpgdest.cpp` | Low | P0 |
| Export | GIF (palette, dithering, transparency, interlacing) | Exporter with palette optimisation. | `wxOil/outptgif.cpp`, `wxOil/giffiltr.cpp`, `wxOil/gpalopt.cpp`, `wxOil/palman.cpp` | Medium | P1 |
| Export | Animated GIF | Saves the layers/frames as an animated GIF with delays and disposal. | `Kernel/frameops.cpp` (`OPTOKEN_SAVEANIMATEDGIF`), `Kernel/animparams.cpp`, `Kernel/bmpseq.cpp`, `wxOil/outptgif.cpp` | High | P3 |
| Export | BMP / DIB | Native exporter. | `wxOil/outptdib.cpp` | Low | P2 |
| Export | TIFF / PSD / PDF / others via ImageMagick | Delegated to the external binary. | `wxOil/imgmgkft.cpp` | Medium | P2 |
| Export | Native `.xar` format | Writing the record tree with compression. **[Superseded: architecture §3.5 makes writing .xar a non-goal]** | `Kernel/native.cpp`, `Kernel/nativeop.cpp`, `Kernel/cxf*.cpp`, `Kernel/zdeflate.cpp`, `Kernel/zdftrees.cpp` | Very high | P0 |
| Export | `.web` format | Native web save (`OPTOKEN_SAVEASWEB`). | `Kernel/webfiltr.cpp` | Medium | P3 |
| Export | SVG | External SVG export filter with an options dialog. | `filters/SVGFilter/export.cpp`, `svgexportdialog.cpp`, `svgfilterui.cpp` | High | P0 |
| Export | EPS / PostScript | EPS export (Camelot-native and generic), with a PS prologue. | `Kernel/saveeps.cpp`, `Kernel/cameleps.cpp`, `Kernel/nativeps.cpp`, `Kernel/psrndrgn.cpp`, `wxOil/psdc.cpp`, `wxOil/xrc/prolog.ps`, `setup.ps`, `spotfunc.ps` | High | P2 |
| Export | Flash / SWF | Vector exporter to Flash: shapes, text, bitmaps, sprites, buttons. (Built only in debug in Xara LX.) | `Kernel/swffiltr.cpp`, `Kernel/swfexpdc.cpp`, `Kernel/swfshape.cpp`, `Kernel/swftext.cpp`, `Kernel/swfbitmp.cpp`, `Kernel/swfsprit.cpp`, `Kernel/swfbuttn.cpp`, `Kernel/swfplace.cpp`, `Kernel/swffont.cpp`, `Kernel/swfrndr.cpp` | Very high | P3 |
| Export | HTML + imagemap | Generates an HTML page with an image map from the objects' URLs. | `Kernel/htmlexp.cpp`, `Kernel/htmlfltr.cpp`, `Kernel/htmllist.cpp`, `Kernel/imagemap.cpp` | High | P3 |
| Export | Image slicing (`Ctrl+I`) | Cuts the drawing into pieces according to named objects and exports HTML + images + rollovers. | `Kernel/slice.cpp`, `Kernel/slicehelper.cpp`, `tools/slicetool.cpp`, `Kernel/opimgset.cpp` | Very high | P3 |
| Export | Export by "sets" (name gallery) | Export only the objects belonging to a named set. | `Kernel/ngsetop.cpp` (`OPTOKEN_EXPORT_SETS`) | Medium | P3 |
| Export | Export hints / persisted parameters | Remembers per-format options in the document. | `Kernel/exphint.cpp`, `Kernel/exagal.cpp` | Low | P2 |
| Export | Export colour palette | Dumps the palette to a file. | `Kernel/expcol.cpp` | Low | P3 |

### 1.14 Web and animation

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Web | Per-object web address (`Ctrl+Shift+W`) | `AttrWebAddress`: URL + target frame attached to any object. | `Kernel/webattr.cpp`, `Kernel/webaddr.cpp`, `Kernel/hlinkdlg.cpp`, `Kernel/urldlg.cpp` | Medium | P3 |
| Web | Rollover states (4 states) | Default / Mouse over / Clicked / Selected, as named layers. | `Kernel/slice.cpp` (`RolloverState`), `Kernel/opbarcreation.cpp` | High | P3 |
| Web | Button bar creation | Generates N copies of the button across state layers, named `button1..N` and mutated per state. | `Kernel/opbarcreation.cpp`, `Kernel/barcreationdlg.cpp`, `Kernel/opdupbar.cpp`, `Kernel/bldbrdef.cpp` | High | P3 |
| Web | Imagemap (client-side) | Generates a `<map>` with polygonal/rectangular areas. | `Kernel/imagemap.cpp` | Medium | P3 |
| Web | Browser preview | `OPTOKEN_FRAME_BROWSERPREVIEW`. | `Kernel/frameops.cpp`, `wxOil/camnet.cpp` | Low | P3 |
| Web | Web preferences | Default web export options. | `Kernel/webprefs.cpp`, `Kernel/webflags.cpp`, `Kernel/webop.cpp`, `Kernel/webparam.cpp` | Low | P3 |
| Animation | Frame model on top of layers | Each layer = one frame; frame properties (delay, disposal, overlay/solid). | `Kernel/frameops.cpp`, `Kernel/animparams.cpp`, `Kernel/sgframe.cpp`, resources `IDB_FGAL_OVERLAY/SOLID` | High | P3 |
| Animation | New / copy / delete / move frame | Frame gallery operations. | `Kernel/frameops.cpp` (`OPTOKEN_FRAME_NEWFRAME`, `COPYFRAME`, `DELETEFRAME`) | Medium | P3 |
| Animation | Animation properties (loop, global delay) | `OPTOKEN_FRAME_ANIPROPERTIES`, `OPTOKEN_GIFANIMPROPERTYTABS`. | `Kernel/animparams.cpp`, `Kernel/frameops.cpp` | Medium | P3 |
| Animation | Grab frame / grab all frames | Captures rasterised frames. | `Kernel/frameops.cpp`, `Kernel/capturemanager.cpp` | Medium | P3 |
| Animation | Preview player | Play/stop/previous/next/start/end. | `Kernel/prevwdlg.cpp`, resources `IDC_PREVIEW_*` | Medium | P3 |
| Animation | Animation palette optimisation | Common palette and dithering across frames. | `wxOil/gpalopt.cpp`, `wxOil/palman.cpp` | High | P3 |

### 1.15 Printing and prepress

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Printing | Printing engine | Rendering to a printer context, scaling, page tiling. | `Kernel/printing.cpp`, `Kernel/printctl.cpp`, `Kernel/prntview.cpp`, `wxOil/grndprnt.cpp`, `wxOil/prncamvw.cpp` | High | P2 |
| Printing | Print and setup dialog | Printer, copies, range, scaling. | `Kernel/printctl.cpp`, `Kernel/prdlgctl`/`wxOil/prdlgctl.cpp`, `Kernel/optsprin.cpp` | Medium | P2 |
| Printing | Print options (layout/general) | Tabbed print options. | `Kernel/optsprin.cpp`, `Kernel/prnprefs.cpp`, resources `IDD_OPTSTAB_PRINTGENERAL/LAYOUT` | Medium | P2 |
| Printing | Printer's marks | Crop marks, registration marks, colour bar, greyscale bar, information. | `Kernel/prnmks.cpp`, `Kernel/prnmkcom.cpp`, resources `IDB_PRINTMARK_*` | Medium | P3 |
| Printing | Colour separation (CMYK + spot) | Per-plate preview and separated output. | `Kernel/colplate.cpp`, `Kernel/xsepsops.cpp`, `Kernel/psrndrgn.cpp` | High | P3 |
| Printing | On-screen plate preview | Composite / Cyan / Magenta / Yellow / Key / Spot 1-8 / Mono. | `Kernel/xsepsops.cpp` (`OPTOKEN_COMPOSITEPREVIEW`, `OPTOKEN_CYANPREVIEW`, …) | Medium | P3 |
| Printing | Line and fill overprint | `AttrOverprintLine`, `AttrOverprintFill`, `AttrPrintOnAllPlates`. | `Kernel/isetattr.cpp`, `Kernel/xsepsops.cpp` | Low | P3 |
| Printing | Print as shapes (text→curves) | `OPTOKEN_TOGGLEPRINTASSHAPES`. | `Kernel/printing.cpp` | Low | P3 |
| Printing | Print progress | Dedicated progress bar. | `wxOil/printprg.cpp`, `wxOil/progbar.cpp` | Low | P3 |
| Printing | PostScript generation | Bespoke PostScript DC with a prologue and screening functions. | `wxOil/psdc.cpp`, `Kernel/psrndrgn.cpp`, `wxOil/xrc/prolog.ps`, `spotfunc.ps` | High | P3 |

### 1.16 Preferences and units

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Preferences | Declarative preferences system | `DeclareSection` / `DeclarePref` with types and ranges; persisted in the profile. | `Kernel/prefs.cpp`, `Kernel/appprefs.cpp`, `wxOil/oilprefs.cpp`, `wxOil/camprofile.cpp`, `wxOil/registry.cpp` | Medium | P0 |
| Preferences | Tabbed options dialog (`Ctrl+Shift+O`) | 13 tabs: General/Edit, Grid & Ruler, Internet, Misc, Page, Plug-ins, Pointers, Print General, Print Layout, Scale, Tune, Undo, Units, View. | `Kernel/optsedit.cpp`, `optsgrid.cpp`, `optsinet.cpp`, `optsmisc.cpp`, `optspage.cpp`, `optsplug.cpp`, `optspntr.cpp`, `optsprin.cpp`, `optsscal.cpp`, `optstune.cpp`, `optsundo.cpp`, `optsunit.cpp`, `optsview.cpp` | High | P1 |
| Preferences | Edit tab: duplicate offset, constrain angle, nudge | Editing parameters. | `Kernel/optsedit.cpp` | Low | P1 |
| Preferences | View tab: default quality, scrolling, double buffering | Display options. | `Kernel/optsview.cpp` | Low | P1 |
| Preferences | Undo tab: undo buffer size | Memory limit on the history. | `Kernel/optsundo.cpp`, `Kernel/ophist.cpp` | Low | P1 |
| Preferences | Tune tab: memory, cache, performance | Bitmap cache and memory settings. | `Kernel/optstune.cpp`, `wxOil/tunemem.cpp`, `wxOil/cammemory.cpp` | Medium | P2 |
| Preferences | Pointers tab: cursors | Pointer style selection. | `Kernel/optspntr.cpp`, `wxOil/cursor.cpp` | Low | P3 |
| Preferences | Scale tab: drawing scale | Cartographic/architectural scale (e.g. 1:100). | `Kernel/optsscal.cpp`, `Kernel/scunit.cpp` | Medium | P2 |
| Units | 12 unit types | Millimetres, centimetres, metres, inches, feet, yards, points, picas, millipoints, miles, kilometres, pixels + "automatic". Internal base: **millipoints**. | `Kernel/units.cpp`, `Kernel/unittype.h`, `Kernel/unitres.h`, `Kernel/scunit.cpp` | Medium | P0 |
| Units | User-defined units | Create derived units (numerator/denominator over a base unit). | `Kernel/units.cpp`, `Kernel/optsunit.cpp` (`OPTOKEN_UNITPROPERTIESDLG`) | Medium | P3 |
| Units | Page units vs font units | Two independent sets (document and typography). | `Kernel/units.cpp`, `Kernel/unitcomp.cpp` | Low | P1 |
| Units | Parsing/formatting of measurements in fields | Accept "10mm", "1 in", "3p6" in any numeric field. | `Kernel/units.cpp`, `Kernel/usercord.cpp`, `Kernel/userrect.cpp` | Medium | P0 |
| Preferences | Startup tips dialog | "Tip of the day". | `Kernel/tipsdlg.cpp` | Low | P3 |
| Preferences | Configurable hotkeys | Shortcut table loaded from a resource, with modifiers and context. | `Kernel/hotkeys.cpp`, `wxOil/keypress.cpp`, `wxOil/wxkeymap.cpp`, `wxOil/xrc/STANDARD_HOTKEYS.res` | Medium | P1 |

### 1.17 Undo / redo and the operations system

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Undo | Operations model (`Operation`/`OpDescriptor`) | Every action is an `Operation` registered with a token; this gives uniform menus, bars, macros and undo. **An architectural pillar.** | `Kernel/ops.cpp`, `Kernel/opdesc.cpp`, `Kernel/opnode.cpp`, `Kernel/undoop.cpp` | Very high | P0 |
| Undo | Undo/redo history | Stack of actions with a textual description, limited by memory. | `Kernel/ophist.cpp`, `Kernel/optsundo.cpp` | High | P0 |
| Undo | Granular actions (`Action`) | Each operation is composed of recorded, reversible actions. | `Kernel/undoop.cpp`, `Kernel/cpyact.cpp`, `Kernel/insertnd.cpp`, `Kernel/objchge.cpp` | High | P0 |
| Undo | Readable description in the menu | "Undo: Move", etc. | `Kernel/opdesc.cpp`, `Kernel/ophist.cpp` | Low | P0 |
| Undo | Non-undoable operations | View changes, preferences, zoom. | `Kernel/ops.cpp` | Low | P0 |
| Undo | Undo for plug-ins and effects | Undo wrapper for external operations. | `Kernel/plugopun.cpp`, `Kernel/bfxopun.cpp` | Medium | P3 |
| Undo | Internal messaging (`Msg`/`MessageHandler`) | Broadcast of selection, document, attribute and preference events. | `Kernel/camtypes.cpp`, `Kernel/msg.h`, `Kernel/optsmsgs.h`, `Kernel/objchge.cpp` | High | P0 |
| Undo | Object registry and bespoke RTTI | `CC_DECLARE_DYNCREATE`, `CCRuntimeClass`, memory dump. | `Kernel/objreg.cpp`, `Kernel/ccobject`/`wxOil/ccobject.cpp`, `Kernel/camtypes.cpp` | High | P0 |

### 1.18 Snapping, guides, grid and rulers

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Snap | Snap to grid (`NumPad .`) | Snapping to the active grid, toggleable during a drag. | `Kernel/snap.cpp`, `Kernel/snapops.cpp`, `Kernel/grid.cpp` | Medium | P0 |
| Snap | Snap to guides (`NumPad 2`) | Snapping to guidelines. | `Kernel/snap.cpp`, `Kernel/guides.cpp` | Low | P0 |
| Snap | Snap to objects / "magnetic snap" (`NumPad *`) | Magnetic snapping to other objects' points and edges, with configurable radii. **A Xara differentiator.** | `Kernel/snap.cpp`, `Kernel/snapops.cpp`, `Kernel/optsgrid.cpp`, cursor `IDC_SNAPPED.cur` | High | P1 |
| Grid | Rectangular and isometric grids | Type, spacing, subdivisions, colour, per spread. | `Kernel/grid.cpp`, `Kernel/optsgrid.cpp`, `tools/gridtool.cpp` | Medium | P1 |
| Grid | Grid tool (`TOOL10`) | Create/move/resize grids in the document. | `tools/gridtool.cpp` (`OPTOKEN_GRIDRESIZE`, `OPTOKEN_GRIDNEWRESIZE`, `OPTOKEN_GRIDSELECTION`) | Medium | P3 |
| Grid | Show/hide grid (`#`) | Visibility toggle. | `Kernel/viewmenu.cpp` (`OPTOKEN_SHOWGRID`) | Low | P0 |
| Guides | Create a guide by dragging from the ruler | Horizontal and vertical guides; dedicated cursors. | `Kernel/guides.cpp`, `wxOil/oilruler.cpp`, cursors `IDCSR_SEL_HGUIDE/VGUIDE.cur` | Medium | P1 |
| Guides | Guide properties / delete guide / delete all | Dialog for the exact position. | `Kernel/guides.cpp` (`OPTOKEN_EDITGUIDELINEPROPDLG`, `OPTOKEN_DELETEGUIDELINE`, `OPTOKEN_DELETEALLGUIDELINES`) | Low | P1 |
| Guides | Show/hide guides (`NumPad 1`) | Toggle. | `Kernel/viewmenu.cpp` (`OPTOKEN_SHOWGUIDES`) | Low | P1 |
| Rulers | Rulers with units and a position marker | `Ctrl+L`; movable origin; pointer marks. | `Kernel/rulers.cpp`, `wxOil/oilruler.cpp` | Medium | P1 |
| Snap | Pull onto grid / arrange pull grid | Snap existing objects onto the grid. | `Kernel/oppull.cpp` (`OPTOKEN_PULLONTOGRID`, `OPTOKEN_ARRANGEPULLGRID`) | Low | P3 |

### 1.19 View, rendering and quality

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Render | `RenderRegion` render engine | Abstraction over the drawing target (screen, printer, bitmap, PostScript, SWF) with an attribute stack. | `Kernel/rndrgn.cpp`, `Kernel/rndstack.cpp`, `Kernel/noderend.cpp`, `Kernel/rrcaps.cpp`, `Kernel/region.cpp`, `Kernel/rgnlist.cpp` | Very high | P0 |
| Render | CDraw/GDraw rasteriser | Proprietary engine for filling, antialiasing and transparency (binary in `libs/`). **This is the key to Xara's quality and speed.** | `GDraw/gdraw.h`, `GDraw/gdraw2.h`, `Kernel/GDrawIntf.cpp`, `wxOil/gdrawcon.cpp`, `wxOil/grndrgn.cpp`, `libs/` | Very high | P0 (to be replaced) |
| Render | High-quality antialiasing | Antialiasing on by default on screen, selectable. | `Kernel/quality.cpp`, `wxOil/grndrgn.cpp` | High | P0 |
| Render | Quality levels: Antialiased / Normal / Simple / Outline | Progressive degradation for speed. | `Kernel/quality.cpp`, `Kernel/qualops.cpp`, `wxOil/xrc/SHARED_MENU.res` | Medium | P1 |
| Render | Incremental redraw by region | Only the invalidated regions are redrawn. | `Kernel/invalid.cpp`, `Kernel/region.cpp`, `wxOil/rendwnd.cpp`, `wxOil/rendbits.cpp` | High | P0 |
| Render | Offscreen rendering and double buffering | Offscreen buffer to avoid flicker. | `wxOil/offscrn.cpp`, `wxOil/osrndrgn.cpp`, `wxOil/grnddib.cpp` | Medium | P0 |
| Render | XOR / EOR rendering for blobs and drags | Drawing handles and previews without redrawing the document. **Key to the sense of speed.** | `Kernel/blobs.cpp`, `wxOil/grndrgn.cpp`, `wxOil/handles.cpp`, `tools/rendsel.cpp` | High | P0 |
| View | Zoom: tool (`TOOL4`), in/out, to page, to drawing, to selection, 100 % | `Ctrl+NumPad+/-`, `Ctrl+Shift+P/J/Z`, `Ctrl+R` (previous zoom). | `tools/zoomtool.cpp`, `tools/zoomops.cpp` | Medium | P0 |
| View | Push tool / pan (`TOOL3`, `Alt+9`, space bar) | Panning the canvas. | `tools/pushtool.cpp`, `tools/pushbase.cpp` | Low | P0 |
| View | Scrolling and scrollbars | Wheel scrolling, scrollbars, scrolling while dragging. | `wxOil/scroller.cpp`, `wxOil/scrlbutn.cpp`, `wxOil/scrlthmb.cpp`, `wxOil/scrvw.cpp` | Medium | P0 |
| View | Multiple views of the same document | `WindowNewView`, tile, cascade, arrange. | `Kernel/view.cpp`, `Kernel/docview.cpp`, `Kernel/scrnview.cpp`, `wxOil/camview.cpp`, `wxOil/scrcamvw.cpp` | High | P2 |
| View | Full screen (`NumPad 8`) | Undecorated mode. | `Kernel/viewmenu.cpp` (`OPTOKEN_VIEWFULLSCREEN`) | Low | P2 |
| View | Sliding quality bar | Quality slider in the status bar. | `Kernel/qualops.cpp` (`OPTOKEN_QUALITYSLIDER`), resources `IDB_QUALITY*` | Low | P3 |
| View | Redraw timing measurement | `TimeDraw` (a development tool). | `wxOil/speedtst.cpp`, `Kernel/viewmenu.cpp` | Low | P3 |

### 1.20 Infrastructure, platform and utilities

| Area | Feature | Description | Reference C++ file(s) | Complexity | Priority |
|---|---|---|---|---|---|
| Infra | OIL layer (OS/toolkit abstraction) | Clean Kernel ↔ OIL separation; it is what made the port from MFC to wxWidgets possible. A model to replicate in Rust (a GUI-free core). | all of `wxOil/`, `Kernel/` with no toolkit dependencies | High | P0 |
| Infra | Fixed-point arithmetic | `FIXED16`, `FIXED24`, `XLONG` for exact coordinates in millipoints. | `Kernel/fixed.cpp`, `Kernel/fixed16.cpp`, `Kernel/fixed24.cpp`, `Kernel/fix24.cpp`, `Kernel/xlong.cpp` | Medium | P0 |
| Infra | Matrices and geometry | `Matrix`, `Trans2DMatrix`, `DocCoord`, `DocRect`, `WorkCoord`. | `Kernel/matrix.cpp`, `Kernel/xmatrix.cpp`, `Kernel/trans2d.cpp`, `Kernel/doccoord.cpp`, `Kernel/docrect.cpp`, `Kernel/wrkcoord.cpp`, `Kernel/coord.cpp`, `Kernel/vector.cpp` | Medium | P0 |
| Infra | Fixed-length string system | `String_8/16/32/64/128/256` — to be avoided in Rust (use `String`). | `wxOil/fixstr*.cpp`, `wxOil/basestr.cpp`, `wxOil/varstr.cpp` | Low | — |
| Infra | Error and exception handling | `ERROR2`, `ENSURE`, localised error boxes. | `wxOil/errors.cpp`, `wxOil/errorbox.cpp`, `wxOil/ensure.cpp`, `wxOil/exceptio.cpp` | Medium | P0 |
| Infra | Resource management and localisation | XRC + `.res` resources, `po/` catalogues (gettext), 152 XRC files. | `wxOil/camresource.cpp`, `wxOil/resources.cpp`, `wxOil/xrc/`, `po/` | Medium | P1 |
| Infra | Progress and long-running operations | Progress bar, cancellation, wait cursors. | `wxOil/progress.cpp`, `wxOil/progbar.cpp`, `wxOil/oilprog.cpp` | Low | P1 |
| Infra | File and path management | `PathName`, buffered files, compressed archives. | `wxOil/pathname.cpp`, `wxOil/pathnmex.cpp`, `wxOil/fileutil.cpp`, `Kernel/ccfile.cpp`, `Kernel/ccbuffil.cpp`, `Kernel/ccafile.cpp`, `Kernel/archive.cpp` | Medium | P0 |
| Infra | Native and internal clipboard | Internal format + exchange with the system. | `wxOil/natclipm.cpp`, `wxOil/clipext.cpp`, `wxOil/fuzzclip.cpp` | Medium | P1 |
| Infra | Graphics tablet support | Pressure and tablet input. | `wxOil/tablet.cpp`, `Kernel/pressure.cpp` | Medium | P3 |
| Infra | Network / HTTP downloads | Downloading libraries, help and updates. | `wxOil/camnet.cpp`, `wxOil/lddirect.cpp`, `wxOil/htmldwnd.cpp`, `wxOil/helpdownload.cpp` | Medium | P3 |
| Infra | Online help and "What's this?" | `F1`, `OPTOKEN_WHATSTHIS`. | `Kernel/opwhat.cpp`, `wxOil/helpuser.cpp`, `wxOil/helptabs.cpp` | Low | P3 |
| Infra | About / register / update dialog | `AboutDlg`, `Register`, `Update`. | `Kernel/aboutdlg.cpp`, `Kernel/release.cpp`, `Kernel/inetop.cpp` | Low | P3 |
| Infra | Debugging tools | Node tree, Xar tree, CMX tree, blobby dialogs, render tests, crash tests. | `Kernel/dbugtree.cpp`, `Kernel/debugdlg.cpp`, `Kernel/cxftree.cpp`, `Kernel/cmxtree.cpp`, `Kernel/blobby.cpp`, `Kernel/renddlg.cpp`, `Kernel/rnddlgs.cpp`, `wxOil/diagnost.cpp` | Medium | P2 |
| Infra | Modules and dynamic loading | Module system (`CreateModule`), tools as plug-ins. | `Kernel/module.cpp`, `Kernel/modlist.cpp`, `tools/camtools.cpp`, `tools/viewmod.cpp` | Medium | P3 |
| Infra | Standalone `xarlib` library | Reading/expanding Xar files outside the app. | `xarlib/`, `xarlib/ExpandXar` | Medium | P2 |

---

**Count**: 20 areas, **335 catalogued features** (rows of the master table, §1.1–§1.20).

---

## 2. MVP (P0) of the first Linux release in Rust

### 2.1 Goal of the MVP

> **A vector editor that opens and saves `.xar` and SVG [Superseded: architecture §3.5 makes writing .xar a non-goal], draws and edits shapes and paths with the *Xara feel* (unified selector, on-canvas fill editing, instant redraw and quality antialiasing), with layers, basic text and export to PNG/JPEG/SVG.**

The cut-off criterion is not "how many features", but **preserving Xara's interaction identity**. A small set of tools that feel exactly like Xara is preferable to a broad, clumsy clone.

### 2.2 What IS in the MVP (P0)

| Block | Contents |
|---|---|
| **Core** | Node tree + attributes as nodes; operations system with granular undo/redo; messaging; coordinates in millipoints (fixed point or `i64`). |
| **Document** | New/Open/Save/Save as; a single page with a configurable size; layers with visibility/lock and a layer gallery. |
| **Render** | Bespoke rasteriser in Rust with quality antialiasing (candidates: `tiny-skia`, `vello`, or a bespoke engine); incremental redraw by region; handle overlay decoupled from the document (the equivalent of XOR rendering). |
| **Tools** | Complete unified Selector (`TOOL7`) with the dual scale/rotation state; Rectangle; Ellipse; Bézier/Shape; Pen; Freehand; Fill; Transparency; Zoom; Push/Pan. **10 tools.** |
| **Path editing** | Nodes and handles; add/delete points; line/curve; smooth/cusp; close; select all points; winding rule. |
| **Transformation** | Move, scale, rotate, shear, flip, copy-and-transform, keyboard nudge (24 variants), numeric X/Y/W/H/angle infobar with 9 anchors and an aspect lock. |
| **Structure** | Group/ungroup; full Z order; alignment and distribution; cut/copy/paste/paste in place; duplicate; clone; delete; select all/none. |
| **Colour** | RGB + HSV + greyscale; named document colours; colour editor; on-screen colour bar with dragging onto objects; "no colour". |
| **Fill** | Flat, linear, radial/circular, with **on-canvas handles** and dragging palette colours onto the stops; multi-stop ramps. |
| **Transparency** | Flat and linear/radial, Mix mode, with on-canvas handles. |
| **Line** | Width, colour, transparency, caps, joins, mitre limit, winding rule. |
| **Text** | Simple and column text; typeface (FreeType/fontconfig), size, bold/italic/underline, justification, line spacing; conversion to shapes. |
| **Bitmaps** | Import PNG/JPEG and insert as a bitmap node; bitmap gallery; move/scale. |
| **Snapping** | Snap to grid and to guides; rectangular grid; rulers; draggable guides. |
| **View** | Zoom (tool + commands), pan, antialias/outline quality, scrolling. |
| **I/O** | **Import**: `.xar` (full reading), SVG, PNG, JPEG. **Export**: `.xar` [Superseded: architecture §3.5 makes writing .xar a non-goal], SVG, PNG, JPEG. |
| **Preferences** | Persisted prefs system + units (all 12 types, parsing "10mm"/"1in" in every field). |
| **UI** | Contextual per-tool infobar; menus and shortcuts compatible with the originals; context menus; status bar with coordinates. |

### 2.3 What is NOT in the MVP, and why

| Excluded | Justification |
|---|---|
| **Blend, Contour, Mould, Bevel** | Four subsystems of *Very high* complexity each (shape and attribute interpolation, envelope/perspective deformation with fills, robust path offsetting, 3D lighting). Each is worth a full phase on its own. None of them blocks day-to-day use of a vector editor. |
| **Shadow and Feather** | Marked **prominently P1**: they are among Xara's most visible differentiators and must land immediately after the MVP, but they need a Gaussian blur pipeline and offscreen rendering that should not be committed to before the base renderer has stabilised. |
| **Live Effects / XPE / bitmap plug-ins** | They depend on an ecosystem of external plug-ins (XPE, Photoshop-style plug-ins) that does not exist on Linux today. Low value, very high cost. |
| **Combine shapes (booleans)** | *Very high* complexity and a high risk of numerical-robustness bugs. P1: to be tackled with an already-proven boolean crate as soon as the path model is settled. |
| **Brushes, stroke types, pressure/tablet** | A large subsystem (`brsh*`, `strk*`, `pp*`) with little demand in a technical-illustration/diagram editor; P2. |
| **Fractal/noise, three- and four-colour, conical and square fills** | The long tail of fill types; 90 % of usage is flat/linear/radial/bitmap. P1–P3. |
| **Advanced transparency modes (Contrast, Hue, Luminosity…)** | They depend on compositing operations from the CDraw engine that have to be reimplemented. P2, after the compositing pipeline. |
| **Text on a path, tab stops, text ruler, fine kerning** | Advanced typography; the MVP covers the text people need in order to label diagrams. P1–P2. |
| **Animation, frames, animated GIF, rollovers, imagemaps, image slicing, HTML/SWF** | The entire "2005 web" block is obsolete. P3, and probably **never reimplemented**; at most, export to animated SVG. |
| **Printing, printer's marks, colour separation, PostScript** | On Linux the natural route is to export PDF and delegate to CUPS. P2 (PDF export) / P3 (prepress). |
| **CDR, CMX, WMF/EMF, Acorn Draw, EPS/AI** | Legacy importers, *Very high* complexity. P2 for EPS/AI (there are still live files), P3 for the rest. A pragmatic alternative: delegate to external libraries (`librevenge`/`libcdr`, Ghostscript). |
| **Clipart/fill galleries and web libraries** | They depended on Xara servers that no longer exist. P3. |
| **Bitmap tracer (auto-trace)** | *Very high*; external alternatives exist (potrace). P2 via integration. |
| **Multiple views of the same document, multi-page/spreads** | Useful but not critical for the first release; they add complexity to the view model. P1–P2. |
| **CMYK/spot colours and colour management** | Only relevant with prepress. P3. |

### 2.4 Risks of the MVP

1. **The rasteriser.** CDraw is binary and proprietary; its antialiasing quality and its speed are *the* differentiator. It is the only element of the MVP that can sink the perception of the product. It must be prototyped and measured (`vello` on GPU vs `tiny-skia` on CPU) **before** anything is built on top of it.
2. **The `.xar` format.** Without reliable reading of `.xar` there is no continuity with existing documents. The *record handlers* (`Kernel/rech*.cpp`, `Kernel/cxf*.cpp`) are the de facto specification; `xarlib/` offers a smaller entry point to start from.
3. **Attributes as nodes.** This is an unusual model (attributes are preceding siblings, not properties of the object). Reproducing it, or replacing it with an explicit property model, is an architectural decision that constrains `.xar` import/export.

---

## 3. Xara's differentiators — what must be preserved

These are the traits that made Xara feel different from (and better than) the Illustrator, CorelDRAW or Inkscape of the period. **Losing them is equivalent to not having reimplemented Xara at all.**

| # | Differentiator | What it consists of | Where it lives in the code | Preservation priority |
|---|---|---|---|---|
| 1 | **Unified selection tool** | A single tool selects, moves, scales, rotates, shears and edits fills. A second click toggles scale ⇄ rotation. There is no need to switch tools for frequent operations. | `tools/selector.cpp`, `tools/selinfo.cpp` | **Critical (P0)** |
| 2 | **Direct, interactive fill editing on the canvas** | Gradient arrows and blobs on the object itself; dragging a palette colour onto the tip of the arrow changes that stop; keyboard nudging of the fill. Nobody else was doing this in 1995. | `tools/filltool.cpp`, `Kernel/opgrad.cpp`, `Kernel/fillndge.cpp`, cursors `IDC_CANDROPONFILL*` | **Critical (P0)** |
| 3 | **Redraw speed** | Incremental redraw by region + per-node caching + XOR rendering of the handles: dragging is instant however complex the drawing. Xara boasted of redrawing an order of magnitude faster than the competition. | `Kernel/invalid.cpp`, `Kernel/region.cpp`, `Kernel/nodecach.cpp`, `wxOil/grndrgn.cpp`, `wxOil/rendwnd.cpp` | **Critical (P0)** |
| 4 | **Antialiasing quality** | The CDraw rasteriser produced visibly cleaner edges than its competitors, with antialiasing on by default on screen. | `GDraw/`, `libs/`, `Kernel/GDrawIntf.cpp`, `Kernel/quality.cpp` | **Critical (P0)** |
| 5 | **Live parametric shapes** | Rounded rectangles, ellipses and QuickShapes remain editable as parameters (radius, number of sides, stellation, curvature) indefinitely; they are converted to paths only when the user asks. | `Kernel/noderect.cpp`, `Kernel/nodeelip.cpp`, `Kernel/nodershp.cpp` | High (P0/P1) |
| 6 | **Real-time soft shadows** | Wall/floor/glow shadow with a draggable penumbra, recomputed live while dragging. In 1999 this was close to magic. | `tools/shadtool.cpp`, `Kernel/nodeshad.cpp`, `Kernel/bshadow.cpp` | High (P1) |
| 7 | **Advanced transparencies** | Graduated transparency with the same geometries as the fills, plus *blend mode* style modes (Stained Glass, Bleach, Contrast, Luminosity, Hue…). | `Kernel/fillattr.cpp`, `Kernel/fillval.h`, `GDraw/gdraw.h` | High (P1/P2) |
| 8 | **Quality vector blend** | Interpolation of shape, colour, attributes and position, with independent bias/gain profiles and blend along a path. | `Kernel/nodeblnd.cpp`, `Kernel/gblend.cpp`, `tools/blndtool.cpp` | Medium-high (P2) |
| 9 | **Moulds (envelope and perspective)** | Deformation of any object or group by a Bézier envelope or a perspective, with the fill deforming coherently. | `Kernel/nodemold.cpp`, `Kernel/moldenv.cpp`, `Kernel/moldpers.cpp`, `Kernel/gmould.cpp` | Medium-high (P2) |
| 10 | **Named colours and linked colours** | Editing a document colour updates every use of it; derived colours (tint/shade/hue) are recomputed in cascade. Coherent palette design with no effort. | `Kernel/doccolor.cpp`, `Kernel/colcomp.cpp`, `Kernel/coldlog.cpp` | High (P1) |
| 11 | **Feather (edge fade) on any object** | An attribute, not a destructive effect: it applies to shapes, groups and text, and stays editable. | `Kernel/opfeathr.cpp`, `Kernel/fthrattr.cpp` | High (P1) |
| 12 | **ClipView** | Live clipping of a group by the topmost shape, destroying nothing. | `Kernel/nodeclip.cpp`, `tools/opclip.cpp` | Medium-high (P1) |
| 13 | **Selection inside groups without ungrouping** | `Ctrl`+click enters to the leaf object; `Alt`+click selects the object beneath. Distinct cursors indicate the mode. | `tools/selector.cpp`, `Kernel/hittest.cpp` | High (P0) |
| 14 | **Magnetic snapping to objects** | Snapping to other objects' points and edges with cursor feedback, switchable on and off mid-drag from the numeric keypad. | `Kernel/snap.cpp`, `Kernel/snapops.cpp` | High (P1) |
| 15 | **Keyboard modifiers live during a drag** | `Ctrl` (constrain), `Shift` (adjust) and `Alt` (alternative) change the behaviour **while** dragging, not only at the start. This includes toggling snapping in the middle of a drag (`WorksInDrag`). | `Kernel/input.cpp`, `wxOil/clikmods.cpp`, `wxOil/keypress.cpp`, `STANDARD_HOTKEYS.res` | **Critical (P0)** |
| 16 | **Contextual infobar with numeric fields that accept units** | Any field accepts "10mm", "1in", "3p6"; the *bump* buttons allow fine increments without typing. | `Kernel/infobar.cpp`, `Kernel/units.cpp`, `tools/*info.cpp` | High (P0) |
| 17 | **Everything is a named operation** | Every action has an `OPTOKEN`, a readable description and granular undo, and can be hung off a menu, a bar, a shortcut or a context menu with no extra code. | `Kernel/ops.cpp`, `Kernel/opdesc.cpp` | **Critical (P0)** |
| 18 | **Size and start-up** | Xara was small and started instantly next to enormous suites. A compact, fast-starting Rust binary preserves that competitive advantage. | (a global property of the design) | High (P0) |

---

## 4. Keyboard shortcuts and interaction model

Extracted from `wxOil/xrc/STANDARD_HOTKEYS.res` (142 lines) and `wxOil/xrc/SHARED_MENU.res`.

### 4.1 Modifiers (Xara's internal nomenclature)

| Xara name | Key | Typical semantics |
|---|---|---|
| `Constrain` | `Ctrl` | Constrain (square/circle, angles in multiples, aspect ratio) and command prefix |
| `Adjust` | `Shift` | Adjust / add to the selection / command variant |
| `Alternative` | `Alt` | Alternative mode (select the object beneath, nudge in pixels, tool switch) |
| `Extended` | — | Marks an extended keyboard key |
| `WorksInDrag` | — | The shortcut stays active **during** a drag (snapping) |
| `CheckUnicode` | — | The shortcut depends on the keyboard layout |

### 4.2 File, edit and document

| Shortcut | Command | Shortcut | Command |
|---|---|---|---|
| `Ctrl+N` | New drawing | `Ctrl+Shift+N` | New animation |
| `Ctrl+O` | Open | `Ctrl+W` | Close |
| `Ctrl+S` | Save | `Ctrl+P` | Print |
| `Ctrl+Shift+I` | Import | `Ctrl+Shift+E` | Export |
| `Ctrl+Shift+Y` | Import from URL | `Ctrl+Shift+O` | Options |
| `Ctrl+Z` / `<` | Undo | `Ctrl+Y` / `>` | Redo |
| `Ctrl+X` / `Backspace` / `Shift+Del` | Cut | `Ctrl+C` / `Ctrl+Ins` | Copy |
| `Ctrl+V` / `Ins` / `Shift+Ins` | Paste | `Ctrl+Shift+V` | Paste in place |
| `Ctrl+Shift+A` | Paste attributes | `Del` | Delete |
| `Ctrl+A` | Select all | `Esc` | Deselect all |
| `Ctrl+D` | Duplicate | `Ctrl+K` | Clone |
| `Return` / `Enter` | Edit selection | `Ctrl+Tab` / `Ctrl+Shift+Tab` | Next / previous document |

### 4.3 Arrange

| Shortcut | Command |
|---|---|
| `Ctrl+F` | Bring to front |
| `Ctrl+Shift+F` | Move forwards |
| `Ctrl+Shift+B` | Move backwards |
| `Ctrl+B` | Send to back |
| `Ctrl+Shift+U` | Move one layer up |
| `Ctrl+Shift+D` | Move one layer down |
| `Ctrl+G` | Group |
| `Ctrl+U` | Ungroup |
| `Ctrl+Shift+L` | Alignment dialog |
| `Ctrl+1` / `Ctrl+2` / `Ctrl+3` / `Ctrl+4` | Combine: Add / Subtract / Intersect / Slice |
| `Ctrl+Shift+S` | Convert to shapes |
| `Ctrl+Shift+C` | Convert to bitmap |
| `Ctrl+Shift+R` | Reverse the text's path |

### 4.4 View, zoom, grid and snapping

| Shortcut | Command |
|---|---|
| `Ctrl+NumPad +` / `Ctrl+NumPad −` | Zoom in / out |
| `Ctrl+Shift+P` | Zoom to spread |
| `Ctrl+Shift+J` | Zoom to drawing |
| `Ctrl+Shift+Z` | Zoom to selection |
| `Ctrl+R` | Previous zoom |
| `Ctrl+L` | Show/hide rulers |
| `#` | Show/hide grid |
| `NumPad 1` | Show/hide guides |
| `NumPad .` | Snap to grid *(active during a drag)* |
| `NumPad 2` | Snap to guides *(active during a drag)* |
| `NumPad *` | Snap to objects *(active during a drag)* |
| `NumPad 8` | Full screen |

### 4.5 Nudge (24 combinations)

| Modifier | Step |
|---|---|
| *(none)* | 1 nudge unit |
| `Ctrl` | ×5 |
| `Shift` | ×10 |
| `Ctrl+Shift` | 1/5 of the step |
| `Alt` | 1 pixel |
| `Alt+Shift` | 10 pixels |

Each one in the 4 directions (`↑ ↓ ← →`). There are three parallel families of nudge operations: objects (`OPTOKEN_NUDGE*`), path points (`OPTOKEN_PATHNUDGE*`) and fills (`OPTOKEN_FILLNUDGE*`).

### 4.6 Tools

| Shortcut | Tool | ID |
|---|---|---|
| `F2` / `Alt+S` / `Space` | Selector | `TOOL7` |
| `F3` | Freehand | `TOOL6` |
| `F4` | Bézier / Shape | `TOOL11` |
| `F5` / `Alt+1` | Graduated fill | `TOOL13` |
| `F6` / `Alt+2` | Transparency | `TOOL17` |
| `F7` / `Alt+6` | Blend | `TOOL16` |
| `F8` | Text | `TOOL21` |
| `Shift+F2` | QuickShape | `TOOL18` |
| `Shift+F3` | Rectangle | `TOOL5` |
| `Shift+F4` | Ellipse | `TOOL12` |
| `Shift+F5` | Pen | `TOOL14` |
| `Shift+F6` / `Alt+7` | Mould | `TOOL19` |
| `Shift+F7` / `Alt+0` / `Alt+Z` | Zoom | `TOOL4` |
| `Shift+F8` / `Alt+9` / `Alt+X` | Push (pan) | `TOOL3` |
| `Ctrl+F2` / `Alt+3` | Shadow | `TOOL22` |
| `Ctrl+F3` / `Alt+4` | Bevel | `TOOL23` |
| `Ctrl+F5` | Live Effects | `TOOL26` |
| `Ctrl+F7` / `Alt+5` | Contour | `TOOL24` |
| `Ctrl+F8` / `Alt+8` | Slice | `TOOL25` |
| *(no shortcut)* | Grid | `TOOL10` |

> **`ToolSwitch`**: `Alt+X`, `Alt+Z`, `Alt+S` and the **space bar** are *momentary* tool switches — releasing the key returns to the previous tool. The space bar activates the Selector temporarily. This pattern is an essential part of Xara's fluency and must be reproduced.

### 4.7 Galleries

| Shortcut | Gallery | Shortcut | Gallery |
|---|---|---|---|
| `F9` | Colours | `Shift+F9` | Fonts |
| `F10` | Layers | `Shift+F10` | Clipart |
| `F11` | Bitmaps | `Shift+F11` | Fills |
| `F12` | Lines | `Shift+F12` | Frames |
| `Ctrl+Shift+F9` | Names | | |

### 4.8 Others

| Shortcut | Command |
|---|---|
| `Ctrl+E` | Eyedropper / colour picker |
| `Ctrl+I` | Image slice |
| `Ctrl+Shift+W` | Web address dialog |
| `F1` | Help index |

### 4.9 The principal interaction model

1. **Everything happens on the canvas.** Modal dialogs are the exception: fills, transparencies, shadows, bevels, contours and moulds are edited with handles on the object.
2. **The infobar is the precision panel.** Every tool publishes its bar carrying the same parameters as its handles, in numeric form and with units.
3. **Tool ↔ object.** Double-clicking an object activates the tool that created it (a rectangle opens the Rectangle tool, a piece of text the Text tool). `Return` = "edit selection".
4. **Live modifiers.** `Ctrl`/`Shift`/`Alt` change the behaviour during a drag; snapping is toggled without releasing the mouse.
5. **Momentary tool switching.** Space bar (selector), `Alt+X` (push), `Alt+Z` (zoom).
6. **Drag and drop as a universal verb.** Palette colours onto objects and onto gradient stops; gallery items onto the canvas; files onto the canvas.
7. **Everything is undoable and has a name.** The Undo menu describes the last action by its real name.

### 4.10 Selector transforms and shape creation: observed behaviour

Facts read from the original as behaviour (added 2026-09-23 for XARA-US-0032/0033), never as code:

- **Fixed point of a transform drag.** Scale and shear drags fix the blob *opposite* the one grabbed (blobs are numbered 1–8 round the box, the opposite of `n` is `9 − n`: `tools/selector.cpp:3884`, `:3909`, `:3932`); a rotation turns about the rotation centre (`tools/selector.cpp:3856`). Holding **Adjust** during the drag switches the fixed point to the centre of the selection's bounds (`Kernel/transop.cpp:985-991`, `:1055-1061`).
- **Rotate + Constrain.** The pointer's angle about the centre is constrained to the constrain angle before and during the drag, so the rotation is a whole number of steps (`tools/oprotate.cpp:250-300`); the default step is 45°.
- **Shear.** The top and bottom edges shear horizontally, the side edges vertically, by the pointer's displacement over its distance from the fixed point; Constrain snaps the pointer's angle about the fixed point (`tools/opshear.cpp:174-190`, `:285-310`, `:333-370`, `:395-416`).
- **Rectangle and ellipse tools create quick shapes in "bounds" mode** (`tools/oprshape.cpp:244-374`). The axes point from the box centre to the middle of the top edge (major) and of the right edge (minor); for a polygon they are lengthened by `1 / cos(π/n)` so its corners land on the box corners — `√2` for a rectangle — and an ellipse keeps them as they are (`tools/oprshape.cpp:526-560`).
- **Constrain while drawing** puts the pointer on the nearest of the four diagonals from the start point, at its true distance (so the box is a square) (`tools/oprshape.cpp:253-283`).
- **Adjust while drawing** re-centres: when Adjust goes down the centre becomes the midpoint of the start and the pointer, and the box is drawn symmetrically about it; when it comes up the start becomes the reflection of the pointer through that centre (`tools/oprshape.cpp:336-365`). Adjust together with Constrain switches to a "radius" mode that rotates the shape with the pointer (`tools/oprshape.cpp:247-250`, `:293-312`).

---

## 5. Import and export formats with priorities

### 5.1 Import

| Format | Ext. | Vector/Bitmap | Original implementation | Priority | Recommended strategy in Rust |
|---|---|---|---|---|---|
| Xara native v2 | `.xar` | Vector | `Kernel/native.cpp`, `cxf*.cpp`, `rech*.cpp` | **P0** | Bespoke record parser + `flate2`; `xarlib/` as a minimal reference |
| SVG | `.svg`, `.svgz` | Vector | `filters/SVGFilter/svgimporter.cpp` | **P0** | `usvg`/`resvg` as a base, adapted to the node model |
| PNG | `.png` | Bitmap | `wxOil/pngfiltr.cpp` | **P0** | `image` / `png` crate |
| JPEG | `.jpg`, `.jpeg` | Bitmap | `Kernel/imjpeg.cpp` | **P0** | `image` / `jpeg-decoder` |
| Xara web | `.web` | Vector | `Kernel/webfiltr.cpp` | P2 | A variant of the `.xar` parser |
| GIF (incl. animated) | `.gif` | Bitmap | `wxOil/giffiltr.cpp` | P1 | `image` / `gif` crate |
| BMP / DIB | `.bmp`, `.dib` | Bitmap | `wxOil/imgmgkft.cpp`, `Kernel/bitfilt.cpp` | P1 | `image` |
| TIFF | `.tif`, `.tiff` | Bitmap | External ImageMagick | P2 | `image` (tiff feature) |
| WebP / AVIF | — | Bitmap | *(did not exist)* | P1 | `image` — **a recommended modern addition** |
| PDF | `.pdf` | Vector/Bitmap | ImageMagick (rasterised) | P2 | `pdfium`/`mupdf` binding, or rasterisation |
| PSD | `.psd` | Bitmap | External ImageMagick | P3 | Optional |
| PNM / PPM / PGM / PBM | `.ppm`… | Bitmap | `wxOil/ppmfiltr.cpp`, ImageMagick | P3 | `image` |
| XPM / ICO / PCD / PICT | various | Bitmap | External ImageMagick | P3 | `image` where available |
| Generic EPS | `.eps` | Vector | `Kernel/epsfiltr.cpp` | P2 | Delegate to Ghostscript, or a partial interpreter |
| Adobe Illustrator (AI/AI5/AI8) | `.ai` | Vector | `Kernel/ai_eps.cpp`, `ai5_eps.cpp`, `ai8_eps.cpp` | P2 | Modern AI = PDF ⇒ via the PDF route |
| EPS from Photoshop / FreeHand / ArtWorks | `.eps` | Vector | `Kernel/coreleps.cpp`, `freeeps.cpp`, `aw_eps.cpp` | P3 | Droppable |
| CorelDRAW | `.cdr` | Vector | `Kernel/cdrfiltr.cpp` + `cdr*.cpp` | P3 | `libcdr` via FFI if asked for |
| Corel CMX 16/32 | `.cmx` | Vector | `Kernel/cmx*.cpp` (9 files) | P3 | Droppable |
| WMF / EMF | `.wmf`, `.emf` | Vector | `wxOil/metafilt.cpp` | P3 | `libwmf` via FFI if asked for |
| Acorn Draw / Sprite | `.aff` | Vector/Bitmap | `Kernel/drawfltr.cpp` | P3 | Droppable |
| Palettes (MS, PSP, Corel, ACO/ACT, JCW) | various | Palette | `Kernel/impcol.cpp` | P3 | Modern `.ase`/`.gpl` instead |
| ANSI / Unicode / RTF text | `.txt`, `.rtf` | Text | `Kernel/textfltr.cpp` | P3 | Plain text only (P2) |
| Import from a URL | — | Any | `Kernel/urlimp.cpp` | P3 | Droppable (browser drag & drop is enough) |

### 5.2 Export

| Format | Ext. | Options in the original | Priority | Recommended strategy in Rust |
|---|---|---|---|---|
| Xara native v2 | `.xar` | Compression, embedded thumbnail, version | **P0** [Superseded: architecture §3.5 makes writing .xar a non-goal] | Record writer + `flate2` |
| SVG | `.svg` | Bespoke options dialog | **P0** | Bespoke serialiser (full control over fidelity) |
| PNG | `.png` | Depth, alpha, interlacing, palette, DPI, antialiasing, scope | **P0** | `png` crate |
| JPEG | `.jpg` | Quality, progressive, DPI | **P0** | `jpeg-encoder` |
| PDF | `.pdf` | *(did not exist natively)* | **P1** | **A priority modern addition**: it replaces EPS/PostScript and printing |
| GIF | `.gif` | Palette, dithering, transparency, interlacing | P1 | `gif` crate + quantisation |
| BMP / DIB | `.bmp` | Depth | P2 | `image` |
| TIFF | `.tif` | Via ImageMagick | P2 | `image` |
| WebP | `.webp` | *(did not exist)* | P2 | **A modern addition** |
| Xara web | `.web` | Reduced format | P3 | Droppable |
| EPS / PostScript | `.eps`, `.ps` | PS prologue, separation, marks | P3 | Replaced by PDF |
| Flash / SWF | `.swf` | Shapes, text, bitmaps, sprites, buttons | P3 | **Do not reimplement** (dead format) |
| HTML + imagemap | `.html` | `<map>` with areas, per-object URLs | P3 | **Do not reimplement** |
| Image slicing (HTML + pieces) | — | Cuts by named objects + rollovers | P3 | **Do not reimplement** |
| Animated GIF | `.gif` | Delays, looping, disposal, common palette | P3 | Consider animated SVG/APNG instead |
| Colour palette | various | Dump of the document's palette | P3 | `.gpl` / `.ase` |

### 5.3 Numeric summary of formats

- **Import**: **24 format families** catalogued (4 at P0, 3 at P1, 6 at P2, 11 at P3). Counting the concrete bitmap variants the original plugged in via ImageMagick, the nominal catalogue exceeds **50 formats** (`FILTERID_*` defines 130+ identifiers, most of them commented out in the Linux build).
- **Export**: **16 format families** (4 at P0, 2 at P1, 3 at P2, 7 at P3), plus 2 recommended modern additions (PDF, WebP).

---

## 6. Tool count

The original defines **26 tool slots** (`TOOLID_1..26` in `Kernel/tool.h`), of which **20 are implemented** in `tools/`:

| # | Tool | ID | File | Priority |
|---|---|---|---|---|
| 1 | Selector | `TOOL7` | `tools/selector.cpp` | P0 |
| 2 | Rectangle | `TOOL5` | `tools/rectangl.cpp` | P0 |
| 3 | Ellipse | `TOOL12` | `tools/eliptool.cpp` | P0 |
| 4 | QuickShape (polygon/star) | `TOOL18` | `tools/regshape.cpp` | P1 |
| 5 | Bézier / Shape | `TOOL11` | `tools/beztool.cpp` | P0 |
| 6 | Pen | `TOOL14` | `tools/pentool.cpp` | P0 |
| 7 | Freehand | `TOOL6` | `tools/freehand.cpp` | P0 |
| 8 | Text | `TOOL21` | `tools/texttool.cpp` | P0 |
| 9 | Graduated fill | `TOOL13` | `tools/filltool.cpp` (`GradFillTool`) | P0 |
| 10 | Transparency | `TOOL17` | `tools/filltool.cpp` (`TranspTool`) | P0 |
| 11 | Blend | `TOOL16` | `tools/blndtool.cpp` | P2 |
| 12 | Mould | `TOOL19` | `tools/moldtool.cpp` | P2 |
| 13 | Contour | `TOOL24` | `tools/cntrtool.cpp` | P2 |
| 14 | Shadow | `TOOL22` | `tools/shadtool.cpp` | P1 |
| 15 | Bevel | `TOOL23` | `tools/bevtool.cpp` | P2 |
| 16 | Live Effects | `TOOL26` | `tools/liveeffectstool.cpp` | P3 |
| 17 | Slice (web) | `TOOL25` | `tools/slicetool.cpp` | P3 |
| 18 | Zoom | `TOOL4` | `tools/zoomtool.cpp` | P0 |
| 19 | Push / Pan | `TOOL3` | `tools/pushtool.cpp` | P0 |
| 20 | Grid | `TOOL10` | `tools/gridtool.cpp` | P3 |
| — | Blank (development template) | `TOOL15` | `tools/blnktool.cpp` | — |
| — | Test / Rect / Rotate / Accusoft (historic slots, unimplemented in LX) | `TOOL2/8/9/20` | — | — |

Functions that in Xara are **not tools** but which other programs do expose as such, and which have to be placed somewhere in the reimplementation's UI:

- **Eyedropper** → the `Ctrl+E` command + dedicated cursors (`wxOil/dragpick.cpp`).
- **Eraser** → *rub-out* mode inside the freehand tool (`tools/freehand.cpp`).
- **Brush** → a line attribute (line gallery + `Kernel/brshattr.cpp`), not a separate tool.
- **Crop / clip** → the ClipView operation (`tools/opclip.cpp`), not a tool.
- **Dimensions / measurement** → does not exist in Xara LX; the rulers, guides and the numeric infobar cover the case.

---

## 7. Notes for planning

1. **Suggested order of attack**: (a) node model + operations + undo; (b) rendering and the handle overlay; (c) selector + shapes + paths; (d) fills/transparencies with on-canvas editing; (e) `.xar` reading, then writing [Superseded: architecture §3.5 makes writing .xar a non-goal]; (f) SVG; (g) text; (h) layers and galleries; (i) shadow and feather; (j) the rest by priority.
2. **The OIL layer is a good precedent**: keeping the *core* in pure Rust with no GUI dependencies buys headless tests, rendering to a file and, later on, other frontends.
3. **`Kernel/rech*.cpp` is the documentation of the `.xar` format**: each file is the *record handler* for one family of records (`rechrect`, `rechellp`, `rechtext`, `rechbmp`, `rechcol`, `rechattr`, `rechpoly`, `rechrshp`, `rechprnt`, `rechunit`, `rechdoc`, `rechinfo`, `rechsmth`, `rechcomp`). They are the starting point for writing the specification of the parser in Rust.
4. **Not to be reimplemented**: SWF, HTML/imagemap, image slicing, rollovers, animated GIF, CMX, CDR, Acorn Draw, PostScript/prepress, XPE plug-ins. They are ~15 % of the original code and ~0 % of its current value.
