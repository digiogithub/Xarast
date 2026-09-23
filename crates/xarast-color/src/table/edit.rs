//! Editing the colour table: redefine, rename, reparent, remove, and the
//! derivation-graph bookkeeping those need.
//!
//! # The derivation graph is a DAG, and `reparent` is what keeps it one
//!
//! `reparent` is the only editing operation that changes an entry's `kind`
//! or `parent`, and it refuses a link that would close a loop or push any
//! chain past [`ColourTable::MAX_PARENT_DEPTH`] — the original refuses the
//! same links (`research/02 §5.10.1`). A loader, which must accept whatever
//! a file says, uses `insert`/`set_parent` and then [`ColourTable::repair_cycles`].
//!
//! # Repaint is driven by what changed, not by what was touched
//!
//! Every mutation re-resolves the edited entry and its descendants (in
//! [`ColourTable::resolve_order`], parents first) and reports the ids whose
//! quantised value moved. That list, plus the epoch bump, is all a repaint
//! needs: no tree walk.

use std::sync::Arc;

use super::{ColourDef, ColourId, ColourKind, ColourTable, PaletteEpoch};
use crate::{ColourModel, ColourValue};

/// A short list of colour ids, inline up to eight.
pub type ColourIds = smallvec::SmallVec<[ColourId; 8]>;

/// What happens to the things that refer to a colour being removed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum OnDelete {
    /// Derived entries keep their currently resolved value: each direct
    /// child becomes [`ColourKind::Normal`] with no parent, holding that
    /// value in its own model. (Object uses are the document's business:
    /// its delete command turns them into direct colours first.)
    Detach,
    /// Fail if any entry still derives from it.
    Reject,
}

/// Why a palette edit was refused. A refused edit leaves the table exactly
/// as it was.
#[derive(Clone, Copy, PartialEq, Eq, Debug, thiserror::Error)]
pub enum ColourEditError {
    /// The link would close a loop in the derivation graph.
    #[error("would create a cycle in the derivation graph")]
    Cycle,
    /// The link would make some chain longer than `MAX_PARENT_DEPTH`.
    #[error("parent chain would exceed ColourTable::MAX_PARENT_DEPTH")]
    TooDeep,
    /// The id, or the parent id, names no entry.
    #[error("no such colour")]
    NotFound,
    /// Something still derives from the entry; use [`OnDelete::Detach`].
    #[error("still referenced; use OnDelete::Detach")]
    StillReferenced,
    /// Another entry already has that name.
    #[error("a colour with that name already exists")]
    NameInUse,
    /// A tint, shade or link was given no parent.
    #[error("a tint, shade or linked colour needs a parent")]
    MissingParent,
    /// A normal or spot colour was given a parent.
    #[error("a normal or spot colour cannot have a parent")]
    UnexpectedParent,
    /// An inherit (`None`) component on a colour that is not linked.
    #[error("only a linked colour can inherit a component")]
    InheritWithoutLink,
}

impl ColourTable {
    /// The current palette epoch. Every mutation moves it forward.
    #[inline]
    #[must_use]
    pub fn epoch(&self) -> PaletteEpoch {
        self.epoch
    }

    /// Moves the epoch strictly past `other` (and past its own value).
    ///
    /// For an undo that swaps a whole table back in: the restored table's
    /// own epoch may already have been used by a table with different
    /// content on the abandoned branch, and an epoch must never name two
    /// different palettes.
    pub fn advance_epoch_past(&mut self, other: PaletteEpoch) {
        self.epoch = PaletteEpoch(self.epoch.0.max(other.0).wrapping_add(1));
    }

    /// Every entry, parents before children: a valid order for a full
    /// recompute. Cached until the next structural change.
    ///
    /// Entries caught in a cycle (only a loader can make one) come last, in
    /// slot order, after everything that can be ordered.
    #[must_use]
    pub fn resolve_order(&self) -> &[ColourId] {
        self.order.get_or_init(|| self.compute_order())
    }

    fn compute_order(&self) -> Arc<[ColourId]> {
        // Kahn's algorithm over "parent -> child" edges of derived entries.
        let mut pending: slotmap::SecondaryMap<ColourId, u8> = slotmap::SecondaryMap::new();
        let mut children: slotmap::SecondaryMap<ColourId, Vec<ColourId>> =
            slotmap::SecondaryMap::new();
        for (id, def) in self.defs.iter() {
            let edge = self.live_parent(def).filter(|p| *p != id);
            pending.insert(id, u8::from(edge.is_some()));
            if let Some(p) = edge
                && let Some(e) = children.entry(p)
            {
                e.or_default().push(id);
            }
        }
        let mut out: Vec<ColourId> = Vec::with_capacity(self.defs.len());
        out.extend(self.defs.keys().filter(|id| pending[*id] == 0));
        let mut i = 0;
        while i < out.len() {
            if let Some(kids) = children.get(out[i]) {
                for k in kids {
                    pending[*k] = 0;
                    out.push(*k);
                }
            }
            i += 1;
        }
        if out.len() < self.defs.len() {
            let placed: std::collections::HashSet<ColourId> = out.iter().copied().collect();
            let rest: Vec<ColourId> = self.defs.keys().filter(|id| !placed.contains(id)).collect();
            out.extend(rest);
        }
        out.into()
    }

    /// The parent an entry resolves through, if it is derived and the parent
    /// exists.
    fn live_parent(&self, def: &ColourDef) -> Option<ColourId> {
        def.parent
            .filter(|_| def.kind.is_derived())
            .filter(|p| self.defs.contains_key(*p))
    }

    /// The entries that derive directly from `id`.
    pub fn children(&self, id: ColourId) -> impl Iterator<Item = ColourId> + '_ {
        self.defs
            .iter()
            .filter(move |(c, d)| *c != id && self.live_parent(d) == Some(id))
            .map(|(c, _)| c)
    }

    /// `id` and every entry that derives from it, directly or not, parents
    /// first.
    #[must_use]
    pub fn descendants_of(&self, id: ColourId) -> ColourIds {
        let mut out = ColourIds::new();
        if !self.defs.contains_key(id) {
            return out;
        }
        out.push(id);
        for c in self.resolve_order() {
            if *c == id {
                continue;
            }
            if let Some(p) = self.defs.get(*c).and_then(|d| self.live_parent(d))
                && out.contains(&p)
            {
                out.push(*c);
            }
        }
        out
    }

    /// How many ancestors `id` has, following derived links; `None` if the
    /// chain loops or runs past the limit.
    fn ancestors(&self, id: ColourId) -> Option<usize> {
        let mut n = 0;
        let mut at = id;
        while let Some(p) = self.defs.get(at).and_then(|d| self.live_parent(d)) {
            n += 1;
            if n >= ColourTable::MAX_PARENT_DEPTH || p == id {
                return None;
            }
            at = p;
        }
        Some(n)
    }

    /// Recomputes the cached 8-bit value of `id` and everything derived
    /// from it, returning `id` plus every descendant whose cached value
    /// changed — what a repaint needs.
    ///
    /// The cached value is quantised as the original quantises it
    /// ([`ColourValue::to_rgba8_packed`]), so it agrees with the cached RGB
    /// a `.xar` file carries. Unknown `id`: returns an empty list.
    pub fn refresh_from(&mut self, id: ColourId) -> ColourIds {
        let affected = self.descendants_of(id);
        let mut changed = ColourIds::new();
        for c in affected.iter().copied() {
            let now = self.resolve(c).to_rgba8_packed();
            let Some(def) = self.defs.get_mut(c) else {
                continue;
            };
            if c == id || def.cached_rgb != now {
                changed.push(c);
            }
            def.cached_rgb = now;
        }
        if !affected.is_empty() {
            self.epoch = self.epoch.next();
        }
        changed
    }

    /// Recomputes every entry's cached value, parents first. For a table
    /// built by hand; a loader keeps the file's cached values instead, since
    /// they are the fallback for entries that cannot be resolved.
    pub fn refresh_all(&mut self) {
        let order: Vec<ColourId> = self.resolve_order().to_vec();
        for c in order {
            let now = self.resolve(c).to_rgba8_packed();
            if let Some(def) = self.defs.get_mut(c) {
                def.cached_rgb = now;
            }
        }
        self.epoch = self.epoch.next();
    }

    /// Changes an entry's own components and model. Returns every id whose
    /// resolved value changed, `id` included.
    ///
    /// # Errors
    ///
    /// [`ColourEditError::NotFound`], or
    /// [`ColourEditError::InheritWithoutLink`] for a `None` component on a
    /// colour that is not linked. Tints and shades keep their factor in
    /// their [`ColourKind`]; change it with [`ColourTable::reparent`].
    pub fn redefine(
        &mut self,
        id: ColourId,
        components: [Option<f32>; 4],
        model: ColourModel,
    ) -> Result<ColourIds, ColourEditError> {
        let def = self.defs.get_mut(id).ok_or(ColourEditError::NotFound)?;
        if def.kind != ColourKind::Linked && components.iter().any(Option::is_none) {
            return Err(ColourEditError::InheritWithoutLink);
        }
        let clamp = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        def.components = components.map(|c| c.map(clamp));
        def.model = model;
        Ok(self.refresh_from(id))
    }

    /// The named entries in colour-line order: by
    /// [`ColourDef::entry_index`], then by slot for equal indices (a file
    /// that wrote none, or colours created before an order was ever set).
    /// Unnamed entries are the document's local colours and are not listed.
    #[must_use]
    pub fn listed(&self) -> Vec<ColourId> {
        let mut v: Vec<(u32, usize, ColourId)> = self
            .defs
            .iter()
            .enumerate()
            .filter(|(_, (_, d))| d.name.is_some())
            .map(|(slot, (id, d))| (d.entry_index, slot, id))
            .collect();
        v.sort_unstable_by_key(|e| (e.0, e.1));
        v.into_iter().map(|e| e.2).collect()
    }

    /// The `entry_index` a colour added now takes so that it lists last.
    #[must_use]
    pub fn next_entry_index(&self) -> u32 {
        self.defs
            .values()
            .filter(|d| d.name.is_some())
            .map(|d| d.entry_index)
            .max()
            .map_or(1, |m| m.saturating_add(1))
    }

    /// Moves a named entry to position `to` of [`ColourTable::listed`]
    /// (clamped to the end), renumbering every listed entry's
    /// `entry_index` as `1..=n` in the new order. Returns whether the order
    /// changed; an unchanged order leaves the table, epoch included, as it
    /// was.
    ///
    /// # Errors
    ///
    /// [`ColourEditError::NotFound`] when `id` is not a listed (named)
    /// entry.
    pub fn move_listed(&mut self, id: ColourId, to: usize) -> Result<bool, ColourEditError> {
        let mut order = self.listed();
        let from = order
            .iter()
            .position(|x| *x == id)
            .ok_or(ColourEditError::NotFound)?;
        order.remove(from);
        let to = to.min(order.len());
        order.insert(to, id);
        if from == to {
            return Ok(false);
        }
        for (i, x) in order.iter().enumerate() {
            if let Some(d) = self.defs.get_mut(*x) {
                d.entry_index = u32::try_from(i + 1).unwrap_or(u32::MAX);
            }
        }
        self.epoch = self.epoch.next();
        Ok(true)
    }

    /// Renames an entry.
    ///
    /// # Errors
    ///
    /// [`ColourEditError::NameInUse`] when another live entry already has
    /// the name, because names are how `.xarast` and the UI refer to
    /// entries; [`ColourEditError::NotFound`].
    pub fn rename(&mut self, id: ColourId, name: Arc<str>) -> Result<(), ColourEditError> {
        if !self.defs.contains_key(id) {
            return Err(ColourEditError::NotFound);
        }
        if let Some(other) = self.by_name.get(&name)
            && *other != id
            && self.defs.contains_key(*other)
        {
            return Err(ColourEditError::NameInUse);
        }
        let def = self.defs.get_mut(id).ok_or(ColourEditError::NotFound)?;
        if let Some(old) = def.name.take()
            && self.by_name.get(&old) == Some(&id)
        {
            self.by_name.remove(&old);
        }
        def.name = Some(name.clone());
        self.by_name.insert(name, id);
        self.epoch = self.epoch.next();
        Ok(())
    }

    /// The only editing operation that changes `kind` or `parent`.
    ///
    /// - To **normal** or **spot** (`parent` must be `None`): a colour that
    ///   derived from a parent keeps its appearance — its resolved value is
    ///   copied into its own components, as the original does.
    /// - To **linked**: coming from another kind, all four components start
    ///   as overrides holding the current resolved value; an existing link
    ///   keeps its components.
    /// - To **tint** or **shade**: the entry takes its parent's model, as the
    ///   original does, and its components are rewritten in the file's
    ///   layout (`[factor, 0, 0, 0]`, `[x, y, 0, 0]`).
    ///
    /// Returns every id whose resolved value changed.
    ///
    /// # Errors
    ///
    /// [`ColourEditError::Cycle`] if `parent` is `id` or derives from it,
    /// [`ColourEditError::TooDeep`] if some chain would pass
    /// [`ColourTable::MAX_PARENT_DEPTH`], [`ColourEditError::NotFound`],
    /// [`ColourEditError::MissingParent`], [`ColourEditError::UnexpectedParent`].
    pub fn reparent(
        &mut self,
        id: ColourId,
        kind: ColourKind,
        parent: Option<ColourId>,
    ) -> Result<ColourIds, ColourEditError> {
        let old = self.defs.get(id).ok_or(ColourEditError::NotFound)?;
        match (kind.is_derived(), parent) {
            (true, None) => return Err(ColourEditError::MissingParent),
            (false, Some(_)) => return Err(ColourEditError::UnexpectedParent),
            _ => {}
        }
        if let Some(p) = parent {
            if !self.defs.contains_key(p) {
                return Err(ColourEditError::NotFound);
            }
            if p == id || self.derives_from(p, id) {
                return Err(ColourEditError::Cycle);
            }
            let above = self.ancestors(p).ok_or(ColourEditError::TooDeep)? + 1;
            let below = self.height(id);
            if above + below >= ColourTable::MAX_PARENT_DEPTH {
                return Err(ColourEditError::TooDeep);
            }
        }

        let current = self.resolve(id);
        let own_model = old.model;
        let was_linked = old.kind == ColourKind::Linked;
        let full = |v: ColourValue, m: ColourModel| v.to_model(m).components().map(Some);
        let (model, components) = match (&kind, parent) {
            (ColourKind::Normal | ColourKind::Spot, _) => (own_model, full(current, own_model)),
            (ColourKind::Linked, _) if was_linked => (own_model, old.components),
            (ColourKind::Linked, _) => (own_model, full(current, own_model)),
            (ColourKind::Tint { factor }, Some(p)) => (
                self.defs[p].model,
                [
                    Some(factor.clamp(0.0, 1.0)),
                    Some(0.0),
                    Some(0.0),
                    Some(0.0),
                ],
            ),
            (ColourKind::Shade { x, y }, Some(p)) => (
                self.defs[p].model,
                [
                    Some(x.clamp(-1.0, 1.0)),
                    Some(y.clamp(-1.0, 1.0)),
                    Some(0.0),
                    Some(0.0),
                ],
            ),
            _ => unreachable!("derived kinds were checked to have a parent"),
        };
        let kind = match kind {
            ColourKind::Tint { factor } => ColourKind::Tint {
                factor: if factor.is_nan() {
                    1.0
                } else {
                    factor.clamp(0.0, 1.0)
                },
            },
            ColourKind::Shade { x, y } => {
                let s = |v: f32| if v.is_nan() { 0.0 } else { v.clamp(-1.0, 1.0) };
                ColourKind::Shade { x: s(x), y: s(y) }
            }
            k => k,
        };
        let def = self.defs.get_mut(id).ok_or(ColourEditError::NotFound)?;
        def.kind = kind;
        def.parent = parent;
        def.model = model;
        def.components = components;
        self.structure_changed();
        Ok(self.refresh_from(id))
    }

    /// Whether `id` derives, directly or not, from `ancestor`. Bounded by
    /// the depth limit, so a corrupt loop cannot hang it.
    #[must_use]
    pub fn derives_from(&self, id: ColourId, ancestor: ColourId) -> bool {
        let mut at = id;
        for _ in 0..=ColourTable::MAX_PARENT_DEPTH {
            match self.defs.get(at).and_then(|d| self.live_parent(d)) {
                Some(p) if p == ancestor => return true,
                Some(p) => at = p,
                None => return false,
            }
        }
        // A chain this long is broken anyway; treat it as a loop through us.
        true
    }

    /// The longest chain of descendants below `id` (0 for a leaf).
    fn height(&self, id: ColourId) -> usize {
        let desc = self.descendants_of(id);
        desc.iter()
            .map(|d| {
                let mut n = 0;
                let mut at = *d;
                while at != id {
                    match self.defs.get(at).and_then(|x| self.live_parent(x)) {
                        Some(p) if n <= ColourTable::MAX_PARENT_DEPTH => {
                            n += 1;
                            at = p;
                        }
                        _ => break,
                    }
                }
                n
            })
            .max()
            .unwrap_or(0)
    }

    /// Removes an entry.
    ///
    /// Returns the entries that were detached (empty under
    /// [`OnDelete::Reject`]). The table never holds a dangling parent link
    /// afterwards.
    ///
    /// # Errors
    ///
    /// [`ColourEditError::NotFound`];
    /// [`ColourEditError::StillReferenced`] under [`OnDelete::Reject`] when
    /// an entry derives from it.
    pub fn remove(&mut self, id: ColourId, policy: OnDelete) -> Result<ColourIds, ColourEditError> {
        if !self.defs.contains_key(id) {
            return Err(ColourEditError::NotFound);
        }
        let kids: ColourIds = self.children(id).collect();
        if policy == OnDelete::Reject && !kids.is_empty() {
            return Err(ColourEditError::StillReferenced);
        }
        for k in kids.iter().copied() {
            let value = self.resolve(k);
            if let Some(def) = self.defs.get_mut(k) {
                def.components = value.to_model(def.model).components().map(Some);
                def.kind = ColourKind::Normal;
                def.parent = None;
            }
        }
        // Normal entries may still carry a stale link; never leave it
        // pointing at a slot that could be reused.
        for (_, def) in self.defs.iter_mut() {
            if def.parent == Some(id) {
                def.parent = None;
            }
        }
        if let Some(def) = self.defs.remove(id)
            && let Some(n) = def.name
            && self.by_name.get(&n) == Some(&id)
        {
            self.by_name.remove(&n);
        }
        self.structure_changed();
        Ok(kids)
    }

    /// Breaks every cycle and over-deep chain a loader let in, so that the
    /// graph is a DAG again. Returns the entries demoted.
    ///
    /// Each offending entry becomes a normal colour holding what it resolves
    /// to now — which, for an entry in a loop, is the file's cached RGB — so
    /// the document looks the same and nothing is rejected. The entry
    /// demoted is the youngest (highest slot) one on the loop.
    pub fn repair_cycles(&mut self) -> ColourIds {
        let mut demoted = ColourIds::new();
        loop {
            let bad = self
                .defs
                .keys()
                .filter(|id| self.ancestors(*id).is_none())
                .max_by_key(|id| slotmap::Key::data(id).as_ffi() & 0xFFFF_FFFF);
            let Some(bad) = bad else { break };
            // Walk up from it. If the walk comes back to an entry it has
            // seen, the entries from there on are the loop: demote the
            // youngest of them. Otherwise the chain is merely too deep, and
            // demoting `bad` itself shortens every chain through it.
            let mut seen: Vec<ColourId> = Vec::new();
            let mut at = bad;
            let mut looped_at = None;
            while let Some(p) = self.defs.get(at).and_then(|d| self.live_parent(d)) {
                if let Some(i) = seen.iter().position(|s| *s == at) {
                    looped_at = Some(i);
                    break;
                }
                if seen.len() > 2 * ColourTable::MAX_PARENT_DEPTH {
                    break;
                }
                seen.push(at);
                at = p;
            }
            let victim = match looped_at {
                Some(i) => seen[i..]
                    .iter()
                    .copied()
                    .max_by_key(|id| slotmap::Key::data(id).as_ffi() & 0xFFFF_FFFF)
                    .unwrap_or(bad),
                None => bad,
            };
            let value = self.resolve(victim);
            if let Some(def) = self.defs.get_mut(victim) {
                def.components = value.to_model(def.model).components().map(Some);
                def.kind = ColourKind::Normal;
                def.parent = None;
            }
            self.structure_changed();
            demoted.push(victim);
        }
        demoted
    }
}
