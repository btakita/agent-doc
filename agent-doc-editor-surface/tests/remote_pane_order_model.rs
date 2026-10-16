//! Exhaustive model of Remote Dev pane order (`#tmuxpanesreversed`).
//!
//! A JetBrains Remote Dev backend (the operator's Coder workspace) reports no
//! split geometry: only which session documents are visible, which one is
//! selected, and which are open. The left-to-right order is therefore an
//! inference carried in [`RemoteLayoutMemory`]. This module checks that
//! inference against a ground-truth editor model.
//!
//! **Editor model.** `splits` is the true left-to-right list of the document
//! each split shows; `focus` indexes the focused split. Order only exists with
//! two or more splits, and Remote Dev cannot tell one split from a lagging
//! tracker (a one-document observation is degenerate by design, GH #105), so
//! the model covers editors of width two and three.
//!
//! **Identifiable transitions** — the ones the evidence determines exactly:
//! focus another split; switch any split (focused or not) to a document that is
//! not visible; close a tab so its split shows another document; close one of
//! three or more splits.
//!
//! **Observation channel.** Before the settled observation arrives, Coder can
//! deliver any sequence (length <= 3) of these lagging observations, each seen
//! in production traces:
//! - selection-first (GH #250): the new selection with the OLD visible set;
//! - stale hidden editor (GH #175): the new visible set plus the replaced
//!   document's still-tracked hidden editor;
//! - degenerate (GH #134, GH #253): one or zero visible documents.
//!
//! Visible lists arrive in tab-open order, never geometry order, so every
//! visible list is tried in every permutation (settled) or forwards and
//! reversed (lagging).
//!
//! **Invariant (inductive step).** If the memory equals the editor before a
//! transition, it equals the editor after the transition's settled
//! observation, whatever lagging observations preceded it. By induction the
//! memory then tracks the editor across any trace of identifiable
//! transitions.

use agent_doc_editor_surface::SurfaceColumn;
use agent_doc_editor_surface::remote_layout::{
    RemoteClientEditors, RemoteLayoutEvidence, RemoteLayoutMemory,
};

const DOCS: [&str; 5] = ["a.md", "b.md", "c.md", "d.md", "e.md"];
/// Longest run of lagging observations before the settled one.
const MAX_LAG: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Editor {
    splits: Vec<&'static str>,
    focus: usize,
    open: Vec<&'static str>,
}

impl Editor {
    fn focused(&self) -> &'static str {
        self.splits[self.focus]
    }

    fn memory(&self) -> RemoteLayoutMemory {
        RemoteLayoutMemory {
            columns: self
                .splits
                .iter()
                .map(|file| SurfaceColumn::new([*file]))
                .collect(),
            focused: Some(self.focused().to_string()),
            ..RemoteLayoutMemory::default()
        }
    }
}

#[derive(Debug, Clone)]
enum Transition {
    Focus(usize),
    Switch(usize, &'static str),
    CloseTab(usize, &'static str),
    CloseSplit(usize),
}

impl Transition {
    fn apply(&self, editor: &Editor) -> Editor {
        let mut next = editor.clone();
        match *self {
            Transition::Focus(index) => next.focus = index,
            Transition::Switch(index, document) => {
                next.splits[index] = document;
                next.focus = index;
                if !next.open.contains(&document) {
                    next.open.push(document);
                }
            }
            Transition::CloseTab(index, document) => {
                let closed = next.splits[index];
                next.open.retain(|file| *file != closed);
                next.splits[index] = document;
                next.focus = index;
                if !next.open.contains(&document) {
                    next.open.push(document);
                }
            }
            Transition::CloseSplit(index) => {
                next.splits.remove(index);
                next.focus = index.min(next.splits.len() - 1);
            }
        }
        next
    }

    /// Every identifiable transition from `editor`.
    fn all(editor: &Editor) -> Vec<Transition> {
        let mut out = Vec::new();
        let width = editor.splits.len();
        for index in 0..width {
            if index != editor.focus {
                out.push(Transition::Focus(index));
            }
            for &document in DOCS.iter().filter(|doc| !editor.splits.contains(doc)) {
                out.push(Transition::Switch(index, document));
                out.push(Transition::CloseTab(index, document));
            }
            if width >= 3 {
                out.push(Transition::CloseSplit(index));
            }
        }
        out
    }
}

fn observation(visible: &[&str], selected: &str, open: &[&str]) -> RemoteLayoutEvidence {
    let owned = |files: &[&str]| files.iter().map(|file| (*file).to_string()).collect();
    RemoteLayoutEvidence {
        clients: vec![RemoteClientEditors {
            visible: owned(visible),
            selected: vec![selected.to_string()],
            open: owned(open),
        }],
        focused: vec![selected.to_string()],
    }
}

fn permutations(items: &[&'static str]) -> Vec<Vec<&'static str>> {
    if items.len() <= 1 {
        return vec![items.to_vec()];
    }
    let mut out = Vec::new();
    for index in 0..items.len() {
        let mut rest = items.to_vec();
        let head = rest.remove(index);
        for mut tail in permutations(&rest) {
            tail.insert(0, head);
            out.push(tail);
        }
    }
    out
}

/// The lagging observations Coder can deliver between `before` and `after`.
fn lagging(before: &Editor, after: &Editor) -> Vec<RemoteLayoutEvidence> {
    let selected = after.focused();
    let open: Vec<&str> = after
        .open
        .iter()
        .copied()
        .chain(before.open.iter().copied())
        .collect();
    let mut shapes: Vec<Vec<&'static str>> = Vec::new();
    // Selection-first (GH #250): old membership, new selection.
    shapes.push(before.splits.clone());
    // Stale hidden editor (GH #175): new membership plus a still-tracked
    // hidden editor of the document the changed split showed.
    for document in before
        .splits
        .iter()
        .filter(|doc| !after.splits.contains(doc))
    {
        let mut stale = after.splits.clone();
        stale.push(document);
        shapes.push(stale);
    }
    // Degenerate (GH #134 / GH #253): one or zero visible documents.
    shapes.push(vec![selected]);
    shapes.push(Vec::new());
    let mut out = Vec::new();
    for shape in shapes {
        let mut reversed = shape.clone();
        reversed.reverse();
        out.push(observation(&shape, selected, &open));
        if reversed != shape {
            out.push(observation(&reversed, selected, &open));
        }
    }
    out
}

fn visible_set(evidence: &RemoteLayoutEvidence) -> Vec<String> {
    let mut set = evidence.clients[0].visible.clone();
    set.sort();
    set
}

/// A stale hidden editor (GH #175) is transient. The same over-wide visible
/// set observed twice in a row has PERSISTED, which is exactly the evidence of
/// a real extra split; GH #253 deliberately resolves it as one. That run is
/// therefore not a lagging observation of this transition, and the model
/// excludes it (it is a different editor history, not a fault).
fn persists(
    previous: Option<&RemoteLayoutEvidence>,
    next: &RemoteLayoutEvidence,
    after: &Editor,
) -> bool {
    previous.is_some_and(|previous| {
        visible_set(previous) == visible_set(next) && visible_set(next).len() > after.splits.len()
    })
}

fn columns(memory: &RemoteLayoutMemory) -> Vec<String> {
    memory
        .columns
        .iter()
        .map(|column| column.files.join(","))
        .collect()
}

fn editors(max_width: usize) -> Vec<Editor> {
    fn ordered(width: usize, used: &mut Vec<&'static str>, out: &mut Vec<Vec<&'static str>>) {
        if used.len() == width {
            out.push(used.clone());
            return;
        }
        for &document in DOCS.iter() {
            if !used.contains(&document) {
                used.push(document);
                ordered(width, used, out);
                used.pop();
            }
        }
    }
    let mut out = Vec::new();
    for width in 2..=max_width {
        let mut layouts = Vec::new();
        ordered(width, &mut Vec::new(), &mut layouts);
        for splits in layouts {
            for focus in 0..width {
                out.push(Editor {
                    open: DOCS.to_vec(),
                    splits: splits.clone(),
                    focus,
                });
            }
        }
    }
    out
}

/// Run one transition through every lagging prefix (length <= `MAX_LAG`) and every
/// settled visible permutation; return the first divergence.
fn check_transition(editor: &Editor, transition: &Transition) -> Result<usize, String> {
    let after = transition.apply(editor);
    let lag = lagging(editor, &after);
    let mut prefixes: Vec<Vec<&RemoteLayoutEvidence>> = vec![Vec::new()];
    let mut frontier: Vec<Vec<&RemoteLayoutEvidence>> = vec![Vec::new()];
    let after_ref = &after;
    for _ in 0..MAX_LAG {
        frontier = frontier
            .iter()
            .flat_map(|prefix| {
                lag.iter()
                    .filter(move |next| !persists(prefix.last().copied(), next, after_ref))
                    .map(move |next| {
                        let mut longer = prefix.clone();
                        longer.push(next);
                        longer
                    })
            })
            .collect();
        prefixes.extend(frontier.iter().cloned());
    }
    let expected: Vec<String> = after
        .splits
        .iter()
        .map(|file| (*file).to_string())
        .collect();
    let mut checked = 0;
    // Two starting points satisfy the induction hypothesis: the canonical
    // memory, and a native reload (memory dropped) re-seeded from the order
    // authority — the layout tmux shows, which equals the editor by
    // induction.
    let reloaded = RemoteLayoutMemory::default().anchor(&editor.memory().columns);
    for (start, prefix) in prefixes
        .iter()
        .flat_map(|prefix| [(editor.memory(), prefix), (reloaded.clone(), prefix)])
    {
        let mut memory = start;
        for observation in prefix {
            memory = memory.advance(observation).0;
        }
        for settled in permutations(&after.splits) {
            let (next, resolution) =
                memory.advance(&observation(&settled, after.focused(), &after.open));
            checked += 1;
            // The induction hypothesis is the WHOLE memory, not only its
            // columns: after the settled observation the memory must be the
            // canonical memory of the new editor (no leftover provisional
            // base, focus on the focused split), so the next transition
            // starts from the same hypothesis.
            if columns(&next) != expected || next != after.memory() {
                return Err(format!(
                    "editor={editor:?} transition={transition:?} lagging={prefix:?} settled_visible={settled:?} \
                     -> memory={:?} resolution={resolution:?} expected={expected:?}",
                    columns(&next)
                ));
            }
        }
    }
    Ok(checked)
}

#[test]
fn memory_tracks_the_editor_across_every_identifiable_transition() {
    let mut checked = 0usize;
    let mut failures = Vec::new();
    for editor in editors(3) {
        for transition in Transition::all(&editor) {
            match check_transition(&editor, &transition) {
                Ok(count) => checked += count,
                Err(failure) => failures.push(failure),
            }
        }
    }
    assert!(
        failures.is_empty(),
        "{} transitions diverged (showing 12):\n{}",
        failures.len(),
        failures
            .iter()
            .take(12)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n")
    );
    assert!(checked > 100_000, "model coverage collapsed: {checked}");
}

/// A cold memory (native reload or first start with no authority yet) only
/// has tab-open order. Soundness: it never labels that order `Retained` —
/// not on the cold fold, and not on any later fold over it — until it is
/// anchored; anchoring to the authority restores the editor order exactly.
#[test]
fn a_cold_memory_never_claims_an_order_until_anchored() {
    use agent_doc_editor_surface::remote_layout::RemoteLayoutColumnOrder;
    let mut checked = 0usize;
    for editor in editors(3) {
        for visible in permutations(&editor.splits) {
            let cold = observation(&visible, editor.focused(), &editor.open);
            let (memory, resolution) = RemoteLayoutMemory::default().advance(&cold);
            assert_eq!(resolution.column_order, RemoteLayoutColumnOrder::Unknown);
            assert!(memory.unanchored, "a cold fold is unanchored: {editor:?}");
            for transition in Transition::all(&editor) {
                let after = transition.apply(&editor);
                for settled in permutations(&after.splits) {
                    let (next, resolution) =
                        memory.advance(&observation(&settled, after.focused(), &after.open));
                    assert_eq!(
                        resolution.column_order,
                        RemoteLayoutColumnOrder::Unknown,
                        "an unanchored fold must not upgrade tab-open order: \
                         editor={editor:?} cold_visible={visible:?} transition={transition:?}"
                    );
                    let anchored = next.anchor(&after.memory().columns);
                    assert_eq!(columns(&anchored), columns(&after.memory()));
                    assert!(!anchored.unanchored);
                    checked += 1;
                }
            }
        }
    }
    assert!(checked > 10_000, "model coverage collapsed: {checked}");
}

/// `anchor` is a slot-preserving permutation: membership and every slot that
/// holds a document the authority does not know stay put; the documents the
/// authority knows take its relative order; anchoring twice is anchoring once.
#[test]
fn anchoring_permutes_only_the_documents_the_authority_orders() {
    let mut checked = 0usize;
    for memory_editor in editors(3) {
        for authority_editor in editors(3) {
            let memory = RemoteLayoutMemory {
                unanchored: true,
                ..memory_editor.memory()
            };
            let authority = authority_editor.memory().columns;
            let anchored = memory.anchor(&authority);
            let before = columns(&memory);
            let after = columns(&anchored);
            let mut sorted_before = before.clone();
            let mut sorted_after = after.clone();
            sorted_before.sort();
            sorted_after.sort();
            assert_eq!(sorted_before, sorted_after, "membership is preserved");
            let known: Vec<String> = authority_editor
                .splits
                .iter()
                .map(|file| (*file).to_string())
                .collect();
            for (slot, file) in before.iter().enumerate() {
                if !known.contains(file) {
                    assert_eq!(&after[slot], file, "an unknown document keeps its slot");
                }
            }
            let shared: Vec<&String> = after.iter().filter(|file| known.contains(file)).collect();
            let authority_order: Vec<&String> =
                known.iter().filter(|file| after.contains(file)).collect();
            assert_eq!(
                shared, authority_order,
                "shared documents follow the authority"
            );
            assert_eq!(anchored.anchor(&authority), anchored, "idempotent");
            assert_eq!(
                anchored.unanchored,
                !before.iter().all(|file| known.contains(file)),
                "anchored iff the authority orders every document"
            );
            checked += 1;
        }
    }
    assert!(checked > 1_000);
}
