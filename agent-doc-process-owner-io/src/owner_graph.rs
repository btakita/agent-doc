//! Reactive pane -> document ownership (`#syncownerreactive`).
//!
//! Owner resolution used to *re-observe* on every question. `pane_runs_other_document_owner`
//! walked `/proc` twice per candidate pane, `find_*_owner_pane` asked it once per
//! candidate, and every caller in a sync pass asked again from scratch — which is
//! how a broken pane swap produced `sync_latency tmux_router elapsed_ms=45063`
//! against a 1000ms budget, with `gap_window_to_ownership` alone at 25.6s.
//!
//! The tmux-side answer to that was [`crate`-external] `observation_cache`: a memo
//! of raw argv output. A memo is not a derivation. Its only invalidation is
//! "recreate the context", because there are no edges to invalidate *along* — so
//! it cannot express "this pane changed, and only this pane's owner is stale".
//!
//! This module builds the graph the memo was standing in for:
//!
//! ```text
//!   Source<Rc<Vec<TreeProcess>>>   one per pane root pid   (the OBSERVATION)
//!            │
//!            ├── Computed<Option<String>>  other document owned by this pane
//!            ├── Computed<Option<String>>  document this pane's agent-doc owner binds
//!            └── Computed<bool>            pane runs an unmanaged harness session
//! ```
//!
//! A process tree is *observed*, never derived, so it is a `Source`. Classification
//! is a pure function of command lines and stays in `agent_doc_controller::command_line`
//! — nothing about the policy moves into the graph. What moves into the graph is the
//! **edge**: [`refresh_process_observations`] re-observes every pane already in the
//! scope in one `/proc` walk and writes each `Source`. Those writes are `PartialEq`-
//! guarded, so a pane whose process tree did not change invalidates nothing, and a
//! pane that *did* change invalidates exactly its own derived cells. Callers stop
//! recomputing; the map recomputes when a pane changes.
//!
//! The scope is [`LocalReadScope`] (`#stategraphjoin`): a bounded read scope, valid
//! until the observed system is mutated rather than until the turn ends. It is
//! opt-in and reference-counted like the tmux observation scope, and thread-local
//! because `lazily::Context` is `Rc`-based and therefore `!Send`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::Path;
use std::rc::Rc;

use agent_doc_controller::command_line::{
    agent_doc_owner_document_from_cmdline, cmdline_owns_other_document,
    cmdlines_are_unmanaged_harness_session, owner_document_from_cmdline,
};
use agent_doc_state_scope::LocalReadScope;
use lazily::{Computed, Source, SourceMap};

use crate::proc_table::{
    TreeProcess, observe_proc_children, observe_process_tree, tree_cmdlines,
};

/// A pane's observed process tree, shared by every derived cell that reads it.
type TreeObservation = Rc<Vec<TreeProcess>>;

struct ProcessObservationState {
    /// The scope these cells live in. Dropping it drops the whole graph, which is
    /// how the scope guard's `Drop` invalidates every observation at once.
    scope: LocalReadScope,
    /// Pane root pid -> observed process tree. `Source`, because a process tree is
    /// observed from outside the graph.
    trees: SourceMap<String, TreeObservation>,
    /// (root pid, claimed document) -> the *other* document this pane owns, if any.
    other_document: HashMap<(String, String), Computed<Option<String>>>,
    /// Root pid -> the document this pane's first agent-doc owner process binds.
    owner_document: HashMap<String, Computed<Option<String>>>,
    /// Root pid -> whether the pane runs a harness session agent-doc did not start.
    unmanaged_harness: HashMap<String, Computed<bool>>,
    depth: usize,
    observations: u64,
    derived_reads: u64,
}

impl ProcessObservationState {
    fn new() -> Self {
        let scope = LocalReadScope::new();
        let trees = SourceMap::new(scope.ctx());
        Self {
            scope,
            trees,
            other_document: HashMap::new(),
            owner_document: HashMap::new(),
            unmanaged_harness: HashMap::new(),
            depth: 1,
            observations: 0,
            derived_reads: 0,
        }
    }

    /// The `Source` holding `root_pid`'s tree, observing it on first request.
    fn tree_source(&mut self, root_pid: &str) -> Source<TreeObservation> {
        if let Some(handle) = self.trees.handle(&root_pid.to_string()) {
            return handle;
        }
        self.observations += 1;
        let observed: TreeObservation = Rc::new(observe_process_tree(
            &observe_proc_children(),
            root_pid,
        ));
        self.trees
            .entry_with(self.scope.ctx(), root_pid.to_string(), || observed)
    }

    fn tree(&mut self, root_pid: &str) -> TreeObservation {
        let source = self.tree_source(root_pid);
        source.get(self.scope.ctx())
    }

    /// The *other* document this pane owns, derived from its tree observation.
    fn derive_other_document(&mut self, root_pid: &str, claimed: &str) -> Option<String> {
        let key = (root_pid.to_string(), claimed.to_string());
        if let Some(cell) = self.other_document.get(&key).copied() {
            self.derived_reads += 1;
            return cell.get(self.scope.ctx());
        }
        let source = self.tree_source(root_pid);
        let claimed = claimed.to_string();
        let cell = self.scope.ctx().computed(move |c| {
            let tree = source.get(c);
            classify_other_document(&tree, &claimed)
        });
        self.other_document.insert(key, cell);
        cell.get(self.scope.ctx())
    }

    /// The document this pane's first agent-doc owner process binds.
    fn derive_owner_document(&mut self, root_pid: &str) -> Option<String> {
        if let Some(cell) = self.owner_document.get(root_pid).copied() {
            self.derived_reads += 1;
            return cell.get(self.scope.ctx());
        }
        let source = self.tree_source(root_pid);
        let cell = self.scope.ctx().computed(move |c| {
            let tree = source.get(c);
            classify_agent_doc_owner_document(&tree)
        });
        self.owner_document.insert(root_pid.to_string(), cell);
        cell.get(self.scope.ctx())
    }

    /// Whether this pane runs a harness session agent-doc did not start.
    fn derive_unmanaged_harness(&mut self, root_pid: &str) -> bool {
        if let Some(cell) = self.unmanaged_harness.get(root_pid).copied() {
            self.derived_reads += 1;
            return cell.get(self.scope.ctx());
        }
        let source = self.tree_source(root_pid);
        let cell = self.scope.ctx().computed(move |c| {
            let tree = source.get(c);
            classify_unmanaged_harness_session(&tree)
        });
        self.unmanaged_harness.insert(root_pid.to_string(), cell);
        cell.get(self.scope.ctx())
    }

    /// Write an already-observed tree for `root_pid`. The write is
    /// `PartialEq`-guarded, so recording an unchanged observation invalidates
    /// nothing.
    fn record_tree(&mut self, root_pid: &str, tree: Vec<TreeProcess>) {
        self.observations += 1;
        let ctx = self.scope.ctx();
        self.trees.set(ctx, root_pid.to_string(), Rc::new(tree));
    }

    /// Re-observe every pane already in this scope from one `/proc` walk. Writes
    /// are `PartialEq`-guarded, so only panes whose process tree actually changed
    /// invalidate their derived cells.
    fn refresh(&mut self) {
        let ctx = self.scope.ctx();
        let roots = self.trees.keys(ctx);
        if roots.is_empty() {
            return;
        }
        let children = observe_proc_children();
        for root in roots {
            self.observations += 1;
            let observed: TreeObservation = Rc::new(observe_process_tree(&children, &root));
            self.trees.set(ctx, root, observed);
        }
    }
}

thread_local! {
    static SCOPE: RefCell<Option<ProcessObservationState>> = const { RefCell::new(None) };
}

/// Statistics for the active process-observation scope.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ProcessObservationStats {
    /// `/proc` tree observations performed (first sighting plus refreshes).
    pub observations: u64,
    /// Reads served from a derived cell instead of re-observing.
    pub derived_reads: u64,
}

/// RAII guard for an active process-observation scope. Ownership questions asked
/// on this thread while the guard is alive derive from shared observations;
/// dropping it drops the graph so a later pass always re-observes.
#[must_use = "process observations are derived only while the guard is alive"]
pub struct ProcessObservationScope {
    _not_send: std::marker::PhantomData<*const ()>,
}

/// Open a process-observation scope on this thread. Nested calls are
/// reference-counted: an inner scope does not drop the outer scope's graph.
pub fn begin_process_observation_scope() -> ProcessObservationScope {
    SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        match scope.as_mut() {
            Some(state) => state.depth += 1,
            None => *scope = Some(ProcessObservationState::new()),
        }
    });
    ProcessObservationScope {
        _not_send: std::marker::PhantomData,
    }
}

impl Drop for ProcessObservationScope {
    fn drop(&mut self) {
        SCOPE.with(|scope| {
            let mut scope = scope.borrow_mut();
            let finished = match scope.as_mut() {
                Some(state) => {
                    state.depth -= 1;
                    state.depth == 0
                }
                None => false,
            };
            if finished {
                *scope = None;
            }
        });
    }
}

/// Statistics for the currently active scope, if any.
pub fn process_observation_scope_stats() -> Option<ProcessObservationStats> {
    SCOPE.with(|scope| {
        scope
            .borrow()
            .as_ref()
            .map(|state| ProcessObservationStats {
                observations: state.observations,
                derived_reads: state.derived_reads,
            })
    })
}

/// Re-observe every pane in the active scope. This is the invalidation **edge**:
/// a pane whose process tree changed invalidates its own derived cells and
/// nothing else. A no-op without an active scope, and a no-op for panes whose
/// observation is byte-identical.
///
/// Called after a tmux mutation, which is the event that can create, replace, or
/// kill the process a pane's ownership is decided from.
pub fn refresh_process_observations() {
    SCOPE.with(|scope| {
        if let Some(state) = scope.borrow_mut().as_mut() {
            state.refresh();
        }
    });
}

/// Feed an already-observed process tree into the active scope, bypassing the
/// `/proc` walk. This is the observation seam: production observes from `/proc`,
/// a caller that already holds a tree writes it here, and the derived owner map
/// is the same graph either way. A no-op without an active scope, and — because
/// the write is `PartialEq`-guarded — a no-op for an unchanged observation.
pub fn record_tree_observation(root_pid: &str, tree: Vec<TreeProcess>) {
    SCOPE.with(|scope| {
        if let Some(state) = scope.borrow_mut().as_mut() {
            state.record_tree(root_pid, tree);
        }
    });
}

/// Read a pane's observed process tree, from the active scope when there is one.
pub(crate) fn with_tree_observation<R>(root_pid: &str, read: impl FnOnce(&[TreeProcess]) -> R) -> R {
    let scoped = SCOPE.with(|scope| {
        scope
            .borrow_mut()
            .as_mut()
            .map(|state| state.tree(root_pid))
    });
    match scoped {
        Some(tree) => read(&tree),
        None => read(&crate::proc_table::observe_process_tree_uncached(root_pid)),
    }
}

/// The *other* document owned by the pane rooted at `root_pid`, if any.
pub fn tree_owner_document_other_than(root_pid: &str, claimed_file: &Path) -> Option<String> {
    let claimed = claimed_file.to_string_lossy();
    SCOPE
        .with(|scope| {
            scope
                .borrow_mut()
                .as_mut()
                .map(|state| state.derive_other_document(root_pid, &claimed))
        })
        .unwrap_or_else(|| {
            classify_other_document(
                &crate::proc_table::observe_process_tree_uncached(root_pid),
                &claimed,
            )
        })
}

/// The document bound by the first agent-doc owner process in the pane's tree.
pub fn tree_agent_doc_owner_document(root_pid: &str) -> Option<String> {
    SCOPE
        .with(|scope| {
            scope
                .borrow_mut()
                .as_mut()
                .map(|state| state.derive_owner_document(root_pid))
        })
        .unwrap_or_else(|| {
            classify_agent_doc_owner_document(&crate::proc_table::observe_process_tree_uncached(
                root_pid,
            ))
        })
}

/// Whether the pane rooted at `root_pid` runs a harness session agent-doc did not
/// start (the bare-foreign-session guard).
pub fn tree_runs_unmanaged_harness_session(root_pid: &str) -> bool {
    SCOPE
        .with(|scope| {
            scope
                .borrow_mut()
                .as_mut()
                .map(|state| state.derive_unmanaged_harness(root_pid))
        })
        .unwrap_or_else(|| {
            classify_unmanaged_harness_session(&crate::proc_table::observe_process_tree_uncached(
                root_pid,
            ))
        })
}

// -- Pure classification over an observation --------------------------------
//
// Policy stays in `agent_doc_controller::command_line`; these are the total
// functions that lift it from one command line to one observed tree.

fn classify_other_document(tree: &[TreeProcess], claimed: &str) -> Option<String> {
    tree_cmdlines(tree).find_map(|cmdline| {
        cmdline_owns_other_document(cmdline, claimed)
            .then(|| owner_document_from_cmdline(cmdline))?
    })
}

fn classify_agent_doc_owner_document(tree: &[TreeProcess]) -> Option<String> {
    tree_cmdlines(tree).find_map(agent_doc_owner_document_from_cmdline)
}

fn classify_unmanaged_harness_session(tree: &[TreeProcess]) -> bool {
    cmdlines_are_unmanaged_harness_session(tree_cmdlines(tree))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree(entries: &[(&str, &str)]) -> Vec<TreeProcess> {
        entries
            .iter()
            .map(|(pid, cmdline)| TreeProcess {
                pid: (*pid).to_string(),
                cmdline: Some((*cmdline).to_string()),
            })
            .collect()
    }

    #[test]
    fn other_document_classification_is_a_pure_function_of_the_observation() {
        let observed = tree(&[("10", "zsh"), ("20", "codex agent-doc tasks/foreign.md")]);
        assert_eq!(
            classify_other_document(&observed, "tasks/claimed.md"),
            Some("tasks/foreign.md".to_string())
        );
        assert_eq!(
            classify_other_document(&observed, "tasks/foreign.md"),
            None,
            "a pane that owns the claimed document owns no OTHER document"
        );
    }

    #[test]
    fn agent_doc_owner_document_prefers_the_route_owned_wrapper() {
        let observed = tree(&[
            ("10", "agent-doc start --route-owned /repo/tasks/selected.md"),
            ("20", "codex resume --last"),
        ]);
        assert_eq!(
            classify_agent_doc_owner_document(&observed).as_deref(),
            Some("/repo/tasks/selected.md")
        );
    }

    #[test]
    fn unmanaged_harness_classification_reads_the_whole_tree() {
        assert!(
            classify_unmanaged_harness_session(&tree(&[("10", "-zsh"), ("20", "claude")])),
            "an operator-started harness with no agent-doc above it is unmanaged"
        );
        assert!(
            !classify_unmanaged_harness_session(&tree(&[
                ("10", "agent-doc start --route-owned /repo/tasks/a.md"),
                ("20", "claude"),
            ])),
            "agent-doc's own pane is managed (#panehijackself)"
        );
    }

    #[test]
    fn a_scope_derives_repeat_questions_instead_of_re_observing() {
        let _scope = begin_process_observation_scope();
        let root = std::process::id().to_string();
        for _ in 0..5 {
            let _ = tree_runs_unmanaged_harness_session(&root);
            let _ = tree_agent_doc_owner_document(&root);
            let _ = tree_owner_document_other_than(&root, Path::new("/repo/tasks/claimed.md"));
        }
        let stats = process_observation_scope_stats().expect("scope is open");
        assert_eq!(
            stats.observations, 1,
            "one pane, one process-tree observation — the rest derive"
        );
        assert_eq!(
            stats.derived_reads, 12,
            "3 cells minted on first use, then 4 further reads each served from the graph"
        );
    }

    #[test]
    fn without_a_scope_every_question_re_observes() {
        let root = std::process::id().to_string();
        let _ = tree_runs_unmanaged_harness_session(&root);
        assert!(
            process_observation_scope_stats().is_none(),
            "deriving must be opt-in so polling paths are unaffected"
        );
    }

    #[test]
    fn refresh_without_a_scope_is_a_no_op() {
        refresh_process_observations();
        assert!(process_observation_scope_stats().is_none());
    }

    #[test]
    fn a_refresh_re_observes_every_pane_in_the_scope() {
        let _scope = begin_process_observation_scope();
        let root = std::process::id().to_string();
        let before = tree_runs_unmanaged_harness_session(&root);
        refresh_process_observations();
        let after = tree_runs_unmanaged_harness_session(&root);
        assert_eq!(before, after, "an unchanged tree derives the same answer");
        let stats = process_observation_scope_stats().expect("scope is open");
        assert_eq!(
            stats.observations, 2,
            "the refresh re-observes the one pane already in the scope"
        );
    }

    #[test]
    fn a_changed_pane_observation_re_derives_that_pane_and_only_that_pane() {
        let _scope = begin_process_observation_scope();
        let claimed = Path::new("/repo/tasks/claimed.md");

        // Two panes: one the operator started by hand, one agent-doc owns.
        record_tree_observation("100", tree(&[("100", "-zsh"), ("101", "claude")]));
        record_tree_observation(
            "200",
            tree(&[
                ("200", "agent-doc start --route-owned /repo/tasks/other.md"),
                ("201", "claude"),
            ]),
        );

        assert!(tree_runs_unmanaged_harness_session("100"));
        assert_eq!(
            tree_owner_document_other_than("200", claimed).as_deref(),
            Some("/repo/tasks/other.md")
        );

        // Pane 100 changes: the operator's session is replaced by an agent-doc
        // one. A memo keyed on the pane would still answer "unmanaged" here —
        // its only invalidation is dropping the whole scope. The derived cell
        // recomputes because its observation is an edge.
        record_tree_observation(
            "100",
            tree(&[
                ("100", "agent-doc start --route-owned /repo/tasks/claimed.md"),
                ("102", "claude"),
            ]),
        );

        assert!(
            !tree_runs_unmanaged_harness_session("100"),
            "a changed pane observation must re-derive its owner cells"
        );
        assert_eq!(
            tree_owner_document_other_than("200", claimed).as_deref(),
            Some("/repo/tasks/other.md"),
            "an untouched pane keeps its derived answer"
        );
    }

    #[test]
    fn re_recording_an_identical_observation_keeps_the_same_answer() {
        let _scope = begin_process_observation_scope();
        let observed = tree(&[("100", "-zsh"), ("101", "claude")]);
        record_tree_observation("100", observed.clone());
        assert!(tree_runs_unmanaged_harness_session("100"));
        record_tree_observation("100", observed);
        assert!(
            tree_runs_unmanaged_harness_session("100"),
            "an unchanged observation is equality-guarded and changes nothing"
        );
    }

    #[test]
    fn nested_scopes_do_not_drop_the_outer_graph() {
        let outer = begin_process_observation_scope();
        let root = std::process::id().to_string();
        let _ = tree_runs_unmanaged_harness_session(&root);
        {
            let _inner = begin_process_observation_scope();
            let _ = tree_runs_unmanaged_harness_session(&root);
        }
        let _ = tree_runs_unmanaged_harness_session(&root);
        let stats = process_observation_scope_stats().expect("outer scope is still open");
        assert_eq!(
            stats.observations, 1,
            "an inner scope drop must not discard the outer observation"
        );
        drop(outer);
        assert!(process_observation_scope_stats().is_none());
    }

    #[test]
    fn a_dropped_scope_does_not_serve_a_later_pass() {
        let root = std::process::id().to_string();
        {
            let _scope = begin_process_observation_scope();
            let _ = tree_runs_unmanaged_harness_session(&root);
        }
        let _scope = begin_process_observation_scope();
        let _ = tree_runs_unmanaged_harness_session(&root);
        let stats = process_observation_scope_stats().expect("scope is open");
        assert_eq!(stats.observations, 1, "the new scope observes afresh");
    }
}
