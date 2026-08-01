//! Lifetime-typed lazily scopes (`#stategraphjoin`).
//!
//! Every ad-hoc `Context` / `ThreadSafeContext` constructed inside a type is a
//! **private graph island**. Nothing outside can derive from its cells, invalidation
//! never crosses it, and a `Computed` created in one is Computed in name only — it
//! recomputes in isolation and nothing can depend on it.
//!
//! A shared context is necessary but **not sufficient**: a bare `&ThreadSafeContext`
//! parameter lets a cell join *any* graph, including one with the wrong lifetime, and
//! neither mistake is caught at runtime.
//!
//! - a document-scoped cell placed in a turn graph is torn down at closeout and
//!   silently stops updating;
//! - a turn-scoped cell placed in a document graph leaks across turns.
//!
//! Both surface much later as a stale value, the most expensive failure shape in this
//! codebase. So the scope is a **type**, and the type names the lifecycle. Dropping a
//! scope drops its context and every cell created in it, so teardown *is* the scope's
//! lifetime rather than a separate deregistration step.
//!
//! # Why this is its own crate
//!
//! The scope types were introduced in `agent-doc-state-backbone`, which depends on
//! `agent-doc-turn` — so the crates holding the remaining islands (`agent-doc-turn`,
//! `agent-doc-merge`, `agent-doc-supervisor`, `agent-doc-element-queue`,
//! `agent-doc-tmux`) could not name a scope without a dependency cycle. A leaf crate
//! with nothing but `lazily` under it can be depended on from anywhere, which is what
//! makes the rule enforceable across the whole workspace instead of in one crate.
//! `agent-doc-state-backbone` re-exports these types, so existing
//! `agent_doc_state_backbone::DocumentScope` paths keep working.
//!
//! # Two families, one rule (`#stategraphjoin-local`)
//!
//! [`DocumentScope`] / [`TurnScope`] / [`ProcessScope`] wrap `ThreadSafeContext`.
//! [`LocalDocumentScope`] / [`LocalTurnScope`] / [`LocalProcessScope`] /
//! [`LocalReadScope`] wrap the single-threaded `lazily::Context` for the thread-local
//! memo scopes that cannot pay for atomics. The rule does not change across that
//! boundary: state joins the scope whose lifecycle matches, and the scope's drop is
//! the teardown.

use lazily::{Context, ThreadSafeContext};

macro_rules! state_scope {
    ($name:ident, $ctx:ty, $doc:literal) => {
        #[doc = $doc]
        #[derive(Default)]
        pub struct $name {
            ctx: $ctx,
        }

        impl $name {
            pub fn new() -> Self {
                Self { ctx: <$ctx>::new() }
            }

            /// The underlying context. Cells created here join this scope's graph and
            /// share its lifetime.
            pub fn ctx(&self) -> &$ctx {
                &self.ctx
            }
        }
    };
}

state_scope!(
    DocumentScope,
    ThreadSafeContext,
    "State whose lifetime is one open document. Document facts (write convergence, queue state) belong here."
);
state_scope!(
    TurnScope,
    ThreadSafeContext,
    "State whose lifetime is one response cycle; dropped at closeout, taking its cells with it."
);
state_scope!(
    ProcessScope,
    ThreadSafeContext,
    "State whose lifetime is the controller/supervisor process."
);

// ---------------------------------------------------------------------------
// Single-threaded family (`#stategraphjoin-local`).
//
// The scopes above wrap `ThreadSafeContext`. State built on the single-threaded
// `lazily::Context` — the thread-local memo scopes in `ops-log-io`, `tmux-io`,
// `supervisor-io`, `sync-io` and `prompt-context` — had no scope of the right kind
// to join, so those sites named the gap in a comment instead. A comment is not a
// lifetime: it does not drop anything, and it cannot stop the next `Context::new()`
// from being an island. These types close that gap so the rule is the same rule on
// both sides of the thread-safety boundary.
// ---------------------------------------------------------------------------

state_scope!(
    LocalDocumentScope,
    Context,
    "Single-threaded twin of [`DocumentScope`]: state whose lifetime is one open document."
);
state_scope!(
    LocalTurnScope,
    Context,
    "Single-threaded twin of [`TurnScope`]: state whose lifetime is one response cycle."
);
state_scope!(
    LocalProcessScope,
    Context,
    "Single-threaded twin of [`ProcessScope`]: state whose lifetime is the process."
);
state_scope!(
    LocalReadScope,
    Context,
    "A bounded read scope: memoized reads that are valid only while the scope is open.\n\n\
     Narrower than a turn — a tmux observation cache or a state-ledger read cache is\n\
     valid until the underlying system is mutated, not until the turn ends. Dropping\n\
     and re-taking the scope *is* the invalidation: it takes the whole graph with it,\n\
     which is why these caches never needed an eviction policy."
);

#[cfg(test)]
mod tests {
    use super::*;

    /// The property the whole rule rests on: two machines built from one scope are in
    /// one graph, so a write through one is visible to a read through the other's
    /// context. An island would fail this by construction.
    #[test]
    fn cells_built_from_one_scope_share_a_graph() {
        let scope = DocumentScope::new();
        let left = scope.ctx().source(1u32);
        let right = scope.ctx().computed(move |ctx| ctx.get(&left) + 1);

        assert_eq!(scope.ctx().get(&right), 2);
        scope.ctx().set(&left, 41);
        assert_eq!(
            scope.ctx().get(&right),
            42,
            "invalidation must cross between cells created in the same scope"
        );
    }

    /// Distinct scopes are distinct graphs even when they are the same *type* — the
    /// type names the lifecycle, and each instance owns one lifetime.
    #[test]
    fn separate_scopes_are_separate_graphs() {
        let a = DocumentScope::new();
        let b = DocumentScope::new();
        let in_a = a.ctx().source(1u32);
        let in_b = b.ctx().source(1u32);

        a.ctx().set(&in_a, 9);
        assert_eq!(a.ctx().get(&in_a), 9);
        assert_eq!(
            b.ctx().get(&in_b),
            1,
            "one document's state must not move another document's"
        );
    }

    /// `#stategraphjoin-local`: the single-threaded family must carry the same
    /// property, or the sites that join it gained a type name and nothing else.
    #[test]
    fn cells_built_from_one_local_scope_share_a_graph() {
        let scope = LocalTurnScope::new();
        let left = scope.ctx().source(1u32);
        let right = scope.ctx().computed(move |ctx| ctx.get(&left) + 1);

        assert_eq!(scope.ctx().get(&right), 2);
        scope.ctx().set(&left, 41);
        assert_eq!(
            scope.ctx().get(&right),
            42,
            "invalidation must cross between cells created in the same local scope"
        );
    }

    /// Re-taking a read scope is the invalidation the memo caches rely on: the old
    /// graph goes with the old scope, so nothing memoized in it can be read back.
    #[test]
    fn retaking_a_local_read_scope_drops_the_previous_graph() {
        let mut scope = LocalReadScope::new();
        let memoized = scope.ctx().source(7u32);
        assert_eq!(scope.ctx().get(&memoized), 7);

        scope = LocalReadScope::new();
        let fresh = scope.ctx().source(0u32);
        assert_eq!(
            scope.ctx().get(&fresh),
            0,
            "a re-taken read scope starts from an empty graph"
        );
    }
}
