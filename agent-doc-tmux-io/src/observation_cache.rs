//! Command-scoped tmux observation cache (`#syncobscache`).
//!
//! Reconciliation passes such as `agent-doc sync` re-ask tmux the same
//! structural questions many times while resolving panes for each tracked
//! document. Measured on a real focus sync: **109 subprocess spawns collapsing
//! to 35 distinct commands** — 53 `display-message` (20 distinct), 35
//! `list-panes` (7 distinct) — at ~20ms per spawn, which was the whole of the
//! multi-second lag between switching a file in the editor and tmux focus
//! following.
//!
//! This module memoizes *read-only* tmux observations for the duration of one
//! explicitly-opened scope, using lazily's [`ComputedMap`] — a derived-slot keyed
//! collection with **no `set`**, so the cache can only ever derive from an
//! observation and can never become a source of truth.
//!
//! Three properties keep it safe:
//!
//! 1. **Opt-in.** Nothing is cached unless a caller opens a scope, so polling
//!    paths (`route` waiting for a dispatch-ready prompt) are unaffected.
//! 2. **Mutation-invalidated.** Any command that is not on the read-only
//!    allowlist clears the cache before running, so a `select-window` or
//!    `swap-window` cannot leave a stale pane list behind it.
//! 3. **Fail-safe classification.** `TmuxCommand::is_read_only` is an allowlist
//!    of stable structural metadata; an unrecognized verb counts as mutating.
//!    Live-content reads like `capture-pane` are deliberately excluded.
//!
//! The scope is thread-local (lazily's `Context`/`ComputedMap` are `Rc`-based and
//! therefore `!Send`), which also means one thread's scope never serves another
//! thread's reads.

use std::cell::RefCell;

use agent_doc_state_scope::LocalReadScope;
use agent_doc_tmux_commands::TmuxCommand;
use lazily::ComputedMap;

use crate::TmuxIoError;

struct ObservationScopeState {
    /// The scope this cache's slots live in (`#stategraphjoin`). Its lifetime is
    /// exactly the observation scope's depth-counted extent, and re-taking it is
    /// how a tmux mutation invalidates every memoized observation at once.
    scope: LocalReadScope,
    /// Keyed by the full argv so two different `display-message` formats never
    /// collide. Only successful outputs are stored — an error may be transient
    /// and must not be memoized.
    reads: ComputedMap<Vec<String>, String>,
    depth: usize,
    hits: u64,
    misses: u64,
}

impl ObservationScopeState {
    fn new() -> Self {
        let scope = LocalReadScope::new();
        let reads = ComputedMap::new(scope.ctx());
        Self {
            scope,
            reads,
            depth: 1,
            hits: 0,
            misses: 0,
        }
    }

    fn reset_entries(&mut self) {
        // Drop the derived slots wholesale rather than evicting key by key: a
        // mutation can invalidate observations we never explicitly recorded a
        // dependency on (a new pane changes `list-panes` for every window).
        // Re-taking the scope IS the invalidation: a tmux mutation must drop every
        // memoized observation, and dropping the scope drops the whole graph.
        self.scope = LocalReadScope::new();
        self.reads = ComputedMap::new(self.scope.ctx());
    }
}

thread_local! {
    static SCOPE: RefCell<Option<ObservationScopeState>> = const { RefCell::new(None) };
}

/// Statistics for one completed scope, used by callers that log sync latency.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct ObservationScopeStats {
    pub hits: u64,
    pub misses: u64,
}

impl ObservationScopeStats {
    /// Subprocess spawns avoided by the cache.
    pub fn spawns_avoided(&self) -> u64 {
        self.hits
    }
}

/// RAII guard for an active observation scope. Read-only tmux commands issued
/// on this thread while the guard is alive are memoized; dropping it discards
/// the memo so a later run always re-observes.
#[must_use = "the observation cache is active only while the guard is alive"]
pub struct TmuxObservationScope {
    /// The pane -> document owner graph shares this scope's lifetime: it answers
    /// questions about the processes running in the panes observed here, and a
    /// tmux mutation is the event that can change them. Held so the two scopes
    /// open and close together rather than requiring every call site to
    /// remember both.
    _process_observations: agent_doc_process_owner_io::ProcessObservationScope,
    _not_send: std::marker::PhantomData<*const ()>,
}

/// Open a command-scoped tmux observation cache on this thread.
///
/// Nested calls are reference-counted: an inner scope does not discard the
/// outer scope's memo when it drops.
pub fn begin_observation_scope() -> TmuxObservationScope {
    SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        match scope.as_mut() {
            Some(state) => state.depth += 1,
            None => *scope = Some(ObservationScopeState::new()),
        }
    });
    TmuxObservationScope {
        _process_observations: agent_doc_process_owner_io::begin_process_observation_scope(),
        _not_send: std::marker::PhantomData,
    }
}

/// Statistics for the currently active scope, if any.
pub fn observation_scope_stats() -> Option<ObservationScopeStats> {
    SCOPE.with(|scope| {
        scope.borrow().as_ref().map(|state| ObservationScopeStats {
            hits: state.hits,
            misses: state.misses,
        })
    })
}

impl Drop for TmuxObservationScope {
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

/// Run `command`, serving it from the active observation scope when it is a
/// cacheable read. Without an active scope this is a straight pass-through to
/// `spawn`.
pub(crate) fn run_with_observation_cache(
    command: &TmuxCommand,
    spawn: impl FnOnce() -> Result<String, TmuxIoError>,
) -> Result<String, TmuxIoError> {
    let cacheable = command.is_read_only();

    if !cacheable {
        // A mutation invalidates every observation, including ones whose
        // dependency on it we cannot express.
        SCOPE.with(|scope| {
            if let Some(state) = scope.borrow_mut().as_mut() {
                state.reset_entries();
            }
        });
        let result = spawn();
        // The pane -> document owner graph CAN express the dependency, so it is
        // re-observed rather than discarded: one `/proc` walk, written into each
        // pane's `Source` under the equality guard. A pane whose process tree the
        // mutation did not touch invalidates nothing; only a pane that actually
        // changed recomputes its owner cells. Refresh after the mutation so the
        // observation reflects the pane it created, killed, or respawned.
        agent_doc_process_owner_io::refresh_process_observations();
        return result;
    }

    let key = command.args().to_vec();

    let cached = SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        let state = scope.as_mut()?;
        let hit = state.reads.get(state.scope.ctx(), &key);
        if hit.is_some() {
            state.hits += 1;
        }
        hit
    });
    if let Some(value) = cached {
        return Ok(value);
    }

    let value = spawn()?;

    SCOPE.with(|scope| {
        let mut scope = scope.borrow_mut();
        if let Some(state) = scope.as_mut() {
            state.misses += 1;
            let observed = value.clone();
            // `get_or_insert_with` is lazily's mint-on-access recipe: the slot is
            // materialized from the observation we just made and thereafter
            // returns without re-running the factory.
            state
                .reads
                .get_or_insert_with(state.scope.ctx(), key, move |_, _| observed.clone());
        }
    });

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_cmd(pane: &str) -> TmuxCommand {
        TmuxCommand::new(["display-message", "-p", "-t", pane, "#{pane_current_path}"])
    }

    #[test]
    fn without_a_scope_every_read_spawns() {
        let mut spawns = 0;
        for _ in 0..3 {
            run_with_observation_cache(&read_cmd("%1"), || {
                spawns += 1;
                Ok("/tmp".to_string())
            })
            .unwrap();
        }
        assert_eq!(spawns, 3, "caching must be opt-in");
    }

    #[test]
    fn duplicate_reads_in_a_scope_spawn_once() {
        let _scope = begin_observation_scope();
        let mut spawns = 0;
        for _ in 0..5 {
            let value = run_with_observation_cache(&read_cmd("%1"), || {
                spawns += 1;
                Ok("/tmp".to_string())
            })
            .unwrap();
            assert_eq!(value, "/tmp");
        }
        assert_eq!(spawns, 1, "repeat reads must be served from the memo");
        let stats = observation_scope_stats().unwrap();
        assert_eq!((stats.hits, stats.misses), (4, 1));
    }

    #[test]
    fn distinct_keys_are_cached_independently() {
        let _scope = begin_observation_scope();
        let mut spawns = 0;
        for pane in ["%1", "%2", "%1", "%2"] {
            run_with_observation_cache(&read_cmd(pane), || {
                spawns += 1;
                Ok(format!("/tmp/{pane}"))
            })
            .unwrap();
        }
        assert_eq!(spawns, 2);
        let value =
            run_with_observation_cache(&read_cmd("%2"), || panic!("must be cached")).unwrap();
        assert_eq!(value, "/tmp/%2");
    }

    #[test]
    fn a_mutation_invalidates_the_scope() {
        let _scope = begin_observation_scope();
        let mut spawns = 0;
        run_with_observation_cache(&read_cmd("%1"), || {
            spawns += 1;
            Ok("before".to_string())
        })
        .unwrap();

        run_with_observation_cache(&TmuxCommand::new(["select-window", "-t", "0"]), || {
            Ok(String::new())
        })
        .unwrap();

        let after = run_with_observation_cache(&read_cmd("%1"), || {
            spawns += 1;
            Ok("after".to_string())
        })
        .unwrap();

        assert_eq!(spawns, 2, "a mutation must force re-observation");
        assert_eq!(after, "after");
    }

    #[test]
    fn errors_are_not_memoized() {
        let _scope = begin_observation_scope();
        let mut spawns = 0;
        for _ in 0..2 {
            let _ = run_with_observation_cache(&read_cmd("%1"), || {
                spawns += 1;
                Err(TmuxIoError::Failed {
                    code: Some(1),
                    stderr: "no server".to_string(),
                })
            });
        }
        assert_eq!(spawns, 2, "a transient failure must not be cached");
    }

    #[test]
    fn live_content_reads_are_never_cached() {
        let _scope = begin_observation_scope();
        let mut spawns = 0;
        for _ in 0..3 {
            run_with_observation_cache(
                &TmuxCommand::new(["capture-pane", "-p", "-t", "%1"]),
                || {
                    spawns += 1;
                    Ok("prompt".to_string())
                },
            )
            .unwrap();
        }
        assert_eq!(
            spawns, 3,
            "capture-pane observes live content; memoizing it would wedge route's prompt poll"
        );
    }

    #[test]
    fn scope_ends_with_the_guard() {
        let mut spawns = 0;
        {
            let _scope = begin_observation_scope();
            for _ in 0..2 {
                run_with_observation_cache(&read_cmd("%1"), || {
                    spawns += 1;
                    Ok("/tmp".to_string())
                })
                .unwrap();
            }
        }
        run_with_observation_cache(&read_cmd("%1"), || {
            spawns += 1;
            Ok("/tmp".to_string())
        })
        .unwrap();
        assert_eq!(spawns, 2, "a dropped scope must not serve a later run");
    }

    #[test]
    fn nested_scopes_do_not_discard_the_outer_memo() {
        let outer = begin_observation_scope();
        let mut spawns = 0;
        run_with_observation_cache(&read_cmd("%1"), || {
            spawns += 1;
            Ok("/tmp".to_string())
        })
        .unwrap();
        {
            let _inner = begin_observation_scope();
            run_with_observation_cache(&read_cmd("%1"), || {
                spawns += 1;
                Ok("/tmp".to_string())
            })
            .unwrap();
        }
        run_with_observation_cache(&read_cmd("%1"), || {
            spawns += 1;
            Ok("/tmp".to_string())
        })
        .unwrap();
        assert_eq!(
            spawns, 1,
            "an inner scope drop must not clear the outer memo"
        );
        drop(outer);
    }
}
