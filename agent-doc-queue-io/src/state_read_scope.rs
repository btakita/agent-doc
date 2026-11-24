//! `#claudestopbudget`: one read snapshot of a document's queue projections
//! for a bounded gate pass.
//!
//! The Claude Stop hook's cycle gates (queue continuation, claimed heads,
//! steering, reported writeback, transient closeout) each derive the same
//! document-wide projections from the same text: the subagent ownership plan,
//! the claim-ledger holds, the reported-release holds, and the out-of-loop
//! ownership set. Every derivation re-read and re-decoded the claim, receipt,
//! and lineage ledgers and re-planned every queue block. On haiven-dev
//! `tasks/sdk/sdk-dev.md` (265 KB, 61 live rows across 30 run queues, a 1.8 MB
//! lineage ledger and a 728 KB receipt ledger) one plan cost ~1 s and the gate
//! pass ran it more than four times, so `claude_stop_cycle_gates` took 9.6 s of
//! the hook's 11 s budget and the transient-closeout assessment never finished.
//!
//! Inside [`with_state_read_scope`] each memoized projection is computed once
//! per (document, text, ambient read context, state write epoch) and served
//! from the scope afterwards:
//!
//! - **Text.** The entry holds the exact text it was derived from and is
//!   served only for an identical text, so an edited document is a miss.
//! - **Ambient read context.** The nested-claim steer snapshots and the
//!   inherited goal presets installed on this thread change what the same
//!   text derives to; both are part of the key. A planned claim overlay
//!   (`PlannedClaimLedgerOverlay`) changes every ledger read, so no memo is
//!   used while one is installed.
//! - **State write epoch.** Every `queue_document_state` write on this thread
//!   bumps [`agent_doc_sqlite::state_store::queue_document_state_write_epoch`].
//!   An entry is recorded only when the epoch did not move while it was
//!   computed, and it is served only under that same epoch, so a projection
//!   never survives a write the pass itself made (a lost-worker recovery, a
//!   continuation request). The Stop hook's own refusal record is bookkeeping
//!   no projection reads and does not bump the epoch.
//!
//! What the scope deliberately does NOT re-observe is a write another process
//! or thread makes while the pass runs, or a claim expiring mid-pass. The pass reads one
//! snapshot of the ledgers, which is no weaker than before: its gates already
//! read the ledgers at different instants, so they could disagree with each
//! other; now they agree. Outside a scope every function computes as before.

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct Key {
    tag: &'static str,
    file: PathBuf,
    context: String,
    epoch: u64,
}

/// One recorded projection: the exact text it was derived from and its value.
type Entry = (Arc<str>, Rc<dyn Any>);

#[derive(Default)]
struct ScopeMemo {
    entries: HashMap<Key, Vec<Entry>>,
}

thread_local! {
    static SCOPE: RefCell<Option<ScopeMemo>> = const { RefCell::new(None) };
    static EVALUATIONS: RefCell<HashMap<&'static str, u64>> = RefCell::new(HashMap::new());
    static DEPTH: Cell<u32> = const { Cell::new(0) };
}

/// Run `f` with one read snapshot of queue projections (see the module docs).
/// A nested call joins the enclosing scope; the snapshot ends when the
/// outermost call returns, panics included.
pub fn with_state_read_scope<T>(f: impl FnOnce() -> T) -> T {
    struct Exit;
    impl Drop for Exit {
        fn drop(&mut self) {
            let depth = DEPTH.with(|depth| {
                let next = depth.get().saturating_sub(1);
                depth.set(next);
                next
            });
            if depth == 0 {
                SCOPE.with(|scope| scope.borrow_mut().take());
            }
        }
    }
    let outermost = DEPTH.with(|depth| {
        let current = depth.get();
        depth.set(current + 1);
        current == 0
    });
    if outermost {
        SCOPE.with(|scope| *scope.borrow_mut() = Some(ScopeMemo::default()));
    }
    let _exit = Exit;
    f()
}

/// True while a [`with_state_read_scope`] is active on this thread.
pub fn in_state_read_scope() -> bool {
    DEPTH.with(|depth| depth.get() > 0)
}

/// How many times the projection `tag` was really computed on this thread
/// (memo hits are not counted; computations outside a scope are). For tests
/// and perf probes.
pub fn evaluations(tag: &str) -> u64 {
    EVALUATIONS.with(|counts| counts.borrow().get(tag).copied().unwrap_or(0))
}

/// Reset [`evaluations`] for this thread.
pub fn reset_evaluations() {
    EVALUATIONS.with(|counts| counts.borrow_mut().clear());
}

fn count_evaluation(tag: &'static str) {
    EVALUATIONS.with(|counts| *counts.borrow_mut().entry(tag).or_insert(0) += 1);
}

fn ambient_context(extra: &str) -> String {
    context_with_steer(
        &agent_doc_queue::nested_claim_steer::installed_fingerprint(),
        extra,
    )
}

fn context_with_steer(steer: &str, extra: &str) -> String {
    format!(
        "{steer}\u{0}{}\u{0}{extra}",
        agent_doc_queue::goal_preset_terminal::installed_fingerprint(),
    )
}

/// `eval()` for `(tag, file, content)`, served from the active scope when the
/// same text, ambient context, `extra` key, and state write epoch produced it
/// before. Outside a scope, or while a planned claim overlay is installed,
/// this is exactly `eval()`.
pub(crate) fn memoized<T: Clone + 'static>(
    tag: &'static str,
    file: &Path,
    content: &str,
    extra: &str,
    eval: impl FnOnce() -> T,
) -> T {
    memoized_ok(tag, file, content, extra, || {
        Ok::<_, std::convert::Infallible>(eval())
    })
    .unwrap_or_else(|never| match never {})
}

/// [`memoized`] for a projection that first installs the document's own
/// nested-claim steer view (`steer_guard_for(file, content)`), as the claim
/// holds and the ownership plan do. When that view is non-empty it replaces
/// whatever view the caller installed, so the caller's view is not part of the
/// key; when it is empty the caller's view stays in force and is. Without this
/// the plan was derived once inside and once outside a caller's claim view on
/// the same text, though both read the same snapshot.
pub(crate) fn memoized_self_steered<T: Clone + 'static, E>(
    tag: &'static str,
    file: &Path,
    content: &str,
    extra: &str,
    eval: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    if !in_state_read_scope() || crate::queue_claim::planned_claim_overlay_installed() {
        count_evaluation(tag);
        return eval();
    }
    let own = memoized("nested_claim_steer_snapshots", file, content, "", || {
        crate::queue_claim::nested_claim_steer_snapshots(file, content)
    });
    let steer = if own.is_empty() {
        agent_doc_queue::nested_claim_steer::installed_fingerprint()
    } else {
        format!("{own:?}")
    };
    memoized_in_context(tag, file, content, context_with_steer(&steer, extra), eval)
}

/// [`memoized`] for a fallible projection. An `Err` is returned as is and never
/// recorded, so a failing read is retried by the next caller.
pub(crate) fn memoized_ok<T: Clone + 'static, E>(
    tag: &'static str,
    file: &Path,
    content: &str,
    extra: &str,
    eval: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    if !in_state_read_scope() || crate::queue_claim::planned_claim_overlay_installed() {
        count_evaluation(tag);
        return eval();
    }
    memoized_in_context(tag, file, content, ambient_context(extra), eval)
}

fn memoized_in_context<T: Clone + 'static, E>(
    tag: &'static str,
    file: &Path,
    content: &str,
    context: String,
    eval: impl FnOnce() -> Result<T, E>,
) -> Result<T, E> {
    let key = Key {
        tag,
        file: file.to_path_buf(),
        context,
        epoch: agent_doc_sqlite::state_store::queue_document_state_write_epoch(),
    };
    let hit = SCOPE.with(|scope| {
        scope.borrow().as_ref().and_then(|memo| {
            memo.entries.get(&key).and_then(|entries| {
                entries
                    .iter()
                    .find(|(text, _)| text.as_ref() == content)
                    .and_then(|(_, value)| value.downcast_ref::<T>().cloned())
            })
        })
    });
    if let Some(value) = hit {
        return Ok(value);
    }
    count_evaluation(tag);
    let value = eval()?;
    // A write during the computation (this projection's own recovery, or a
    // nested gate) means the value may straddle two ledger states: serve it
    // to this caller only.
    if agent_doc_sqlite::state_store::queue_document_state_write_epoch() == key.epoch {
        SCOPE.with(|scope| {
            if let Some(memo) = scope.borrow_mut().as_mut() {
                memo.entries
                    .entry(key)
                    .or_default()
                    .push((Arc::from(content), Rc::new(value.clone())));
            }
        });
    }
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_scope_serves_one_computation_per_text_and_epoch() {
        reset_evaluations();
        let file = Path::new("/tmp/state-read-scope-unit.md");
        let computed = Cell::new(0);
        let read = |content: &str| {
            memoized("unit", file, content, "", || {
                computed.set(computed.get() + 1);
                content.len()
            })
        };
        // Outside a scope every call computes.
        read("a");
        read("a");
        assert_eq!(computed.get(), 2);

        with_state_read_scope(|| {
            assert_eq!(read("abc"), 3);
            assert_eq!(read("abc"), 3);
            assert_eq!(computed.get(), 3, "the same text is served from the scope");
            assert_eq!(read("abcd"), 4);
            assert_eq!(computed.get(), 4, "a different text is a miss");
            // A nested scope joins the outer snapshot.
            with_state_read_scope(|| assert_eq!(read("abc"), 3));
            assert_eq!(computed.get(), 4);
            // An in-process state write ends every entry.
            agent_doc_sqlite::state_store::note_queue_document_state_write();
            assert_eq!(read("abc"), 3);
            assert_eq!(computed.get(), 5, "a write since the entry is a miss");
        });
        assert!(!in_state_read_scope());
        read("abc");
        assert_eq!(computed.get(), 6, "the snapshot ends with the scope");
        assert_eq!(evaluations("unit"), 6);
    }

    #[test]
    fn a_value_computed_across_a_write_is_not_recorded_and_errors_retry() {
        let file = Path::new("/tmp/state-read-scope-unit-2.md");
        let computed = Cell::new(0);
        with_state_read_scope(|| {
            let writing = || {
                memoized("unit-writing", file, "x", "", || {
                    computed.set(computed.get() + 1);
                    agent_doc_sqlite::state_store::note_queue_document_state_write();
                })
            };
            writing();
            writing();
            assert_eq!(
                computed.get(),
                2,
                "a self-writing computation is never served"
            );

            let failing = || {
                memoized_ok("unit-failing", file, "x", "", || {
                    computed.set(computed.get() + 1);
                    Err::<(), _>("unreadable")
                })
            };
            assert!(failing().is_err());
            assert!(failing().is_err());
            assert_eq!(computed.get(), 4, "an error is retried, never recorded");
        });
    }
}
