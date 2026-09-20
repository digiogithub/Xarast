//! The benchmark that settles `docs/10-architecture.md` §3.1: a generational
//! arena versus a persistent HAMT as the **live** store.
//!
//! The decision rule is pre-registered in
//! `docs/phases/phase-02-document-model.md` §W2.11, before this file was
//! written, so the result cannot be rationalised afterwards:
//!
//! > The arena stands unless *all three* hold: the HAMT is within 20 % of the
//! > arena on A and B; the HAMT beats the arena by more than 5× on C; and the
//! > HAMT's memory in D is within 1.5× of the arena's.
//!
//! Four measurements, on a synthetic 100 000-node document:
//!
//! | M | What | Why |
//! |---|---|---|
//! | A | full render traversal, nodes per second | the render hot path |
//! | B | 10 000 000 random lookups by `NodeId` | hit-test and resolution |
//! | C | one attribute edit, then undo | the 1 ms budget |
//! | D | resident bytes for the whole document | large-document viability |
//!
//! Run it as:
//!
//! ```text
//! cargo run --release -p xarast-doc --example arena_vs_persistent -- time
//! cargo run --release -p xarast-doc --example arena_vs_persistent -- mem-arena
//! cargo run --release -p xarast-doc --example arena_vs_persistent -- mem-hamt
//! ```
//!
//! D is measured in a process of its own for each representation, because
//! resident memory does not reliably come back down after a free.

use std::sync::Arc;
use std::time::{Duration, Instant};

use xarast_doc::{
    AttrSlot, AttrValue, Command, CommandBus, Document, EditError, NodeData, NodeId, NodeKind,
    SynthSpec, Tx, WalkEvent, synthetic_document,
};
use xarast_geom::Mp;

/// The same tree, stored in a persistent hash array mapped trie.
///
/// It implements exactly the operations the benchmark measures, and nothing
/// else — the point is to compare the representations, not to write a second
/// document model.
struct Hamt {
    nodes: imbl::HashMap<NodeId, Arc<NodeData>>,
    root: NodeId,
}

impl Hamt {
    fn from_document(doc: &Document) -> Hamt {
        let mut nodes = imbl::HashMap::new();
        for id in doc.tree.preorder(doc.tree.root()) {
            if let Some(d) = doc.tree.get(id) {
                nodes.insert(id, Arc::new(d.clone()));
            }
        }
        Hamt {
            nodes,
            root: doc.tree.root(),
        }
    }

    fn get(&self, id: NodeId) -> Option<&Arc<NodeData>> {
        self.nodes.get(&id)
    }

    /// The same event sequence `Tree::walk_render` produces, over the HAMT.
    fn walk(&self) -> usize {
        // The scope events carry no payload here: the benchmark counts the
        // same work the arena walk does, and nothing consumes the parent.
        enum Step {
            Enter,
            Visit(NodeId),
            Leave,
        }
        let mut visits = 0usize;
        let mut stack = vec![Step::Visit(self.root)];
        while let Some(step) = stack.pop() {
            match step {
                Step::Enter | Step::Leave => {}
                Step::Visit(n) => {
                    visits += 1;
                    let Some(d) = self.nodes.get(&n) else {
                        continue;
                    };
                    if d.links.first_child.is_none() {
                        continue;
                    }
                    stack.push(Step::Leave);
                    let mut kids = Vec::new();
                    let mut c = d.links.first_child;
                    while let Some(k) = c {
                        kids.push(k);
                        c = self.nodes.get(&k).and_then(|x| x.links.next);
                    }
                    for k in kids.into_iter().rev() {
                        stack.push(Step::Visit(k));
                    }
                    stack.push(Step::Enter);
                }
            }
        }
        visits
    }

    /// One attribute edit. Returns the previous version, which is what undo
    /// costs in a persistent store: a pointer swap.
    fn set_attr(&mut self, id: NodeId, value: AttrValue) -> imbl::HashMap<NodeId, Arc<NodeData>> {
        let previous = self.nodes.clone();
        if let Some(d) = self.nodes.get(&id) {
            let mut next = (**d).clone();
            if let NodeKind::Attr(a) = &mut next.kind {
                a.value = value;
            }
            self.nodes.insert(id, Arc::new(next));
        }
        previous
    }
}

#[derive(Debug)]
struct SetWidth(NodeId, i32);

impl Command for SetWidth {
    fn label(&self) -> &'static str {
        "set width"
    }
    fn run(&self, tx: &mut Tx<'_>) -> Result<(), EditError> {
        tx.set_attr(self.0, AttrValue::LineWidth(Mp::new(self.1)))
    }
}

fn resident_bytes() -> u64 {
    let Ok(s) = std::fs::read_to_string("/proc/self/statm") else {
        return 0;
    };
    let pages: u64 = s
        .split_whitespace()
        .nth(1)
        .and_then(|x| x.parse().ok())
        .unwrap_or(0);
    pages * 4096
}

fn first_attr(doc: &Document) -> NodeId {
    doc.tree
        .preorder(doc.tree.root())
        .find(|id| matches!(doc.tree.kind(*id), Some(NodeKind::Attr(_))))
        .expect("the synthetic document has attribute nodes")
}

fn time_it(iters: u32, mut f: impl FnMut()) -> Duration {
    // One warm-up pass, then the measured run.
    f();
    let t = Instant::now();
    for _ in 0..iters {
        f();
    }
    t.elapsed() / iters
}

fn main() {
    let mode = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "time".to_string());
    let spec = SynthSpec::default();

    match mode.as_str() {
        "mem-arena" => {
            let base = resident_bytes();
            let doc = synthetic_document(spec);
            let n = doc.tree.node_count();
            let used = resident_bytes().saturating_sub(base);
            println!(
                "D arena  nodes={n} resident_bytes={used} per_node={}",
                used / n as u64
            );
            std::hint::black_box(&doc);
        }
        "mem-hamt" => {
            let doc = synthetic_document(spec);
            let n = doc.tree.node_count();
            let base = resident_bytes();
            let hamt = Hamt::from_document(&doc);
            let used = resident_bytes().saturating_sub(base);
            println!(
                "D hamt   nodes={n} resident_bytes={used} per_node={}",
                used / n as u64
            );
            std::hint::black_box(&hamt.nodes.len());
        }
        _ => {
            let mut doc = synthetic_document(spec);
            let n = doc.tree.node_count();
            let root = doc.tree.root();
            let ids: Vec<NodeId> = doc.tree.preorder(root).collect();
            let hamt_doc = synthetic_document(spec);
            let mut hamt = Hamt::from_document(&hamt_doc);
            println!("document: {n} nodes");

            // ── A: full render traversal ────────────────────────────────────
            let a_arena = time_it(20, || {
                let mut v = 0usize;
                for ev in doc.tree.walk_render(root) {
                    if let WalkEvent::Visit { .. } = ev {
                        v += 1;
                    }
                }
                std::hint::black_box(v);
            });
            let a_hamt = time_it(20, || {
                std::hint::black_box(hamt.walk());
            });
            println!(
                "A traversal   arena {:>9.3} ms ({:>6.1} M nodes/s)   hamt {:>9.3} ms ({:>6.1} M nodes/s)   ratio {:.2}x",
                a_arena.as_secs_f64() * 1e3,
                n as f64 / a_arena.as_secs_f64() / 1e6,
                a_hamt.as_secs_f64() * 1e3,
                n as f64 / a_hamt.as_secs_f64() / 1e6,
                a_hamt.as_secs_f64() / a_arena.as_secs_f64()
            );

            // ── B: 10 000 000 random lookups ────────────────────────────────
            const LOOKUPS: usize = 10_000_000;
            let mut i = 0usize;
            let t = Instant::now();
            let mut hits = 0usize;
            for _ in 0..LOOKUPS {
                i = i.wrapping_mul(2_654_435_761).wrapping_add(12_345) % ids.len();
                if doc.tree.get(ids[i]).is_some() {
                    hits += 1;
                }
            }
            let b_arena = t.elapsed();
            std::hint::black_box(hits);

            let mut i = 0usize;
            let t = Instant::now();
            let mut hits = 0usize;
            for _ in 0..LOOKUPS {
                i = i.wrapping_mul(2_654_435_761).wrapping_add(12_345) % ids.len();
                if hamt.get(ids[i]).is_some() {
                    hits += 1;
                }
            }
            let b_hamt = t.elapsed();
            std::hint::black_box(hits);
            println!(
                "B lookups     arena {:>9.3} ms ({:>6.2} ns each)      hamt {:>9.3} ms ({:>6.2} ns each)      ratio {:.2}x",
                b_arena.as_secs_f64() * 1e3,
                b_arena.as_secs_f64() * 1e9 / LOOKUPS as f64,
                b_hamt.as_secs_f64() * 1e3,
                b_hamt.as_secs_f64() * 1e9 / LOOKUPS as f64,
                b_hamt.as_secs_f64() / b_arena.as_secs_f64()
            );

            // ── C: one attribute edit, then undo ────────────────────────────
            let attr = first_attr(&doc);
            let mut bus = CommandBus::new();
            let mut w = 0i32;
            let c_arena = time_it(2_000, || {
                w = (w + 1) % 4_000;
                bus.dispatch(&mut doc, &SetWidth(attr, w)).expect("edit");
                bus.history_mut().undo(&mut doc).expect("undo");
            });
            let hattr = first_attr(&hamt_doc);
            let mut w = 0i32;
            let c_hamt = time_it(2_000, || {
                w = (w + 1) % 4_000;
                let previous = hamt.set_attr(hattr, AttrValue::LineWidth(Mp::new(w)));
                hamt.nodes = previous; // undo: a pointer swap
            });
            println!(
                "C edit+undo   arena {:>9.3} us                       hamt {:>9.3} us                       ratio {:.2}x",
                c_arena.as_secs_f64() * 1e6,
                c_hamt.as_secs_f64() * 1e6,
                c_arena.as_secs_f64() / c_hamt.as_secs_f64()
            );
            println!(
                "              (C ratio above is arena/hamt: how many times faster the HAMT is)"
            );

            let _ = doc.defaults.get(AttrSlot::LineWidth);
            println!("\nrun `mem-arena` and `mem-hamt` for measurement D");
        }
    }
}
