//! Phase E rung 1 (`#adstatechart1`): local-process Harel state chart that
//! consolidates root cause **E** — transport / editor-sync / closeout /
//! supervisor state that is otherwise scattered across booleans and string
//! enums, where a forgotten guard at one of N call sites is the recurring
//! failure shape (`#boundaryaccum`, `#live_prompt_drift`, `#qftlossdelta`).
//!
//! # Scope boundary (load-bearing — do not cross)
//!
//! This chart is **per-process local state only**. It is compute, not a wire
//! protocol. It does NOT model — and must not be extended to model — the
//! genuinely distributed failures:
//!
//! - stale-**binary** supervisor vs newer finalize client (`stale_install`)
//! - concurrent sessions force-pushing the shared superproject
//! - editor-plugin vs CLI version skew (that is root cause **B**,
//!   `#ipcverhandshake`)
//!
//! Those stay git/CRDT authority + recycle. A per-process chart would not have
//! prevented this doc's stale-cdylib-with-no-live-supervisor wedge.
//!
//! # API shape
//!
//! The underlying `lazily::StateChart` rejects `{"expr": …}` extended-state
//! guards and `run` actions, so every guard is computed **here** in pure Rust
//! from [`ChartFacts`] and passed to `send` as a named boolean via
//! [`guard_map`]. Guards stay pure and unit-testable; the chart never reads
//! extended state. An absent/unknown guard name fails closed (`send` → `false`).
//!
//! Rung 1 is NOT wired into the live write path — it defines the chart, the
//! guards, and proves the load-bearing rejected edge (`commit` while the editor
//! buffer is ahead of disk). Rungs 2–4 (observability read, load-bearing
//! closeout guard, fold-in of the `#mergestatemachine3` merge-ownership FSM)
//! are tracked in `tasks/agent-doc/prd-adstatechart-local-process-statechart.md`.

use lazily::{ChartBuilder, ChartDef, StateBuilder, StateChart, ThreadSafeStateChart};
use serde::{Deserialize, Serialize};

/// Raw per-process facts the named guards are computed from. These are the only
/// inputs the chart consumes; it never reads files, IPC, or process state.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChartFacts {
    /// Monotonic editor-buffer edit epoch (bumped on each live edit).
    pub edit_epoch: u64,
    /// Last edit epoch the binary has flushed/synced to disk.
    pub last_synced_epoch: u64,
    /// The editor IPC socket send failed or timed out (no_ack / send_failed).
    pub ipc_send_failed: bool,
    /// No socket listener owns the project (VS Code / pluginless / stale sock).
    pub ipc_no_listener: bool,
    /// Build id the running supervisor/session was launched from, if known.
    pub running_build_id: Option<String>,
    /// Build id currently installed on disk, if known.
    pub installed_build_id: Option<String>,
}

/// The editor buffer is in sync with disk: no unflushed edits are ahead.
pub fn editor_synced(facts: &ChartFacts) -> bool {
    facts.edit_epoch <= facts.last_synced_epoch
}

/// The live editor buffer has unflushed changes ahead of disk. This is the
/// `#live_prompt_drift` "refusing to commit from stale disk" condition.
pub fn editor_ahead(facts: &ChartFacts) -> bool {
    facts.edit_epoch > facts.last_synced_epoch
}

/// The editor IPC socket path is degraded (send failed / no_ack).
pub fn transport_degraded(facts: &ChartFacts) -> bool {
    facts.ipc_send_failed
}

/// No CP editor listener currently owns the project. Durable intent remains
/// retained until a Lazily replica registers; there is no file fallback.
pub fn transport_no_listener(facts: &ChartFacts) -> bool {
    facts.ipc_no_listener
}

/// The running supervisor build differs from the installed build. Only a fact
/// when both ids are known; unknown fails open to `false` (not stale) because
/// staleness must be a proven mismatch, never inferred from missing data.
pub fn supervisor_stale(facts: &ChartFacts) -> bool {
    match (&facts.running_build_id, &facts.installed_build_id) {
        (Some(running), Some(installed)) => running != installed,
        _ => false,
    }
}

/// The load-bearing closeout invariant: a `written → committed` transition is
/// permitted only when the editor buffer is synced (no unflushed edits ahead of
/// disk). Encoded once here; the chart edge carries the `editor_synced` guard so
/// `send("commit", …)` is a rejected edge when this is false.
pub fn commit_allowed(facts: &ChartFacts) -> bool {
    editor_synced(facts)
}

/// Resolve every named guard the chart references for one `send`. Names must
/// match the guard names wired in [`adstatechart_def`]. Any name the chart
/// references but this map omits fails closed inside `StateChart::send`.
pub fn guard_map(facts: &ChartFacts) -> std::collections::HashMap<String, bool> {
    let mut g = std::collections::HashMap::new();
    g.insert("editor_synced".to_string(), editor_synced(facts));
    g.insert("editor_ahead".to_string(), editor_ahead(facts));
    g.insert("transport_degraded".to_string(), transport_degraded(facts));
    g.insert(
        "transport_no_listener".to_string(),
        transport_no_listener(facts),
    );
    g.insert("supervisor_stale".to_string(), supervisor_stale(facts));
    g
}

/// The declarative Harel chart: four orthogonal regions under a parallel root,
/// assembled with the typed lazily 0.19 [`ChartBuilder`] (Phase E rung 3 typed
/// migration; the definition-equivalent of the former `CHART_DEF_JSON`).
///
/// - `transport`:   `socket ↔ degraded` (recovery retries the same durable intent)
/// - `editor_sync`: `synced → editor_ahead → publishing`
/// - `closeout`:    `idle → written → committed → session_ok` (final)
/// - `supervisor`:  `sup_idle / sup_busy / sup_stale → sup_recycle`
///
/// The `closeout.written --commit--> committed` edge carries guard
/// `editor_synced`, making commit-while-editor-ahead a rejected edge. State
/// insertion order (first parent-less state = root) fixes deterministic
/// parallel-region descent, exactly as JSON key order did.
pub fn adstatechart_def() -> Result<ChartDef, String> {
    ChartBuilder::new()
        .state(StateBuilder::parallel("root"))
        // transport region
        .state(StateBuilder::compound("transport", "socket").parent("root"))
        .state(
            StateBuilder::atomic("socket")
                .parent("transport")
                .on("send_timeout", "degraded")
                .on("no_ack", "degraded")
                .on("no_ipc_listener", "degraded"),
        )
        .state(
            StateBuilder::atomic("degraded")
                .parent("transport")
                .on("no_ipc_listener", "degraded")
                .on("socket_recovered", "socket"),
        )
        // editor_sync region
        .state(StateBuilder::compound("editor_sync", "synced").parent("root"))
        .state(
            StateBuilder::atomic("synced")
                .parent("editor_sync")
                .on_guarded("editor_edited", "editor_ahead", "editor_ahead"),
        )
        .state(
            StateBuilder::atomic("editor_ahead")
                .parent("editor_sync")
                .on("publish_started", "publishing")
                .on_guarded("editor_resynced", "synced", "editor_synced"),
        )
        .state(
            StateBuilder::atomic("publishing")
                .parent("editor_sync")
                .on_guarded("publish_acked", "synced", "editor_synced"),
        )
        // closeout region
        .state(StateBuilder::compound("closeout", "idle").parent("root"))
        .state(
            StateBuilder::atomic("idle")
                .parent("closeout")
                .on("write", "written"),
        )
        .state(
            StateBuilder::atomic("written")
                .parent("closeout")
                .on_guarded("commit", "committed", "editor_synced"),
        )
        .state(
            StateBuilder::atomic("committed")
                .parent("closeout")
                .on("session_check_ok", "session_ok"),
        )
        .state(StateBuilder::final_state("session_ok").parent("closeout"))
        // supervisor region
        .state(StateBuilder::compound("supervisor", "sup_idle").parent("root"))
        .state(
            StateBuilder::atomic("sup_idle")
                .parent("supervisor")
                .on("turn_started", "sup_busy")
                .on_guarded("stale_observed", "sup_stale", "supervisor_stale"),
        )
        .state(
            StateBuilder::atomic("sup_busy")
                .parent("supervisor")
                .on("turn_ended", "sup_idle")
                .on_guarded("stale_observed", "sup_stale", "supervisor_stale"),
        )
        .state(
            StateBuilder::atomic("sup_stale")
                .parent("supervisor")
                .on("recycle_requested", "sup_recycle"),
        )
        .state(
            StateBuilder::atomic("sup_recycle")
                .parent("supervisor")
                .on("recycled", "sup_idle"),
        )
        .build()
}

/// Build a fresh [`StateChart`] over `ctx` in its initial configuration.
pub fn new_adstatechart(ctx: &lazily::Context) -> Result<StateChart, String> {
    Ok(StateChart::new(ctx, adstatechart_def()?))
}

/// Build a fresh [`ThreadSafeStateChart`] over `ctx` — same chart semantics as
/// [`new_adstatechart`], but `Send + Sync` so a status-observer thread can read
/// [`ThreadSafeStateChart::configuration`] while another thread drives
/// [`ThreadSafeStateChart::send`]. This is the cross-thread status-observation
/// path Phase E rung 3 wires the supervisor/session status reader through.
pub fn new_adstatechart_threadsafe(
    ctx: &lazily::ThreadSafeContext,
) -> Result<ThreadSafeStateChart, String> {
    Ok(ThreadSafeStateChart::new(ctx, adstatechart_def()?))
}

// ------------------------------------------------------- observability read
// Phase E rung 2 (`#adstatechart2`): an advisory, read-only projection of the
// four orthogonal regions as a compact named snapshot. This does NOT gate
// closeout — it constructs its own throwaway chart, drives it to the fact-implied
// configuration, and reads `active_leaves()` back so session-check / status can
// log `transport.x editor_sync.x closeout.x supervisor.x` alongside the existing
// `ops.log` markers. The guard used for the closeout region is the same
// `editor_synced` (`edit_epoch <= last_synced_epoch`) the live commit path uses
// (`git.rs` `commit_blocked_live_buffer_ahead_of_disk`), so the advisory never
// disagrees with the shipped A/C guard.

/// The observed closeout phase the snapshot should reflect. The closeout region
/// is driven by the real write/commit lifecycle, which is not derivable from
/// [`ChartFacts`] alone, so the caller passes what it observed. Defaults to
/// [`CloseoutPhase::Idle`] (the chart's initial closeout leaf).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CloseoutPhase {
    /// No write has happened this cycle.
    #[default]
    Idle,
    /// The response was written; a `commit` is pending.
    Written,
    /// The cycle committed.
    Committed,
    /// `session-check` passed; the cycle is final.
    SessionOk,
}

/// Observed inputs the snapshot needs that are not derivable from [`ChartFacts`]:
/// the closeout lifecycle position and whether the supervisor is mid-turn.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ObservedPhases {
    /// Where the write/commit lifecycle is this cycle.
    pub closeout: CloseoutPhase,
    /// The supervisor is actively running a turn (drives `sup_idle -> sup_busy`).
    /// Ignored when [`supervisor_stale`] holds — staleness wins.
    pub supervisor_busy: bool,
}

/// Map a leaf state id to its `region.leaf` observability label. Unknown leaves
/// (should not occur for this chart) are returned unprefixed so a drift is
/// visible rather than silently dropped.
fn region_label(leaf: &str) -> String {
    let region = match leaf {
        "socket" | "degraded" => "transport",
        "synced" | "editor_ahead" | "publishing" => "editor_sync",
        "idle" | "written" | "committed" | "session_ok" => "closeout",
        "sup_idle" | "sup_busy" | "sup_stale" | "sup_recycle" => "supervisor",
        _ => return leaf.to_string(),
    };
    format!("{region}.{leaf}")
}

/// The ordered events that drive a fresh chart to the configuration implied by
/// `facts` + `observed`. Shared by the single-threaded and thread-safe snapshot
/// paths so both project identical state — the chart type differs, the driving
/// sequence does not. Regions are orthogonal, so per-region events are
/// independent; the closeout `commit` edge still carries the real `editor_synced`
/// guard, so an editor-ahead `commit` is rejected regardless of the caller's
/// claimed phase.
fn snapshot_events(facts: &ChartFacts, observed: &ObservedPhases) -> Vec<&'static str> {
    let mut events = Vec::new();

    // transport region
    if transport_no_listener(facts) {
        events.push("no_ipc_listener");
    } else if transport_degraded(facts) {
        events.push("send_timeout");
    }

    // editor_sync region
    if editor_ahead(facts) {
        events.push("editor_edited");
    }

    // supervisor region — staleness wins over busy.
    if supervisor_stale(facts) {
        events.push("stale_observed");
    } else if observed.supervisor_busy {
        events.push("turn_started");
    }

    // closeout region — walk the lifecycle up to the observed phase.
    if !matches!(observed.closeout, CloseoutPhase::Idle) {
        events.push("write");
    }
    if matches!(
        observed.closeout,
        CloseoutPhase::Committed | CloseoutPhase::SessionOk
    ) {
        events.push("commit");
    }
    if matches!(observed.closeout, CloseoutPhase::SessionOk) {
        events.push("session_check_ok");
    }

    events
}

/// Format active leaves as the stable advisory line
/// `transport.x editor_sync.x closeout.x supervisor.x`.
fn format_leaves(leaves: Vec<String>) -> String {
    fn region_rank(label: &str) -> u8 {
        match label.split('.').next().unwrap_or("") {
            "transport" => 0,
            "editor_sync" => 1,
            "closeout" => 2,
            "supervisor" => 3,
            _ => 4,
        }
    }
    let mut labels: Vec<String> = leaves.iter().map(|leaf| region_label(leaf)).collect();
    labels.sort_by_key(|l| region_rank(l));
    labels.join(" ")
}

/// Drive a fresh chart from `facts` + `observed` and return the advisory named
/// snapshot: the active leaf of each region, ordered
/// `transport.x editor_sync.x closeout.x supervisor.x`.
///
/// Read-only observability (`#adstatechart2`): builds its own chart and never
/// touches the live closeout path.
pub fn configuration_snapshot(facts: &ChartFacts, observed: &ObservedPhases) -> String {
    // #stategraphjoin-allow: read-only advisory snapshot (`#adstatechart2`). Builds a
    // fresh chart, drives it from the passed facts, formats, and drops — it never
    // touches the live closeout path and nothing derives from it.
    let ctx = lazily::Context::new();
    let Ok(chart) = new_adstatechart(&ctx) else {
        return "adstatechart.unavailable".to_string();
    };
    let g = guard_map(facts);
    for event in snapshot_events(facts, observed) {
        chart.send(&ctx, event, &g);
    }
    format_leaves(chart.active_leaves(&ctx))
}

/// Thread-safe twin of [`configuration_snapshot`], driving a
/// [`ThreadSafeStateChart`] over a [`lazily::ThreadSafeContext`]. Projects the
/// identical snapshot (same [`snapshot_events`]), but is usable from a status
/// observer thread — the cross-thread status-observation path for Phase E rung 3.
pub fn configuration_snapshot_threadsafe(facts: &ChartFacts, observed: &ObservedPhases) -> String {
    // #stategraphjoin-allow: read-only advisory snapshot (`#adstatechart2`). Builds a
    // fresh chart, drives it from the passed facts, formats, and drops — it never
    // touches the live closeout path and nothing derives from it.
    let ctx = lazily::ThreadSafeContext::new();
    let Ok(chart) = new_adstatechart_threadsafe(&ctx) else {
        return "adstatechart.unavailable".to_string();
    };
    let g = guard_map(facts);
    for event in snapshot_events(facts, observed) {
        chart.send(&ctx, event, &g);
    }
    format_leaves(chart.active_leaves(&ctx))
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazily::Context;

    fn facts_synced() -> ChartFacts {
        ChartFacts {
            edit_epoch: 3,
            last_synced_epoch: 3,
            ..ChartFacts::default()
        }
    }

    fn facts_editor_ahead() -> ChartFacts {
        ChartFacts {
            edit_epoch: 5,
            last_synced_epoch: 3,
            ..ChartFacts::default()
        }
    }

    #[test]
    fn guard_editor_synced_truth_table() {
        assert!(editor_synced(&facts_synced()));
        assert!(!editor_ahead(&facts_synced()));
        assert!(!editor_synced(&facts_editor_ahead()));
        assert!(editor_ahead(&facts_editor_ahead()));
        // Equal epochs are synced (boundary).
        let eq = ChartFacts {
            edit_epoch: 7,
            last_synced_epoch: 7,
            ..Default::default()
        };
        assert!(editor_synced(&eq));
        assert!(!editor_ahead(&eq));
    }

    #[test]
    fn guard_transport_and_supervisor_truth_table() {
        let mut f = ChartFacts::default();
        assert!(!transport_degraded(&f));
        assert!(!transport_no_listener(&f));
        f.ipc_send_failed = true;
        assert!(transport_degraded(&f));
        f.ipc_no_listener = true;
        assert!(transport_no_listener(&f));

        // supervisor_stale requires a PROVEN mismatch; unknown ids fail open.
        assert!(!supervisor_stale(&ChartFacts::default()));
        assert!(!supervisor_stale(&ChartFacts {
            running_build_id: Some("a".into()),
            installed_build_id: None,
            ..Default::default()
        }));
        assert!(!supervisor_stale(&ChartFacts {
            running_build_id: Some("a".into()),
            installed_build_id: Some("a".into()),
            ..Default::default()
        }));
        assert!(supervisor_stale(&ChartFacts {
            running_build_id: Some("a".into()),
            installed_build_id: Some("b".into()),
            ..Default::default()
        }));
    }

    #[test]
    fn chart_def_parses_and_enters_initial_configuration() {
        let ctx = Context::new();
        let chart = new_adstatechart(&ctx).expect("chart parses");
        // Every orthogonal region enters its own initial leaf.
        assert!(chart.matches(&ctx, "socket"));
        assert!(chart.matches(&ctx, "synced"));
        assert!(chart.matches(&ctx, "idle"));
        assert!(chart.matches(&ctx, "sup_idle"));
        // Parallel root is active alongside all four regions.
        assert!(chart.matches(&ctx, "root"));
    }

    /// THE load-bearing proof: `commit` while the editor buffer is ahead of disk
    /// is a rejected edge — the configuration stays in `written`, no commit.
    #[test]
    fn commit_while_editor_ahead_is_rejected_edge() {
        let ctx = Context::new();
        let chart = new_adstatechart(&ctx).expect("chart parses");

        // idle -> written always allowed.
        assert!(chart.send(&ctx, "write", &guard_map(&facts_synced())));
        assert!(chart.matches(&ctx, "written"));

        // Editor is ahead of disk: commit MUST be rejected, config unchanged.
        let ahead = facts_editor_ahead();
        assert!(!commit_allowed(&ahead));
        let took = chart.send(&ctx, "commit", &guard_map(&ahead));
        assert!(!took, "commit must be rejected while editor is ahead");
        assert!(
            chart.matches(&ctx, "written"),
            "config must stay in written"
        );
        assert!(!chart.matches(&ctx, "committed"));

        // Editor resynced: commit now allowed.
        let synced = facts_synced();
        assert!(commit_allowed(&synced));
        let took = chart.send(&ctx, "commit", &guard_map(&synced));
        assert!(took, "commit allowed once editor is synced");
        assert!(chart.matches(&ctx, "committed"));

        // Finish the cycle.
        assert!(chart.send(&ctx, "session_check_ok", &guard_map(&synced)));
        assert!(chart.matches(&ctx, "session_ok"));
    }

    #[test]
    fn unknown_guard_name_fails_closed() {
        let ctx = Context::new();
        let chart = new_adstatechart(&ctx).expect("chart parses");
        assert!(chart.send(&ctx, "write", &guard_map(&facts_synced())));
        // Empty guard map: the `editor_synced` guard resolves to absent -> false.
        let empty = std::collections::HashMap::new();
        assert!(!chart.send(&ctx, "commit", &empty));
        assert!(chart.matches(&ctx, "written"));
    }

    /// Orthogonality: transport transitions are independent of closeout progress.
    #[test]
    fn transport_region_is_orthogonal_to_closeout() {
        let ctx = Context::new();
        let chart = new_adstatechart(&ctx).expect("chart parses");
        let g = guard_map(&facts_synced());

        // Drive closeout forward.
        assert!(chart.send(&ctx, "write", &g));
        // A transport event fires independently; closeout stays in `written`.
        assert!(chart.send(&ctx, "send_timeout", &g));
        assert!(chart.matches(&ctx, "degraded"));
        assert!(chart.matches(&ctx, "written"));
        // A missing endpoint stays degraded; recovery keeps the same intent.
        assert!(chart.send(&ctx, "no_ipc_listener", &g));
        assert!(chart.matches(&ctx, "degraded"));
        assert!(chart.matches(&ctx, "written"));
    }

    #[test]
    fn no_matching_transition_is_a_no_op() {
        let ctx = Context::new();
        let chart = new_adstatechart(&ctx).expect("chart parses");
        // `commit` is not valid from `idle`; nothing transitions.
        let before = chart.configuration(&ctx);
        assert!(!chart.send(&ctx, "commit", &guard_map(&facts_synced())));
        assert_eq!(before, chart.configuration(&ctx));
    }

    #[test]
    fn snapshot_initial_state_all_regions_initial() {
        let snap = configuration_snapshot(&facts_synced(), &ObservedPhases::default());
        assert_eq!(
            snap,
            "transport.socket editor_sync.synced closeout.idle supervisor.sup_idle"
        );
    }

    #[test]
    fn snapshot_reflects_transport_and_editor_sync_facts() {
        // Editor ahead + IPC send failed (degraded transport).
        let mut f = facts_editor_ahead();
        f.ipc_send_failed = true;
        let snap = configuration_snapshot(&f, &ObservedPhases::default());
        assert!(snap.contains("transport.degraded"), "got: {snap}");
        assert!(snap.contains("editor_sync.editor_ahead"), "got: {snap}");

        // No listener remains degraded; there is no hot-path file transport.
        f.ipc_no_listener = true;
        let snap = configuration_snapshot(&f, &ObservedPhases::default());
        assert!(snap.contains("transport.degraded"), "got: {snap}");
    }

    #[test]
    fn snapshot_closeout_walks_lifecycle_when_synced() {
        let f = facts_synced();
        let written = configuration_snapshot(
            &f,
            &ObservedPhases {
                closeout: CloseoutPhase::Written,
                ..Default::default()
            },
        );
        assert!(written.contains("closeout.written"), "got: {written}");

        let ok = configuration_snapshot(
            &f,
            &ObservedPhases {
                closeout: CloseoutPhase::SessionOk,
                ..Default::default()
            },
        );
        assert!(ok.contains("closeout.session_ok"), "got: {ok}");
    }

    /// The advisory read must not disagree with the shipped closeout guard: an
    /// observed `Committed` while the editor is ahead stalls at `written`.
    #[test]
    fn snapshot_committed_while_editor_ahead_stalls_at_written() {
        let snap = configuration_snapshot(
            &facts_editor_ahead(),
            &ObservedPhases {
                closeout: CloseoutPhase::Committed,
                ..Default::default()
            },
        );
        assert!(snap.contains("closeout.written"), "got: {snap}");
        assert!(!snap.contains("closeout.committed"), "got: {snap}");
    }

    #[test]
    fn snapshot_supervisor_stale_wins_over_busy() {
        let stale = ChartFacts {
            running_build_id: Some("old".into()),
            installed_build_id: Some("new".into()),
            ..facts_synced()
        };
        let snap = configuration_snapshot(
            &stale,
            &ObservedPhases {
                supervisor_busy: true,
                ..Default::default()
            },
        );
        assert!(snap.contains("supervisor.sup_stale"), "got: {snap}");

        // Busy without staleness → sup_busy.
        let busy = configuration_snapshot(
            &facts_synced(),
            &ObservedPhases {
                supervisor_busy: true,
                ..Default::default()
            },
        );
        assert!(busy.contains("supervisor.sup_busy"), "got: {busy}");
    }

    #[test]
    fn snapshot_region_order_is_stable() {
        // Every region present exactly once, in canonical order.
        let snap = configuration_snapshot(&facts_synced(), &ObservedPhases::default());
        let regions: Vec<&str> = snap
            .split(' ')
            .map(|l| l.split('.').next().unwrap())
            .collect();
        assert_eq!(
            regions,
            ["transport", "editor_sync", "closeout", "supervisor"]
        );
    }

    /// The typed [`ChartBuilder`] migration is behavior-preserving: the
    /// thread-safe chart projects the identical snapshot as the single-threaded
    /// one across a spread of fact/phase combinations.
    #[test]
    fn threadsafe_snapshot_matches_single_threaded() {
        let stale = ChartFacts {
            running_build_id: Some("old".into()),
            installed_build_id: Some("new".into()),
            ..facts_synced()
        };
        let cases = [
            (facts_synced(), ObservedPhases::default()),
            (
                facts_editor_ahead(),
                ObservedPhases {
                    closeout: CloseoutPhase::Committed,
                    ..Default::default()
                },
            ),
            (
                ChartFacts {
                    ipc_no_listener: true,
                    ..facts_synced()
                },
                ObservedPhases {
                    closeout: CloseoutPhase::SessionOk,
                    supervisor_busy: true,
                },
            ),
            (
                stale,
                ObservedPhases {
                    supervisor_busy: true,
                    ..Default::default()
                },
            ),
        ];
        for (facts, observed) in cases {
            assert_eq!(
                configuration_snapshot(&facts, &observed),
                configuration_snapshot_threadsafe(&facts, &observed),
                "single-threaded and thread-safe snapshots must agree for {facts:?} {observed:?}"
            );
        }
    }

    /// A [`ThreadSafeStateChart`] built from the typed def is `Send + Sync`: an
    /// observer thread can read `configuration()` after another drives `send()`.
    #[test]
    fn threadsafe_chart_observes_config_across_threads() {
        use lazily::ThreadSafeContext;
        let ctx = ThreadSafeContext::new();
        let chart = new_adstatechart_threadsafe(&ctx).expect("threadsafe chart builds");
        // Drive the closeout region forward on this thread.
        let g = guard_map(&facts_synced());
        assert!(chart.send(&ctx, "write", &g));
        // Read the configuration from a separate observer thread.
        let observed =
            std::thread::scope(|s| s.spawn(|| chart.matches(&ctx, "written")).join().unwrap());
        assert!(observed, "observer thread must see closeout.written");
    }

    #[test]
    fn supervisor_stale_edge_gated_on_proven_mismatch() {
        let ctx = Context::new();
        let chart = new_adstatechart(&ctx).expect("chart parses");
        // Unknown build ids: stale_observed is rejected (fail open to not-stale).
        assert!(!chart.send(&ctx, "stale_observed", &guard_map(&ChartFacts::default())));
        assert!(chart.matches(&ctx, "sup_idle"));
        // Proven mismatch: the edge fires.
        let stale = ChartFacts {
            running_build_id: Some("old".into()),
            installed_build_id: Some("new".into()),
            ..Default::default()
        };
        assert!(chart.send(&ctx, "stale_observed", &guard_map(&stale)));
        assert!(chart.matches(&ctx, "sup_stale"));
    }
}
