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

use lazily::{ChartDef, StateChart};
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

/// No socket listener owns the project, so writes fall back to the file signal.
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
/// match the `guard` fields in [`CHART_DEF_JSON`]. Any name the chart references
/// but this map omits fails closed inside `StateChart::send`.
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

/// The declarative Harel chart: four orthogonal regions under a parallel root.
///
/// - `transport`:   `socket → degraded → file_fallback` (+ recovery back)
/// - `editor_sync`: `synced → editor_ahead → publishing`
/// - `closeout`:    `idle → written → committed → session_ok` (final)
/// - `supervisor`:  `sup_idle / sup_busy / sup_stale → sup_recycle`
///
/// The `closeout.written --commit--> committed` edge carries guard
/// `editor_synced`, making commit-while-editor-ahead a rejected edge.
pub const CHART_DEF_JSON: &str = r#"{
  "initial": "root",
  "states": {
    "root": { "parallel": true },

    "transport": { "parent": "root", "initial": "socket" },
    "socket": { "parent": "transport", "on": {
      "send_timeout": { "target": "degraded" },
      "no_ack": { "target": "degraded" },
      "no_ipc_listener": { "target": "file_fallback" }
    } },
    "degraded": { "parent": "transport", "on": {
      "no_ipc_listener": { "target": "file_fallback" },
      "socket_recovered": { "target": "socket" }
    } },
    "file_fallback": { "parent": "transport", "on": {
      "socket_recovered": { "target": "socket" }
    } },

    "editor_sync": { "parent": "root", "initial": "synced" },
    "synced": { "parent": "editor_sync", "on": {
      "editor_edited": { "target": "editor_ahead", "guard": "editor_ahead" }
    } },
    "editor_ahead": { "parent": "editor_sync", "on": {
      "publish_started": { "target": "publishing" },
      "editor_resynced": { "target": "synced", "guard": "editor_synced" }
    } },
    "publishing": { "parent": "editor_sync", "on": {
      "publish_acked": { "target": "synced", "guard": "editor_synced" }
    } },

    "closeout": { "parent": "root", "initial": "idle" },
    "idle": { "parent": "closeout", "on": {
      "write": { "target": "written" }
    } },
    "written": { "parent": "closeout", "on": {
      "commit": { "target": "committed", "guard": "editor_synced" }
    } },
    "committed": { "parent": "closeout", "on": {
      "session_check_ok": { "target": "session_ok" }
    } },
    "session_ok": { "parent": "closeout", "kind": "final" },

    "supervisor": { "parent": "root", "initial": "sup_idle" },
    "sup_idle": { "parent": "supervisor", "on": {
      "turn_started": { "target": "sup_busy" },
      "stale_observed": { "target": "sup_stale", "guard": "supervisor_stale" }
    } },
    "sup_busy": { "parent": "supervisor", "on": {
      "turn_ended": { "target": "sup_idle" },
      "stale_observed": { "target": "sup_stale", "guard": "supervisor_stale" }
    } },
    "sup_stale": { "parent": "supervisor", "on": {
      "recycle_requested": { "target": "sup_recycle" }
    } },
    "sup_recycle": { "parent": "supervisor", "on": {
      "recycled": { "target": "sup_idle" }
    } }
  }
}"#;

/// Parse [`CHART_DEF_JSON`] into a [`ChartDef`]. Returns the parser error string
/// on a malformed chart or an unsupported feature (`run` action / `expr` guard).
pub fn adstatechart_def() -> Result<ChartDef, String> {
    let value: serde_json::Value = serde_json::from_str(CHART_DEF_JSON)
        .map_err(|e| format!("adstatechart CHART_DEF_JSON is not valid JSON: {e}"))?;
    ChartDef::from_json(&value)
}

/// Build a fresh [`StateChart`] over `ctx` in its initial configuration.
pub fn new_adstatechart(ctx: &lazily::Context) -> Result<StateChart, String> {
    Ok(StateChart::new(ctx, adstatechart_def()?))
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
        // Transport degrades further to file fallback.
        assert!(chart.send(&ctx, "no_ipc_listener", &g));
        assert!(chart.matches(&ctx, "file_fallback"));
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
