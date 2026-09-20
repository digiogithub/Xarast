# Phase 7 — Tools & editing

> After this phase Xarast stops being a viewer: you can draw, select, transform, reshape, arrange and snap — with Xara's unified selector, its live modifiers and its 24 nudges — and every single change is undoable by construction.

## Goal

Implement the MVP tool set of `research/04 §2.2`, minus the fill and transparency tools
(Phase 8) and text (Phase 9):

**Unified selector** (with the scale ⇄ rotation/skew dual state), **rectangle**,
**ellipse**, **bézier/shape**, **pen**, **freehand**, **zoom**, **push/pan** — eight
tools — plus node and handle editing, the full transform set, structure operations, and
snapping to grid and guides.

Two structural things matter more than the tool count, because they decide whether the
rest of the product is buildable:

1. **The tool state machine.** One explicit machine, shared by every tool, that owns
   hover, drag thresholds, live modifiers, mid-drag snap toggling, momentary tool switch
   and cancellation. Xara's feel comes from these details (`research/04 §3` items 1, 13,
   15, 16), not from the tools themselves.
2. **Tools never mutate the arena** (architecture §4). A tool emits `Command`s on the bus;
   the bus builds a `Transaction` that computes its inverse *before* applying it. That is
   what makes undo complete by construction rather than by diligence, and it is the rule
   most likely to be quietly broken under deadline pressure. The API below makes breaking
   it impossible: a tool never receives `&mut Document`.

---

## Scope

### In scope

| # | Item | Reference |
|---|---|---|
| S1 | Tool state machine, tool registry, tool switching (including momentary switch) | `research/04 §4.6`, `§4.9` |
| S2 | Command bus: `Command` → `Transaction` (inverse-first), coalescing, labelled undo/redo | architecture §4, `research/02 §10.13` |
| S3 | Live drag preview without mutation (transient transform / transient geometry consumed by the scene walker) | architecture §4, §5 |
| S4 | On-canvas handle overlay ("blobs"): object, bounding-box, rotation-centre, node and control handles, drawn into the Phase 4 overlay surface | `research/04 §1.4`, `§1.19` |
| S5 | Hit testing against real geometry: fill, stroke, and group traversal, with `Ctrl`-click leaf selection and `Alt`-click select-under | `research/04 §1.4` |
| S6 | Unified selector: click, marquee (touch vs enclose), additive/toggle selection, scale ⇄ rotate/skew dual state, draggable rotation centre | `research/04 §1.4` |
| S7 | Transforms: move, scale (with/without aspect, with/without line-width scaling), rotate, skew, flip H/V, copy-and-transform, and correct propagation into groups and fills | `research/04 §1.4` |
| S8 | The 24 keyboard nudge variants × three families (objects, path points, fills — fills wired but inert until Phase 8) | `research/04 §4.5` |
| S9 | Numeric infobar: X/Y/W/H/angle/skew with a 9-anchor grid, aspect padlock, bump buttons, unit-parsing fields | `research/04 §1.4`, `§3` item 16 |
| S10 | Rectangle and ellipse as **live parametric shapes** (not converted to paths), with interactive corner-radius handles, `Ctrl` constraint and create-from-centre | `research/04 §1.2` |
| S11 | Bézier/shape tool: select/move nodes and control handles, add/delete points, line⇄curve, smooth⇄cusp, close path, select/deselect all points | `research/04 §1.3` |
| S12 | Pen tool: point-by-point lines and curves with live preview | `research/04 §1.3` |
| S13 | Freehand tool: curve fitting and configurable smoothing, plus rub-out while drawing | `research/04 §1.3` |
| S14 | Zoom tool and zoom commands; push/pan tool and space-bar panning | `research/04 §1.19` |
| S15 | Structure: group/ungroup (nested), full Z-order (front/back/forward/backward/layer up/down), align and distribute (9 anchors, both axes), cut/copy/paste/paste-in-place, duplicate, clone, delete, select all/none | `research/04 §1.4` |
| S16 | Clipboard: internal format plus system interchange (SVG and PNG flavours) | `research/04 §1.20` |
| S17 | Snapping to grid and to guides, toggled mid-drag from the numeric keypad (`WorksInDrag`) | `research/04 §1.18` |
| S18 | Winding-rule attribute (nonzero/evenodd) on paths | `research/04 §1.3` |
| S19 | Convert to shapes (`Ctrl+Shift+S`) for rectangle/ellipse | `research/04 §1.2` |
| S20 | Per-tool "current attributes" for newly created objects | `research/04 §1.5` |
| S21 | Context menus and the tool infobar host | `research/04 §4.9` |

### Explicitly out of scope (and which phase owns it)

| Item | Owner |
|---|---|
| Fill and transparency tools, on-canvas gradient handles, colour drag-and-drop onto stops | Phase 8 (the nudge family and the infobar host are wired here so Phase 8 only fills them in) |
| Text tool and text editing | Phase 9 |
| QuickShape tool (polygons/stars, stellation, curvature) | Later — P1 in `research/04 §1.2`, not MVP |
| Boolean shape combination (`Ctrl+1..4`) | Phase 8 or later — P1, and it depends on `xarast-geom` boolean robustness |
| Join/break shapes, reverse path, smooth selection, retrofit, inset path | Later — P1/P2 |
| Magnetic snapping to objects (`NumPad *`) | Later — P1; the `SnapSource` trait is defined here so adding it is additive |
| Blend, mould, contour, shadow, bevel, live-effect and slice tools | Phase 13 |
| Bitmap/photo tools and the grid tool | Phase 10 / later |
| Multi-page and spread management | Later |
| Pressure-driven variable-width strokes | Later — the `StrokeSample` pressure field is already delivered by Phase 5 and is recorded but unused |

---

## Prerequisites

| Need | Source | Hard or soft |
|---|---|---|
| `xarast-doc` arena, attribute model, `Action`/`Transaction`/`History` | Phase 2 | Hard |
| `xarast-geom`: hit testing primitives, flattening, curve fitting, path editing, stroke→path | Phase 1 | Hard |
| `xarast-render` overlay surface and scene walker contract | Phase 4 | Hard |
| Shell input: live modifiers, per-frame coalesced samples, shortcut dispatcher, clipboard | Phase 5 | Hard |
| `xarast-ui` panel host, docking, status bar, canvas widget | Phase 5 | Hard |
| Grid and guide **display** | Phase 5 | Hard (this phase adds snapping to them) |

---

## Workstreams

### W1 — The command bus and the editing substrate

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T1.1 | `Command` enum and the `CommandBus`; every command produces its inverse before applying | `xarast-app` | L | — |
| T1.2 | `Transaction` construction with the builder pattern (drop without commit reverts) | `xarast-doc` | M | T1.1 |
| T1.3 | Coalescing policy: a drag collapses into one undo step; typed rules per command kind | `xarast-app` | M | T1.2 |
| T1.4 | Labelled undo/redo surfaced in the menu ("Undo Move", "Undo Apply Fill") | `xarast-app`, `xarast-ui` | S | T1.3 |
| T1.5 | `EditState` ownership: selection, point selection, active layer, insertion point — outside the arena and outside undo | `xarast-app` | M | T1.1 |
| T1.6 | Selection invalidation rules when nodes are deleted, detached or re-parented | `xarast-app` | M | T1.5 |
| T1.7 | `Preview`: transient transform / transient geometry applied at scene-build time, never committed | `xarast-app` | L | T1.1 |
| T1.8 | Permission checks (`ChangeKind`) before any mutation, so locked layers and locked objects refuse cleanly | `xarast-doc` | M | T1.1 |

**Tricky parts.**

*The preview mechanism is what makes "tools never mutate" affordable.* Dragging 5,000
objects cannot commit a transaction per mouse move, and it cannot mutate the arena
"temporarily and put it back". Instead the tool publishes a `Preview` — a set of node ids
plus a transform, or a replacement path for one node — which the scene walker applies
while building the display list. On mouse-up the tool emits one real command; on `Esc` it
drops the preview and nothing ever happened. This also gives cancellation for free, which
is otherwise a notorious source of half-applied edits.

*Coalescing is per command kind, not global.* Fifty `TransformNodes` commands from one
drag collapse to one; a `Group` followed by a `Move` must not. The rule lives with the
command definition, following the model of `Operation::PerformMergeProcessing`
(`research/02 §10.13`).

*Undo history is budgeted in bytes, not steps* (architecture §3.1, `research/02 §10.13`).
`Action::size_hint` must be implemented for every action added here, or the budget
silently stops working.

---

### W2 — Tool state machine and tool infrastructure

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T2.1 | `Tool` trait and `ToolCtx` (read-only document, mutable preview and command sink) | `xarast-app` | M | W1 |
| T2.2 | The shared interaction machine: `Idle → Hover → ArmedDrag → Dragging → Committing`, plus `Cancelled` | `xarast-app` | L | T2.1 |
| T2.3 | Drag threshold, double-click detection, auto-scroll at the viewport edge during a drag | `xarast-app` | M | T2.2 |
| T2.4 | Live modifier delivery: modifier changes reach the active gesture even with no pointer movement | `xarast-app` | M | T2.2 |
| T2.5 | Tool registry, activation, deactivation, and momentary switch (space/`Alt+X`/`Alt+Z`/`Alt+S`, restoring on release) | `xarast-app` | M | T2.1 |
| T2.6 | "Double-click opens the creating tool"; `Return` = edit selection | `xarast-app` | S | T2.5 |
| T2.7 | Per-tool current attributes for new objects | `xarast-app` | M | T2.1 |
| T2.8 | Infobar host: each tool publishes a numeric bar; fields parse units (`10mm`, `1in`, `3p6`) and have bump buttons | `xarast-ui` | L | T2.1 |
| T2.9 | Cursor management per tool and per state (including the leaf/under/snapped cursors) | `xarast-ui` | M | T2.2 |
| T2.10 | Context menu per tool and per hit target | `xarast-ui` | M | T2.5 |

**The state machine, precisely.**

```
                 pointer enters canvas
        ┌───────────────────────────────────┐
        ▼                                   │
   ┌────────┐  move    ┌───────┐  press  ┌──────────┐ move > threshold ┌──────────┐
   │  Idle  │─────────▶│ Hover │────────▶│ArmedDrag │─────────────────▶│ Dragging │
   └────────┘          └───────┘         └──────────┘                  └──────────┘
        ▲                   │                 │ release (no move)           │
        │                   │                 ▼                             │
        │                   │            ┌─────────┐                        │
        │                   └───────────▶│  Click  │                        │
        │                                └─────────┘                        │
        │                                     │                             │
        │             Esc / focus loss        ▼            release          ▼
        │        ┌──────────────────────────────────┐              ┌───────────────┐
        └────────│            Cancelled             │◀─────────────│  Committing   │
                 └──────────────────────────────────┘  Esc during  └───────────────┘
                   drops the preview, emits nothing      commit             │
                                                                    emits 1 Command
```

Rules that hold for every tool, enforced by the machine rather than by each tool:

1. **No command is emitted before `Committing`.** Everything visible during `Dragging` is
   a `Preview`.
2. **`Esc` at any point returns to `Idle` and drops the preview.** Nothing partially
   applied, nothing to undo.
3. **Modifiers are sampled continuously** (`research/04 §3` item 15). `Ctrl` (Constrain),
   `Shift` (Adjust) and `Alt` (Alternative) change the in-flight gesture; the machine
   re-evaluates the gesture on every modifier change, with the last pointer position.
4. **Snap is toggled mid-drag** by `NumPad .` (grid) and `NumPad 2` (guides); the gesture
   recomputes immediately. This is the `WorksInDrag` flag of the original.
5. **Momentary tool switch** suspends the active tool's gesture rather than cancelling it
   where that makes sense (space-bar pan during a drag resumes the drag afterwards); where
   it does not, the gesture is cancelled explicitly and the tool says so.
6. **Auto-scroll during drag** moves the viewport, and the gesture keeps working in
   document coordinates, not device ones.

---

### W3 — Selection, hit testing and handles

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T3.1 | Hit testing against real geometry (fill area, stroke band, image alpha), respecting z-order and layer visibility/lock | `xarast-app`, `xarast-geom` | L | — |
| T3.2 | Group traversal: default selects the outermost group; `Ctrl`-click selects the leaf; `Alt`-click selects the object beneath | `xarast-app` | M | T3.1 |
| T3.3 | Marquee selection with touch vs enclose semantics | `xarast-app` | M | T3.1 |
| T3.4 | Additive (`Shift`) and toggle selection; select all (`Ctrl+A`), select none (`Esc`) | `xarast-app` | S | T3.1 |
| T3.5 | Selection bounds cache with correct invalidation | `xarast-app` | M | W1 |
| T3.6 | Handle overlay renderer: object blobs, bounding-box handles, rotation-centre, node and control handles; independent visibility toggles | `xarast-ui` | L | Phase 4 overlay |
| T3.7 | Handle hit testing in device space with a fixed pixel tolerance regardless of zoom | `xarast-app` | M | T3.6 |

**Tricky parts.**

*Handles are the replacement for Xara's XOR blob rendering* (`research/04 §1.19`, `§3`
item 3). The point of XOR was to draw and erase handles without redrawing the document.
Our equivalent is the Phase 4 overlay surface: a separate, never-cached display list
composited over the cached document. The invariant is that moving a handle must not dirty
a single document tile.

*Handle tolerance is in device pixels, not document units.* A 6 px grab radius at 100 %
zoom must still be 6 px at 3000 %. Convert the pointer to document space for geometry, but
do handle picking in device space.

---

### W4 — The unified selector and transforms

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T4.1 | Selector tool skeleton: click, marquee, move drag | `xarast-app` | M | W2, W3 |
| T4.2 | Dual state: second click on the selection toggles scale ⇄ rotate/skew handles; draggable rotation centre with its own snap | `xarast-app` | L | T4.1 |
| T4.3 | Scale gesture: corner and edge handles, aspect constraint with `Ctrl`, scale-from-centre, optional line-width scaling | `xarast-app` | L | T4.2 |
| T4.4 | Rotate gesture: about the rotation centre, `Ctrl` constrains to angle increments | `xarast-app` | M | T4.2 |
| T4.5 | Skew gesture on the edge handles | `xarast-app` | M | T4.2 |
| T4.6 | Flip horizontal/vertical | `xarast-app` | S | T4.1 |
| T4.7 | Copy-and-transform: the drag leaves a copy behind | `xarast-app` | M | T4.1 |
| T4.8 | Transform propagation into groups, and into fill/transparency geometry, so a rotated gradient rotates with its object | `xarast-app`, `xarast-doc` | L | T4.3 |
| T4.9 | Numeric infobar: X/Y/W/H/angle/skew, 9-anchor grid, aspect padlock, line-scale toggle, bump buttons | `xarast-ui` | L | T2.8, T4.3 |
| T4.10 | The 24 nudge variants for objects, plus the parallel path-point and fill families | `xarast-app` | M | W1 |

**Tricky parts.**

*The dual state is the single most recognisable Xara behaviour* (`research/04 §3` item 1).
Its rules: clicking an unselected object selects it and shows **scale** handles; clicking
again on an already-selected object switches to **rotate/skew** handles and reveals the
rotation centre; clicking a third time returns to scale. Selecting a different object
resets to scale. The rotation centre is draggable, snaps like any other point, and its
position persists for that selection until the selection changes.

*The 9-anchor grid is not cosmetic.* It defines which point of the bounding box stays
fixed when W, H, X or Y are typed. With the padlock engaged, editing W updates H (and vice
versa) about the anchor. Getting this wrong is immediately visible to anyone who used
Xara, and it is cheap to test exhaustively: 9 anchors × {W, H, X, Y} × {locked, unlocked}
is 72 cases and they all belong in a table-driven test.

*The 24 nudges* (`research/04 §4.5`) are four directions × six step sizes: plain (one nudge
unit), `Ctrl` ×5, `Shift` ×10, `Ctrl+Shift` one fifth, `Alt` one pixel, `Alt+Shift` ten
pixels. "One pixel" means one *device* pixel at the current zoom, so it is a function of
the viewport; the others are document units. Three parallel families exist — objects, path
points and fills — dispatching on the active tool and on what is selected.

---

### W5 — Shape tools

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T5.1 | Rectangle tool: drag to create, `Ctrl` for a square, create-from-centre, live parametric node | `xarast-app` | M | W2 |
| T5.2 | Interactive corner-radius handle on the rectangle | `xarast-app` | M | T5.1 |
| T5.3 | Ellipse tool: drag to create, `Ctrl` for a circle, create-from-centre, live parametric node | `xarast-app` | M | W2 |
| T5.4 | Infobars for both, with numeric width/height/radius | `xarast-ui` | M | T2.8 |
| T5.5 | Convert to shapes (`Ctrl+Shift+S`): parametric → editable path | `xarast-app` | M | T5.1, T5.3 |
| T5.6 | Double-click a rectangle/ellipse activates its creating tool | `xarast-app` | S | T2.6 |

**Tricky part.** These stay **parametric indefinitely** (`research/04 §3` item 5): a
rounded rectangle is a rectangle with a radius, not a path, until the user asks otherwise.
That means the scene walker generates its geometry on demand and the node stores
parameters — and it means scaling a rounded rectangle has to decide what happens to the
radius. Xara's behaviour here is **to be determined in this phase**: it is determined by
scripted comparison against the original in the VM (scale a rounded rectangle by 2× in one
axis, observe the corner), and the result recorded in `docs/memory/document-model.md`.

---

### W6 — Path tools

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T6.1 | Bézier/shape tool: node and control-handle selection, marquee over nodes, move | `xarast-app` | L | W2, W3 |
| T6.2 | Add point, delete point, with correct curve continuity | `xarast-app`, `xarast-geom` | M | T6.1 |
| T6.3 | Make line / make curve on selected segments | `xarast-app` | M | T6.1 |
| T6.4 | Smooth / cusp toggling, with handle synchronisation | `xarast-app` | M | T6.1 |
| T6.5 | Close path and auto-close | `xarast-app` | S | T6.1 |
| T6.6 | Select all / deselect all path points | `xarast-app` | S | T6.1 |
| T6.7 | Winding-rule attribute on the path | `xarast-doc`, `xarast-app` | S | — |
| T6.8 | Pen tool: click for corner, drag for smooth, live rubber-band preview, finish on `Enter`/`Esc`/close | `xarast-app` | L | W2 |
| T6.9 | Freehand tool: sample capture, curve fitting with a configurable smoothing parameter, live preview of the fitted path | `xarast-app`, `xarast-geom` | L | W2 |
| T6.10 | Freehand rub-out: retracing backwards over the fresh stroke erases it | `xarast-app` | M | T6.9 |
| T6.11 | Path-point nudge family | `xarast-app` | S | T4.10 |

**Tricky parts.**

*Point selection lives outside the geometry* (`research/02 §10.5(a)` and the model memory
note, decision 9): a `HashMap<NodeId, BitVec>` in `EditState`, not a flag inside
`PathData`. Otherwise selecting a point mutates the path, which breaks `Arc` copy-on-write
and makes geometry non-comparable with `==`.

*Freehand fitting quality is the tool's whole value.* Fit incrementally so the preview
keeps up at 200 Hz input, and re-fit the finished stroke once at higher quality on commit.
The smoothing parameter is exposed in the infobar. The fitting algorithm itself is
`xarast-geom`'s (Phase 1); this phase supplies the interaction and the incremental
strategy.

*The pen tool's preview shows the segment that does not exist yet* — the rubber band from
the last committed point to the pointer, including the control handle being dragged. That
is a `Preview`, not a node.

---

### W7 — Structure operations

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T7.1 | Group / ungroup, nested, preserving appearance through the attribute scope | `xarast-app`, `xarast-doc` | L | W1 |
| T7.2 | Z-order: bring to front, send to back, forward one, backward one, layer up, layer down | `xarast-app` | M | W1 |
| T7.3 | Align and distribute: 9 anchors, both axes, align to selection/page/first-selected | `xarast-app` | L | W1 |
| T7.4 | Cut / copy / paste / paste-in-place | `xarast-app` | L | W1, Phase 5 clipboard |
| T7.5 | Clipboard interchange: internal format plus SVG and PNG flavours out, SVG and image in | `xarast-app`, `xarast-format` | L | T7.4 |
| T7.6 | Duplicate (with configurable offset) and clone (in place) | `xarast-app` | S | T7.4 |
| T7.7 | Delete, with the retained-node rule so undo can restore it | `xarast-app` | S | W1 |
| T7.8 | Layer operations from the panel: move selection to layer, new layer, delete layer | `xarast-app`, `xarast-ui` | M | W1 |

**Tricky part.** *Grouping must preserve appearance*, and that is exactly why the
attribute model is lexically scoped (architecture §3.6). When objects that inherit
different attributes from different positions are grouped, the grouping operation has to
materialise the attributes each object was actually inheriting, or the group changes how
things look. This is the single most error-prone operation in the phase and deserves a
dedicated property test: *for any selection, group-then-ungroup renders identically to
the original, pixel for pixel*.

---

### W8 — Snapping, grid and guides

| ID | Task | Crate | Size | Depends on |
|---|---|---|---|---|
| T8.1 | `SnapSource` trait and the snap resolver (nearest candidate within a radius, priority order) | `xarast-app` | M | — |
| T8.2 | Grid snapping (rectangular), per-spread spacing and subdivisions | `xarast-app` | M | T8.1 |
| T8.3 | Guide snapping | `xarast-app` | S | T8.1 |
| T8.4 | Mid-drag toggles: `NumPad .` grid, `NumPad 2` guides (and `NumPad *` reserved for object snap) | `xarast-app` | M | T2.4 |
| T8.5 | Snap feedback: cursor change and a transient overlay marker at the snapped point | `xarast-ui` | M | T3.6 |
| T8.6 | Guide creation by dragging from a ruler; guide properties, delete, delete all | `xarast-app`, `xarast-ui` | M | Phase 5 rulers |
| T8.7 | Snapping applies to every gesture that produces a position: create, move, scale handles, rotation centre, node edit, guide drag | `xarast-app` | M | T8.1 |

**Tricky part.** Snapping belongs to the *gesture*, not to the tool: the resolver takes
the candidate document point and the active constraints and returns the snapped point plus
what it snapped to (for feedback). That way adding object snapping later is one more
`SnapSource` implementation and nothing else changes.

---

## Public API introduced

```rust
// ─────────────────────────── crates/xarast-app/src/command.rs

/// Every mutation of the document is one of these. Tools construct them; the bus
/// applies them. A `Command` knows how to build its own inverse.
#[derive(Debug, Clone)]
pub enum Command {
    // structure
    CreateNode   { parent: NodeId, after: Option<NodeId>, kind: Box<NodeKind>,
                   attrs: AttrSet },
    DeleteNodes  { nodes: Vec<NodeId> },
    Group        { nodes: Vec<NodeId> },
    Ungroup      { groups: Vec<NodeId> },
    Reorder      { nodes: Vec<NodeId>, op: ZOrderOp },
    MoveToLayer  { nodes: Vec<NodeId>, layer: NodeId },
    Duplicate    { nodes: Vec<NodeId>, offset: DocVec },
    Clone        { nodes: Vec<NodeId> },
    Paste        { payload: ClipboardPayload, at: PasteTarget },

    // geometry and transform
    TransformNodes { nodes: Vec<NodeId>, xf: Matrix, scale_line_widths: bool },
    SetShapeParams { node: NodeId, params: ShapeParams },   // rect radius, ellipse radii
    ReplacePath    { node: NodeId, path: Arc<PathData> },
    EditPathPoints { node: NodeId, edit: PathEdit },
    ConvertToShapes{ nodes: Vec<NodeId> },

    // attributes
    SetAttr      { nodes: Vec<NodeId>, attr: AttrValue },
    SetWindingRule { nodes: Vec<NodeId>, rule: FillRule },

    // layout
    Align        { nodes: Vec<NodeId>, spec: AlignSpec },
    Distribute   { nodes: Vec<NodeId>, spec: DistributeSpec },
}

impl Command {
    /// Human-readable, shown in the Undo menu ("Move", "Group", "Apply Fill").
    pub fn label(&self) -> &'static str;
    /// Whether this command may merge with the previous one into one undo step.
    pub fn coalesces_with(&self, prev: &Command) -> bool;
}

/// The only way to change a document. Tools own a `&mut CommandSink`, never a
/// `&mut Document` — that is what makes undo complete by construction.
pub trait CommandSink {
    fn emit(&mut self, cmd: Command);
    fn emit_all(&mut self, cmds: impl IntoIterator<Item = Command>);
}

pub struct CommandBus { /* … */ }

impl CommandBus {
    pub fn apply(&mut self, doc: &mut Document, edit: &mut EditState, cmd: Command)
        -> Result<CommandOutcome, EditError>;
    pub fn undo(&mut self, doc: &mut Document, edit: &mut EditState) -> Option<&'static str>;
    pub fn redo(&mut self, doc: &mut Document, edit: &mut EditState) -> Option<&'static str>;
    pub fn undo_label(&self) -> Option<&'static str>;
    pub fn redo_label(&self) -> Option<&'static str>;
    /// Opens a coalescing window: everything until `end_gesture` is one undo step.
    pub fn begin_gesture(&mut self, label: &'static str);
    pub fn end_gesture(&mut self);
}

pub struct CommandOutcome {
    pub dirty: DocRect,
    pub selection_changed: bool,
    pub created: Vec<NodeId>,
}

// ─────────────────────────── crates/xarast-app/src/tool.rs

/// A tool sees the document read-only, writes to a preview, and emits commands.
pub trait Tool: 'static {
    fn id(&self) -> ToolId;
    fn name(&self) -> &'static str;

    fn on_activate(&mut self, cx: &mut ToolCtx) {}
    fn on_deactivate(&mut self, cx: &mut ToolCtx) {}

    /// Called for every interaction event, already resolved by the shared machine.
    fn on_gesture(&mut self, ev: &GestureEvent, cx: &mut ToolCtx);

    /// Handles and feedback this tool wants drawn on the overlay this frame.
    fn overlay(&self, cx: &ToolCtx, out: &mut OverlayBuilder);

    /// The numeric infobar for this tool.
    fn infobar(&mut self, ui: &mut egui::Ui, cx: &mut ToolCtx);

    fn cursor(&self, cx: &ToolCtx) -> CursorKind;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ToolId {
    Selector, Rectangle, Ellipse, Bezier, Pen, Freehand, Zoom, Push,
    // reserved for later phases
    Fill, Transparency, Text,
}

pub struct ToolCtx<'a> {
    /// Read-only. There is deliberately no `&mut Document` anywhere in this API.
    pub doc: &'a Document,
    pub edit: &'a EditState,
    pub viewport: &'a Viewport,
    pub snap: &'a SnapResolver,
    pub modifiers: Modifiers,
    pub preview: &'a mut Preview,
    pub commands: &'a mut dyn CommandSink,
    pub select: &'a mut dyn SelectionSink,
}

/// Events the shared state machine produces. Tools never see raw platform input.
#[derive(Debug, Clone)]
pub enum GestureEvent {
    Hover      { at: DocPoint, hit: Option<HitResult> },
    Click      { at: DocPoint, hit: Option<HitResult>, count: u8 },
    DragStart  { from: DocPoint, hit: Option<HitResult> },
    DragUpdate { from: DocPoint, to: DocPoint, snapped: Option<SnapHit> },
    DragEnd    { from: DocPoint, to: DocPoint, snapped: Option<SnapHit> },
    Cancel,
    /// Delivered even with no pointer movement (live modifiers, research/04 §3.15).
    ModifiersChanged { modifiers: Modifiers, at: DocPoint },
    Key        { key: Key, modifiers: Modifiers },
    Stroke     { samples: &'static [StrokeSample] },   // freehand: all samples this frame
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolState { Idle, Hover, ArmedDrag, Dragging, Committing, Cancelled }

pub struct ToolMachine { /* owns ToolState, drag threshold, momentary switch stack */ }

impl ToolMachine {
    pub fn set_active(&mut self, id: ToolId, cx: &mut ToolCtx);
    pub fn push_momentary(&mut self, id: ToolId, cx: &mut ToolCtx);
    pub fn pop_momentary(&mut self, cx: &mut ToolCtx);
    pub fn state(&self) -> ToolState;
    pub fn handle_input(&mut self, input: CanvasInput, cx: &mut ToolCtx);
}

// ─────────────────────────── preview: live feedback without mutation

/// Applied by the scene walker while building the display list; never committed.
#[derive(Debug, Default, Clone)]
pub struct Preview {
    pub transform: Option<(Vec<NodeId>, Matrix)>,
    pub geometry:  HashMap<NodeId, Arc<PathData>>,
    /// Objects being created that do not exist in the arena yet.
    pub phantom:   Vec<PhantomNode>,
    pub hidden:    Vec<NodeId>,
}

impl Preview {
    pub fn clear(&mut self);
    pub fn is_empty(&self) -> bool;
}

// ─────────────────────────── selection and hit testing

pub trait SelectionSink {
    fn set(&mut self, nodes: impl IntoIterator<Item = NodeId>);
    fn add(&mut self, node: NodeId);
    fn toggle(&mut self, node: NodeId);
    fn clear(&mut self);
    fn set_points(&mut self, node: NodeId, points: BitVec);
}

#[derive(Debug, Clone, Copy)]
pub struct HitResult {
    pub node: NodeId,
    /// The outermost group containing `node`, which is what a plain click selects.
    pub top_group: NodeId,
    pub part: HitPart,
    pub distance_px: f32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HitPart { Fill, Stroke, Node(u32), ControlHandle(u32), BBoxHandle(BBoxHandle),
                   RotationCentre, ShapeHandle(u8) }

pub struct HitTester<'a> { /* … */ }
impl<'a> HitTester<'a> {
    pub fn pick(&self, at: DocPoint, tolerance_px: f32, mode: PickMode) -> Option<HitResult>;
    pub fn pick_all(&self, rect: DocRect, mode: MarqueeMode) -> Vec<NodeId>;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickMode { TopGroup, Leaf, Under { below: NodeId } }   // plain / Ctrl / Alt

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarqueeMode { Enclose, Touch }

// ─────────────────────────── transform specification

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Anchor { NW, N, NE, W, Centre, E, SW, S, SE }   // the 9-anchor grid

#[derive(Debug, Clone)]
pub struct TransformSpec {
    pub anchor: Anchor,
    pub lock_aspect: bool,
    pub scale_line_widths: bool,
}

#[derive(Debug, Clone, Copy)]
pub enum NudgeStep {
    One,          // no modifier
    Times5,       // Ctrl
    Times10,      // Shift
    Fifth,        // Ctrl+Shift
    OnePixel,     // Alt      — one device pixel at the current zoom
    TenPixels,    // Alt+Shift
}

#[derive(Debug, Clone, Copy)]
pub enum NudgeTarget { Objects, PathPoints, Fill }

pub fn nudge_vector(dir: Direction, step: NudgeStep, vp: &Viewport, prefs: &Preferences)
    -> DocVec;

// ─────────────────────────── snapping

pub trait SnapSource {
    fn candidates(&self, near: DocPoint, radius: DocScalar, out: &mut Vec<SnapCandidate>);
    fn kind(&self) -> SnapKind;
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SnapKind { Grid, Guide, Object }   // Object is reserved for a later phase

pub struct SnapResolver { /* registered sources + enabled flags */ }

impl SnapResolver {
    pub fn enabled(&self, kind: SnapKind) -> bool;
    pub fn set_enabled(&mut self, kind: SnapKind, on: bool);   // mid-drag toggles
    pub fn snap(&self, p: DocPoint, vp: &Viewport) -> (DocPoint, Option<SnapHit>);
}

#[derive(Debug, Clone, Copy)]
pub struct SnapHit { pub kind: SnapKind, pub at: DocPoint, pub axis: SnapAxis }

// ─────────────────────────── overlay (handles)

pub struct OverlayBuilder { /* … */ }
impl OverlayBuilder {
    pub fn handle(&mut self, at: DocPoint, style: HandleStyle);
    pub fn bbox(&mut self, rect: DocRect, style: BBoxStyle);
    pub fn marquee(&mut self, rect: DocRect);
    pub fn path_outline(&mut self, path: &PathData, style: OutlineStyle);
    pub fn rotation_centre(&mut self, at: DocPoint);
    pub fn snap_marker(&mut self, hit: SnapHit);
}
```

---

## Acceptance criteria

1. **Tools cannot mutate the document.** `ToolCtx` exposes no `&mut Document`; a
   compile-fail test (`trybuild`) asserts that a tool attempting to obtain one does not
   compile, and a grep test asserts no `impl Tool` block names `xarast_doc::Document` as
   mutable.
2. **Everything is undoable.** `cargo test -p xarast-app --test undo_completeness` drives a
   scripted session of ≥ 200 operations covering every `Command` variant, then undoes all
   of them, and asserts the resulting document is structurally identical to the initial
   one (canonical comparison) — and that redoing all of them reproduces the final state.
3. **A drag is one undo step.** A 500-sample move produces exactly one entry in the undo
   history, asserted by `history.len()`.
4. **Cancel leaves nothing behind.** For every tool, a scripted gesture interrupted by
   `Esc` at 10 different points leaves the document byte-identical (canonical hash
   unchanged) and the history length unchanged.
5. **Undo/redo budget.** `cargo bench -p xarast-app -- undo` reports ≤ 1 ms for a single
   edit (roadmap budget).
6. **Dual state.** `cargo test -p xarast-app selector::dual_state` asserts the
   click sequence selects → scale handles → rotate/skew handles + rotation centre → scale
   handles, and that selecting a different object resets to scale.
7. **Nudges.** A table-driven test covers all 24 variants × 3 families = 72 cases,
   asserting the exact displacement, including that `Alt` moves exactly one device pixel
   at three different zoom levels.
8. **Infobar anchors.** A table-driven test covers 9 anchors × {W, H, X, Y} × {locked,
   unlocked} = 72 cases, asserting which bounding-box point stays fixed and what the
   resulting bounds are.
9. **Unit parsing.** Every numeric field accepts `10mm`, `1in`, `3p6`, `72pt`, `0.5cm` and
   bare numbers in the document's current unit; a property test round-trips
   parse∘format for all 12 unit types.
10. **Group/ungroup preserves appearance.** A property test over the corpus: for any
    selection, rendering before grouping and after group-then-ungroup are **pixel
    identical** on the CPU backend. Also that group-then-ungroup restores the original
    z-order.
11. **Live modifiers.** A scripted drag that presses `Ctrl` mid-gesture without moving the
    pointer changes the result (constrained), asserted for move, scale, rotate, rectangle
    and ellipse creation.
12. **Mid-drag snap toggle.** A scripted drag that presses `NumPad .` mid-gesture produces
    a snapped result from that point on; releasing it returns to free movement. Asserted
    for move and for node editing.
13. **Snapping correctness.** Grid snap lands exactly on grid intersections (exact
    millipoint equality, not approximate); guide snap lands exactly on the guide
    coordinate; the snap radius is respected in device pixels at three zoom levels.
14. **Parametric shapes stay parametric.** Creating, moving, scaling and rotating a
    rounded rectangle leaves the node kind unchanged; only `ConvertToShapes` turns it into
    a path, asserted by node-kind inspection.
15. **Path editing invariants.** Property tests: adding then deleting a point returns the
    original path; smooth⇄cusp⇄smooth is the identity for an already-smooth node;
    closing then reopening preserves point count; point selection lives in `EditState` and
    never changes `PathData` (asserted by `Arc::ptr_eq`).
16. **Freehand keeps up.** A 200 Hz synthetic stroke of 4,000 samples is fitted with no
    dropped samples, the preview updates every frame, and the committed path has a maximum
    deviation from the samples below the configured smoothing tolerance.
17. **Clipboard round trip.** Copy a selection, paste it in a second document, and the
    pasted objects are structurally equal to the originals; paste-in-place puts them at
    identical coordinates; SVG-flavour paste into Inkscape is checked manually per
    release.
18. **Hit-test correctness.** A golden hit-test suite: 200 (document, point) pairs with
    the expected node, covering fills, strokes, transparent interiors of unfilled shapes,
    nested groups, `Ctrl`-click leaf, `Alt`-click under, and locked/hidden layers.
19. **Handles never dirty the document.** Moving the pointer over the canvas with a
    selection active produces zero document tile invalidations (asserted by reading the
    Phase 4 `FrameTimings`/cache counters).
20. **Interaction latency.** Dragging 1,000 selected objects sustains ≤ 16 ms per frame on
    the reference machine, and pointer-to-handle latency stays ≤ 2 frames.
21. **All eight tools reachable.** `F2`/`Alt+S`/space, `F3`, `F4`, `Shift+F3`, `Shift+F4`,
    `Shift+F5`, `Shift+F7`/`Alt+Z`, `Shift+F8`/`Alt+X` activate the right tool, and the
    momentary variants restore the previous tool on release — one test per row of the
    shortcut table.

---

## Performance budgets

| Budget | Target | Measured how |
|---|---|---|
| Undo or redo of one edit | ≤ 1 ms | `criterion`; roadmap budget |
| Drag of 1,000 selected objects | ≤ 16 ms per frame | scripted session, frame histogram |
| Drag of 10,000 selected objects | ≤ 33 ms per frame (degraded but usable) | same |
| Hit test, 100k-object document | ≤ 2 ms worst case | `criterion`, points chosen adversarially (deep group nesting, dense overlap) |
| Marquee selection over 100k objects | ≤ 20 ms | `criterion` |
| Preview application in the scene walker (1,000 previewed nodes) | ≤ 1 ms | `criterion` |
| Freehand sample→preview latency | ≤ 1 frame | instrumented test |
| Group of 5,000 objects | ≤ 50 ms | `criterion` |
| Align/distribute of 5,000 objects | ≤ 50 ms | `criterion` |
| Memory per undo step (typical move of 100 objects) | ≤ 8 KB | `Action::size_hint` asserted |
| Handle overlay rebuild per frame | ≤ 0.5 ms | `criterion` |

---

## Risks and mitigations

| # | Risk | Likelihood | Impact | Mitigation |
|---|---|---|---|---|
| K1 | A tool takes a shortcut and mutates the document directly "just this once" | Medium | **Very high** | The API makes it impossible: `ToolCtx` has no mutable document. Enforced by a compile-fail test, not by review |
| K2 | The preview mechanism diverges from what commits, so what you see is not what you get | Medium | High | Every gesture test asserts that the previewed render and the post-commit render are pixel identical on the CPU backend |
| K3 | Group/ungroup changes appearance because of attribute scoping | High | High | The lexically scoped attribute model is deliberate (architecture §3.6); criterion 10 is a pixel-level property test over the whole corpus |
| K4 | Coalescing collapses operations that should be separate undo steps | Medium | Medium | Coalescing rules are per command kind and unit tested; the undo menu label is part of the assertion, so a wrong merge shows up as a wrong label |
| K5 | The dual state feels wrong to people who used Xara | Medium | High | Scripted side-by-side comparison against the original in the VM for the click sequences, plus a manual "does this feel right" pass recorded in the phase note. Some behaviours are not documented anywhere and must be observed |
| K6 | Hit testing is too slow on deeply nested documents | Medium | Medium | Bounds-tree acceleration with the Phase 2 bounds cache; adversarial cases in the benchmark from day one |
| K7 | Freehand drops samples and strokes look angular | Medium | Medium | Phase 5 already guarantees every sample of a frame is delivered; this phase asserts none is dropped between the shell and the fitter |
| K8 | Scaling a rounded rectangle does the wrong thing with the radius | Medium | Low | Behaviour observed against the original and recorded; until then, documented as to be determined in this phase |
| K9 | Snap feels sticky or jumpy at high zoom | Medium | Medium | Snap radius is in device pixels; tested at three zoom levels; the transient marker gives the user an explanation for every jump |
| K10 | The selection outlives the nodes it refers to after deletion or undo | Medium | High | T1.6 invalidation rules plus a property test: after any random command sequence, every selected id is reachable from the root (model invariant 9) |

---

## Test plan

**Unit.** Nudge vector table (72 cases); infobar anchor table (72 cases); unit parsing and
formatting for all 12 unit types; snap resolution against synthetic sources; z-order
operations on hand-built trees; align and distribute arithmetic; `Command::label` and
`coalesces_with` matrices.

**Property (`proptest`).**
- Random command sequences: undo-all returns the canonical initial document; redo-all
  returns the canonical final document.
- Group∘ungroup is the identity on both structure and rendering.
- Path edits: add∘delete = identity; smooth⇄cusp involution; close∘open preserves points.
- After any sequence, the model invariants of `docs/memory/document-model.md` hold
  (`Tree::validate()`).
- Selection only ever contains nodes reachable from the root.

**Golden rendering.** For each tool, a scripted gesture that produces a shape, rendered on
the CPU backend and compared exactly to a committed golden. This catches geometry
regressions that structural tests miss (e.g. a corner radius applied on the wrong axis).

**Scripted interaction tests.** A `GestureScript` format — a list of synthetic
`CanvasInput` events with timestamps and modifier states — replayed headlessly against the
tool machine. Every criterion above that says "a scripted gesture" is one of these. They
run without a window, which is what makes them CI-viable.

**UI tests.** `egui_kittest` snapshots for each tool's infobar, in both themes, with and
without a selection.

**Performance.** `criterion` per budget row; a scripted 60 s editing session producing a
frame histogram with p50/p99 asserted.

**Manual, per phase (recorded in the phase note).** Side-by-side with the original in the
VM: the dual-state click sequence, a freehand stroke at speed, a mid-drag snap toggle, a
nudge of each of the six step sizes, and grouping objects that inherit different
attributes. These are the behaviours no assertion fully captures and they are the reason
`research/04 §3` exists.

---

## Memory note

Two notes get updated.

**`docs/memory/ui.md`** — add an "Editing and tools" section recording:

- **Current state:** which tools exist and what each can do; which gestures are stubbed.
- **Decisions:** the tool state machine and its six invariant rules; the `Preview`
  mechanism and why it replaces temporary mutation; the coalescing policy; handle
  tolerance in device pixels; the dual-state click sequence as implemented; the snap
  priority order.
- **Observed original behaviour:** everything determined by comparison in the VM —
  notably the rounded-rectangle scaling rule, the exact marquee touch/enclose semantics,
  the rotation-centre persistence rule, and the drag threshold in pixels. These are facts
  that exist nowhere else once the VM is gone; write them down with the observation
  method.
- **Dead ends:** any interaction approach tried and rejected, especially ones that felt
  fine in isolation and wrong in use.
- **Open TODOs:** QuickShape, booleans, join/break, object snapping, pressure-driven
  width, and the P1 path operations not implemented here.

**`docs/memory/document-model.md`** — update with:

- The final `Command` set and the `Action` variants each maps to, plus the `size_hint`
  values used for the byte budget.
- Confirmation (or correction) of the model note's decision 9: point selection lives in
  `EditState`, and the `Arc` copy-on-write property it protects.
- The resolution of the model note's open TODO about attribute scope on group/ungroup,
  with the test that guards it.
- Any invariant this phase discovered — in particular anything about attribute
  materialisation during grouping, and the retained-node rule for deletion and undo.

Also record the measured budget rows in **`docs/memory/perf.md`**.
