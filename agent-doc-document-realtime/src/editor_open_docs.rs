//! Reactive editor open-document registry (`#live-editor-reactive`).
//!
//! The set of files currently open in an editor — and the agent-doc subset of it —
//! exposed as a reactive signal on the lazily thread-safe backbone. This is the
//! process-local advisory input for the authority question *"could the editor
//! buffer differ from disk?"*. Cross-process authority lives in the durable
//! reliable-sync liveness projection.
//!
//! This registry never overrides the CRDT/reliable-sync plane. It exists for local
//! UI observation and is rebuilt from reliable editor events after a recycle.
//!
//! ## Source-agnostic driving
//!
//! Editor open/close reports arrive cross-process on the reliable-sync Lazily
//! plane, so the registry is driven through a source-agnostic API:
//! - [`EditorOpenDocs::mark_open`] / [`EditorOpenDocs::mark_closed`] — event-at-a-time
//!   updates, used by the in-process FFI editor-report path.
//! - [`EditorOpenDocs::reconcile`] — a whole-set refresh, used by a process (e.g. the
//!   project controller) that observes the controller-owned open set.
//!
//! Classification (`is_agent_doc`) is supplied by the caller so this module stays a
//! pure reactive model with no frontmatter/IO dependency.

use std::collections::HashSet;
use std::sync::OnceLock;

use lazily::{CellHandle, SlotHandle, ThreadSafeCellMap, ThreadSafeContext};

/// Per-document open state held in the reactive family.
///
/// `is_agent_doc` is the (stable) classification of the path — whether it is an
/// agent-doc session document; `open` flips as the editor opens or closes the buffer.
/// Both fields participate in the derived counts, so a classification change on an
/// already-open document recomputes the agent-doc subset reactively.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DocOpenState {
    /// Whether the document currently has an open buffer in some editor.
    pub open: bool,
    /// Whether the document is an agent-doc session document.
    pub is_agent_doc: bool,
}

impl DocOpenState {
    const CLOSED_UNKNOWN: Self = DocOpenState {
        open: false,
        is_agent_doc: false,
    };
}

/// Reactive registry of editor-open documents.
///
/// Cheap to construct; holds a private [`ThreadSafeContext`]. All handles stored here
/// are `Send + Sync`, so a single instance can back a process-global registry (see
/// [`editor_open_docs`]).
pub struct EditorOpenDocs {
    ctx: ThreadSafeContext,
    /// `path -> DocOpenState`. Deferral-not-dealloc: a closed document's key stays
    /// present-but-closed (bounded per session), never counted as open.
    docs: ThreadSafeCellMap<String, DocOpenState>,
    /// Bumped when a brand-new key is materialized so the derived counts observe it
    /// (a not-yet-observed cell is not yet a dependency; the epoch is).
    epoch: CellHandle<u64>,
    /// Reactive count of currently-open documents.
    open_count: SlotHandle<usize>,
    /// Reactive count of currently-open agent-doc session documents.
    open_agent_doc_count: SlotHandle<usize>,
}

impl EditorOpenDocs {
    /// Build an empty registry.
    pub fn new() -> Self {
        let ctx = ThreadSafeContext::new();
        let epoch = ctx.cell(0u64);
        let docs: ThreadSafeCellMap<String, DocOpenState> = ThreadSafeCellMap::new(&ctx);
        let open_count = {
            let docs = docs.clone();
            ctx.computed(move |ctx| {
                // Depend on the membership epoch so a newly-present key forces a
                // recompute that then picks it up in `present_keys`.
                let _ = ctx.get_cell(&epoch);
                docs.present_keys()
                    .into_iter()
                    .filter(|key| {
                        docs.observe(ctx, key)
                            .unwrap_or(DocOpenState::CLOSED_UNKNOWN)
                            .open
                    })
                    .count()
            })
        };
        let open_agent_doc_count = {
            let docs = docs.clone();
            ctx.computed(move |ctx| {
                let _ = ctx.get_cell(&epoch);
                docs.present_keys()
                    .into_iter()
                    .filter(|key| {
                        let state = docs
                            .observe(ctx, key)
                            .unwrap_or(DocOpenState::CLOSED_UNKNOWN);
                        state.open && state.is_agent_doc
                    })
                    .count()
            })
        };
        Self {
            ctx,
            docs,
            epoch,
            open_count,
            open_agent_doc_count,
        }
    }

    /// Materialize (if needed) and set a document's open state. Never holds the family
    /// lock across the `ctx` write (the family releases its lock before touching
    /// `ctx`), so there is no lock-order cycle with any outer registry lock.
    fn set_state(&self, path: &str, state: DocOpenState) {
        self.docs.set(&self.ctx, path.to_string(), state);
    }

    fn bump_epoch(&self) {
        let epoch = self.ctx.get_cell(&self.epoch);
        self.ctx.set_cell(&self.epoch, epoch.wrapping_add(1));
    }

    /// Record that `path` is open in an editor, with its agent-doc classification.
    /// Idempotent: re-marking an already-open document just refreshes its state.
    pub fn mark_open(&self, path: &str, is_agent_doc: bool) {
        let newly_present = !self.docs.is_present(&path.to_string());
        self.set_state(
            path,
            DocOpenState {
                open: true,
                is_agent_doc,
            },
        );
        if newly_present {
            self.bump_epoch();
        }
    }

    /// Like [`mark_open`](Self::mark_open) but classification is computed lazily via
    /// `classify`, and **only** the first time `path` is seen. On reopen of a
    /// previously-known path the stored classification is reused, so a hot-path caller
    /// (an editor buffer report on every keystroke) never re-runs an expensive
    /// classifier (e.g. a frontmatter read) for an already-known document.
    pub fn mark_open_with(&self, path: &str, classify: impl FnOnce() -> bool) {
        let key = path.to_string();
        if self.docs.is_present(&key) {
            let is_agent_doc = self
                .docs
                .observe(&self.ctx, &key)
                .unwrap_or(DocOpenState::CLOSED_UNKNOWN)
                .is_agent_doc;
            self.set_state(
                path,
                DocOpenState {
                    open: true,
                    is_agent_doc,
                },
            );
        } else {
            let is_agent_doc = classify();
            self.set_state(
                path,
                DocOpenState {
                    open: true,
                    is_agent_doc,
                },
            );
            self.bump_epoch();
        }
    }

    /// Record that `path` is no longer open. The classification is preserved (a closed
    /// document is still known to be an agent-doc or not); only `open` flips to false.
    /// A close for a never-seen path records a present-but-closed entry (uncounted).
    pub fn mark_closed(&self, path: &str) {
        let key = path.to_string();
        let newly_present = !self.docs.is_present(&key);
        let is_agent_doc = if newly_present {
            false
        } else {
            self.docs
                .observe(&self.ctx, &key)
                .unwrap_or(DocOpenState::CLOSED_UNKNOWN)
                .is_agent_doc
        };
        self.set_state(
            path,
            DocOpenState {
                open: false,
                is_agent_doc,
            },
        );
        if newly_present {
            self.bump_epoch();
        }
    }

    /// Reconcile the registry against the **full** current open set (path,
    /// is_agent_doc). Any document currently marked open but absent from `open` is
    /// closed; every entry in `open` is marked open. This is the whole-set refresh a
    /// cross-process reliable-sync observer uses.
    pub fn reconcile(&self, open: &[(String, bool)]) {
        let open_paths: HashSet<&str> = open.iter().map(|(path, _)| path.as_str()).collect();
        for key in self.docs.present_keys() {
            let still_open = open_paths.contains(key.as_str());
            if !still_open
                && self
                    .docs
                    .observe(&self.ctx, &key)
                    .unwrap_or(DocOpenState::CLOSED_UNKNOWN)
                    .open
            {
                self.mark_closed(&key);
            }
        }
        for (path, is_agent_doc) in open {
            self.mark_open(path, *is_agent_doc);
        }
    }

    /// Whether `path` currently has an open editor buffer (reactive read). Does not
    /// materialize an entry for a never-seen path.
    pub fn is_open(&self, path: &str) -> bool {
        let key = path.to_string();
        self.docs.is_present(&key)
            && self
                .docs
                .observe(&self.ctx, &key)
                .unwrap_or(DocOpenState::CLOSED_UNKNOWN)
                .open
    }

    /// Whether `path` has ever been recorded (open OR closed) in this registry. Lets a
    /// consumer distinguish a **known-closed** document from a **never-seen** one so it
    /// can fall back to a durable backup source *only on the never-seen (cold-miss) case*
    /// — e.g. right after a process recycle, before any open/close event has re-seeded the
    /// in-memory reactive authority — instead of consulting the backup on every read. Does
    /// not materialize an entry for a never-seen path.
    pub fn is_tracked(&self, path: &str) -> bool {
        self.docs.is_present(&path.to_string())
    }

    /// The number of documents currently open in an editor — a reactive read of the
    /// derived `open_count` slot.
    pub fn open_count(&self) -> usize {
        self.ctx.get(&self.open_count)
    }

    /// The number of **agent-doc session documents** currently open in an editor — a
    /// reactive read of the derived `open_agent_doc_count` slot.
    pub fn open_agent_doc_count(&self) -> usize {
        self.ctx.get(&self.open_agent_doc_count)
    }

    /// The currently-open document paths (unordered).
    pub fn open_paths(&self) -> Vec<String> {
        self.docs
            .present_keys()
            .into_iter()
            .filter(|key| {
                self.docs
                    .observe(&self.ctx, key)
                    .unwrap_or(DocOpenState::CLOSED_UNKNOWN)
                    .open
            })
            .collect()
    }

    /// The currently-open agent-doc session document paths (unordered).
    pub fn open_agent_docs(&self) -> Vec<String> {
        self.docs
            .present_keys()
            .into_iter()
            .filter(|key| {
                let state = self
                    .docs
                    .observe(&self.ctx, key)
                    .unwrap_or(DocOpenState::CLOSED_UNKNOWN);
                state.open && state.is_agent_doc
            })
            .collect()
    }
}

impl Default for EditorOpenDocs {
    fn default() -> Self {
        Self::new()
    }
}

/// The process-global editor open-document registry. In-process editor reports (FFI)
/// and any controller-side reliable-sync reconcile drive this single instance.
pub fn editor_open_docs() -> &'static EditorOpenDocs {
    static GLOBAL: OnceLock<EditorOpenDocs> = OnceLock::new();
    GLOBAL.get_or_init(EditorOpenDocs::new)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// One scripted editor event in the deterministic SimWorld model.
    #[derive(Clone, Copy, Debug)]
    enum Ev<'a> {
        Open(&'a str, bool),
        Close(&'a str),
    }

    /// Shared pure decision function: fold one event into a reference model of
    /// `path -> (open, is_agent_doc)`.
    fn apply_to_model(model: &mut BTreeMap<String, (bool, bool)>, ev: Ev) {
        match ev {
            Ev::Open(path, is_agent_doc) => {
                model.insert(path.to_string(), (true, is_agent_doc));
            }
            Ev::Close(path) => {
                let is_agent_doc = model.get(path).map(|(_, a)| *a).unwrap_or(false);
                model.insert(path.to_string(), (false, is_agent_doc));
            }
        }
    }

    fn apply_to_registry(reg: &EditorOpenDocs, ev: Ev) {
        match ev {
            Ev::Open(path, is_agent_doc) => reg.mark_open(path, is_agent_doc),
            Ev::Close(path) => reg.mark_closed(path),
        }
    }

    fn model_open_count(model: &BTreeMap<String, (bool, bool)>) -> usize {
        model.values().filter(|(open, _)| *open).count()
    }

    fn model_open_agent_doc_count(model: &BTreeMap<String, (bool, bool)>) -> usize {
        model
            .values()
            .filter(|(open, is_agent_doc)| *open && *is_agent_doc)
            .count()
    }

    #[test]
    fn reactive_open_counts_match_reference_model_across_events() {
        use Ev::*;
        let script = [
            Open("plan.md", true),
            Open("src/lib.rs", false),
            Open("notes.md", false),
            Close("src/lib.rs"),
            Open("tasks/a.md", true),
            Close("plan.md"),
            Open("src/lib.rs", false), // reopen a previously-closed file
            Open("plan.md", true),     // reopen a previously-closed agent doc
        ];

        let reg = EditorOpenDocs::new();
        let mut model: BTreeMap<String, (bool, bool)> = BTreeMap::new();
        assert_eq!(reg.open_count(), 0);
        assert_eq!(reg.open_agent_doc_count(), 0);

        for (step, ev) in script.into_iter().enumerate() {
            apply_to_registry(&reg, ev);
            apply_to_model(&mut model, ev);
            assert_eq!(
                reg.open_count(),
                model_open_count(&model),
                "open_count diverged at step {step} after {ev:?}",
            );
            assert_eq!(
                reg.open_agent_doc_count(),
                model_open_agent_doc_count(&model),
                "open_agent_doc_count diverged at step {step} after {ev:?}",
            );
        }
    }

    #[test]
    fn open_agent_docs_reports_only_open_session_documents() {
        let reg = EditorOpenDocs::new();
        reg.mark_open("plan.md", true);
        reg.mark_open("src/main.rs", false);
        reg.mark_open("tasks/x.md", true);
        reg.mark_closed("tasks/x.md");

        let mut open_docs = reg.open_agent_docs();
        open_docs.sort();
        assert_eq!(open_docs, vec!["plan.md".to_string()]);
        assert_eq!(reg.open_agent_doc_count(), 1);
        assert_eq!(
            reg.open_count(),
            2,
            "the non-agent-doc source file is still open"
        );
    }

    #[test]
    fn is_open_does_not_materialize_unknown_paths() {
        let reg = EditorOpenDocs::new();
        assert!(!reg.is_open("never-seen.md"));
        // A pure query must not inflate the derived count by materializing a key.
        assert_eq!(reg.open_count(), 0);

        reg.mark_open("plan.md", true);
        assert!(reg.is_open("plan.md"));
        reg.mark_closed("plan.md");
        assert!(!reg.is_open("plan.md"));
    }

    #[test]
    fn is_tracked_distinguishes_never_seen_from_known_closed() {
        let reg = EditorOpenDocs::new();
        // Never seen → not tracked (cold miss). A pure query must not materialize it.
        assert!(!reg.is_tracked("never-seen.md"));
        assert_eq!(reg.open_count(), 0);

        // A close records a present-but-closed entry: known-closed IS tracked, so a
        // consumer will read the reactive authority (closed) and NOT re-consult a backup.
        reg.mark_closed("closed.md");
        assert!(reg.is_tracked("closed.md"));
        assert!(!reg.is_open("closed.md"));

        // An open is likewise tracked.
        reg.mark_open("open.md", true);
        assert!(reg.is_tracked("open.md"));
        assert!(reg.is_open("open.md"));
    }

    #[test]
    fn reconcile_closes_dropped_and_opens_new() {
        let reg = EditorOpenDocs::new();
        reg.mark_open("a.md", true);
        reg.mark_open("b.rs", false);
        reg.mark_open("c.md", true);
        assert_eq!(reg.open_count(), 3);

        // The editor now reports only b.rs and a brand-new d.md open.
        reg.reconcile(&[("b.rs".to_string(), false), ("d.md".to_string(), true)]);

        let mut open = reg.open_paths();
        open.sort();
        assert_eq!(open, vec!["b.rs".to_string(), "d.md".to_string()]);
        assert_eq!(reg.open_count(), 2);
        assert_eq!(
            reg.open_agent_doc_count(),
            1,
            "only d.md is an open agent doc"
        );
        assert!(!reg.is_open("a.md"), "a.md was dropped from the open set");
        assert!(!reg.is_open("c.md"), "c.md was dropped from the open set");
    }
}
