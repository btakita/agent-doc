//! Current-document resolution as a derived value (`#sccurrentpass`).
//!
//! Resolving the current document is the most expensive thing `session-check`
//! does — profiled at ~491ms per call. The sweep needs one resolution shared
//! across every guard that reads the document, re-taken only when a step
//! actually rewrote it.
//!
//! That used to be a `HashMap` memo plus a `remove()` call. The map worked; what
//! did not was the call sites, which invalidated after every self-heal that
//! *could* rewrite the document rather than every one that *did* (fixed
//! separately). With that corrected, the remaining problem with the map is that
//! it is not a dependency: nothing can derive from it, and "the entry is stale"
//! is expressed by deleting it rather than by anything the graph understands.
//!
//! So the version is a [`Source`] and the resolution is a [`Computed`] over it.
//! Bumping the version invalidates the resolution; equal versions reuse it. The
//! observable behavior matches the map exactly — this is the same policy, said
//! in the vocabulary the rest of the codebase derives in.
//!
//! # Why the thread-safe family
//!
//! `lazily::Context` is `Rc`-based and not `Clone`, and the single-threaded
//! `ComputedMap` factory is `Fn(&K) -> V` with no context parameter — so a
//! single-threaded derived entry cannot read another cell at all.
//! `ThreadSafeContext` is `Clone`, so the derived entry can capture the graph
//! and subscribe to the version. (The real fix belongs upstream: `mint_with`
//! already threads the context into the compute closure and
//! `get_or_insert_handle` discards it. Exposing that would remove the capture
//! from both families.)
//!
//! The scope is a [`TurnScope`]: one sweep, dropped with the pass, so ending the
//! pass and dropping the cache are the same event rather than two things to keep
//! in sync.

use std::cell::RefCell;
use std::path::{Path, PathBuf};

use agent_doc_document_realtime::CurrentDocument;
use agent_doc_state_scope::TurnScope;
use lazily::{ThreadSafeComputedMap, ThreadSafeSourceMap};

thread_local! {
    /// The pass graph, present only inside [`with_current_document_pass`].
    ///
    /// Scoped rather than ambient, for the same reason the map it replaces was:
    /// outside a sweep, resolution must be unmemoized.
    static PASS: RefCell<Option<PassGraph>> = const { RefCell::new(None) };
}

struct PassGraph {
    scope: TurnScope,
    /// Monotonic per-document version. Bumping it is the invalidation.
    version: ThreadSafeSourceMap<PathBuf, u64>,
    resolved: ThreadSafeComputedMap<PathBuf, Option<CurrentDocument>>,
}

impl PassGraph {
    fn new() -> Self {
        let scope = TurnScope::new();
        let version = ThreadSafeSourceMap::new(scope.ctx());
        let resolved = ThreadSafeComputedMap::new(scope.ctx());
        Self {
            scope,
            version,
            resolved,
        }
    }
}

/// Run `f` with the pass graph installed (`#sccurrentpass`).
///
/// Nested calls reuse the outer graph, so a sweep that delegates into another
/// entry point still observes one document version.
pub(crate) fn with_current_document_pass<T>(f: impl FnOnce() -> T) -> T {
    let installed = PASS.with(|slot| {
        let mut slot = slot.borrow_mut();
        if slot.is_some() {
            return false;
        }
        *slot = Some(PassGraph::new());
        true
    });
    let result = f();
    if installed {
        PASS.with(|slot| *slot.borrow_mut() = None);
    }
    result
}

/// Invalidate `file`'s resolution after a step that actually rewrote it.
///
/// Call sites are responsible for only reaching here on a real mutation — a
/// step that healed nothing must not bump the version, or the sweep pays a
/// fresh resolve to observe an unchanged document.
pub(crate) fn invalidate_current_document_pass(file: &Path) {
    PASS.with(|slot| {
        let slot = slot.borrow();
        let Some(graph) = slot.as_ref() else {
            return;
        };
        let key = file.to_path_buf();
        let ctx = graph.scope.ctx();
        let next = graph.version.observe(ctx, &key).unwrap_or(0).wrapping_add(1);
        graph.version.set(ctx, key, next);
    });
}

/// The pass-scoped resolution for `file`.
///
/// `None` outside a pass, and also when the resolve failed — the caller then
/// re-runs it uncached so the real error surfaces instead of a cached absence.
pub(crate) fn pass_resolved(
    file: &Path,
    resolve: impl Fn() -> Option<CurrentDocument> + Send + Sync + 'static,
) -> Option<CurrentDocument> {
    PASS.with(|slot| {
        let slot = slot.borrow();
        let graph = slot.as_ref()?;
        let key = file.to_path_buf();
        let ctx = graph.scope.ctx();
        if !graph.version.is_present(&key) {
            graph.version.set(ctx, key.clone(), 0);
        }
        let tracked_ctx = ctx.clone();
        let version = graph.version.clone();
        graph.resolved.get_or_insert_with(ctx, key, move |k| {
            // Reading the version subscribes this entry to it, so a later bump
            // invalidates exactly this document's resolution.
            let _tracked = version.observe(&tracked_ctx, k);
            resolve()
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};
    use std::sync::Arc;

    fn counting_resolve(calls: &Arc<AtomicU32>) -> impl Fn() -> Option<CurrentDocument> + Send + Sync + 'static {
        let calls = Arc::clone(calls);
        move || {
            calls.fetch_add(1, Ordering::SeqCst);
            None
        }
    }

    /// The property the sweep depends on: repeated reads with no invalidation
    /// resolve once.
    #[test]
    fn repeated_reads_within_a_pass_resolve_once() {
        let calls = Arc::new(AtomicU32::new(0));
        with_current_document_pass(|| {
            let file = Path::new("/tmp/agent-doc-pass.md");
            for _ in 0..5 {
                let _ = pass_resolved(file, counting_resolve(&calls));
            }
            assert_eq!(
                calls.load(Ordering::SeqCst),
                1,
                "a pass must resolve the document once, not once per reader"
            );
        });
    }

    /// A real rewrite must be observed: bumping the version invalidates.
    #[test]
    fn an_invalidation_forces_exactly_one_more_resolve() {
        let calls = Arc::new(AtomicU32::new(0));
        with_current_document_pass(|| {
            let file = Path::new("/tmp/agent-doc-pass-invalidate.md");
            let _ = pass_resolved(file, counting_resolve(&calls));
            assert_eq!(calls.load(Ordering::SeqCst), 1);

            invalidate_current_document_pass(file);
            let _ = pass_resolved(file, counting_resolve(&calls));
            assert_eq!(
                calls.load(Ordering::SeqCst),
                2,
                "a bumped version must invalidate the derived resolution"
            );

            // No further invalidation: still reused.
            let _ = pass_resolved(file, counting_resolve(&calls));
            assert_eq!(calls.load(Ordering::SeqCst), 2);
        });
    }

    /// Distinct documents do not share a resolution.
    #[test]
    fn documents_are_keyed_independently() {
        let calls = Arc::new(AtomicU32::new(0));
        with_current_document_pass(|| {
            let _ = pass_resolved(Path::new("/tmp/a.md"), counting_resolve(&calls));
            let _ = pass_resolved(Path::new("/tmp/b.md"), counting_resolve(&calls));
            assert_eq!(calls.load(Ordering::SeqCst), 2);
            // Invalidating one must not invalidate the other.
            invalidate_current_document_pass(Path::new("/tmp/a.md"));
            let _ = pass_resolved(Path::new("/tmp/b.md"), counting_resolve(&calls));
            assert_eq!(
                calls.load(Ordering::SeqCst),
                2,
                "invalidating one document must not re-resolve another"
            );
        });
    }

    /// Outside a pass there is no memo at all, matching the previous behavior.
    #[test]
    fn resolution_is_unmemoized_outside_a_pass() {
        let calls = Arc::new(AtomicU32::new(0));
        assert!(pass_resolved(Path::new("/tmp/doc.md"), counting_resolve(&calls)).is_none());
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "no pass means no graph to resolve into"
        );
    }

    /// The graph is dropped with the pass, so a second sweep starts empty.
    #[test]
    fn a_pass_does_not_outlive_itself() {
        with_current_document_pass(|| PASS.with(|slot| assert!(slot.borrow().is_some())));
        PASS.with(|slot| {
            assert!(
                slot.borrow().is_none(),
                "the pass graph must not survive the sweep that installed it"
            )
        });
    }
}
