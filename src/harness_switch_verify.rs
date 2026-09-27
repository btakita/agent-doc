//! # Module: harness_switch_verify
//!
//! Verify a LIVE authoritative-harness switch from its ops.log receipts
//! (`#hswdisposabledoc`).
//!
//! ## Spec
//! - A switch leaves an ordered receipt trail: `harness_change_detected` (with the
//!   boundary gate verdict), then `agent_restart_triggered`, then exactly ONE
//!   `agent_restart_performed ... action=spawn_fresh_harness` per switch.
//! - The invariant under test is a COUNT, not a presence: one spawn per switch. A
//!   presence check passes a respawn storm, which is the failure mode the
//!   deterministic scenario
//!   `route_sim_consecutive_harness_switches_spawn_exactly_once_each_with_no_storm`
//!   guards offline and this command guards live.
//!
//! ## Agentic Contracts
//! - This owns the counting so the operator script stays a scaffold-and-print shell
//!   (all deterministic behavior in the binary). A shell `grep -q` cannot express
//!   "exactly one per switch" and inverts on a match, which is how a measurement
//!   read from an exit status false-greens.
//! - An absent receipt class is reported as unobserved, never as a pass.

use agent_doc_turn::op_log::strip_timestamp_prefix;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

const DETECTED: &str = "harness_change_detected";
const TRIGGERED: &str = "agent_restart_triggered";
const PERFORMED: &str = "agent_restart_performed";
const SPAWN_ACTION: &str = "action=spawn_fresh_harness";

/// One `agent_restart_performed` receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SpawnReceipt {
    pub old_harness: String,
    pub new_harness: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessSwitchVerification {
    pub doc_tag: String,
    pub ops_log: PathBuf,
    pub detected_count: usize,
    pub triggered_count: usize,
    pub spawns: Vec<SpawnReceipt>,
}

pub fn run(file: &Path, expect_switches: Option<usize>) -> Result<()> {
    let report = verify(file, expect_switches)?;
    println!("harness-switch verification ok for {}", file.display());
    println!("ops_log={}", report.ops_log.display());
    println!(
        "doc={} harness_change_detected={} agent_restart_triggered={} spawns={}",
        report.doc_tag,
        report.detected_count,
        report.triggered_count,
        report.spawns.len()
    );
    for (index, spawn) in report.spawns.iter().enumerate() {
        println!(
            "  spawn {}: {} -> {}",
            index + 1,
            spawn.old_harness,
            spawn.new_harness
        );
    }
    Ok(())
}

fn verify(file: &Path, expect_switches: Option<usize>) -> Result<HarnessSwitchVerification> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let project_root = agent_doc_fs::find_project_root(&canonical)
        .with_context(|| format!("no .agent-doc project root found for {}", file.display()))?;
    let ops_log = project_root.join(".agent-doc/logs/ops.log");
    let log = std::fs::read_to_string(&ops_log)
        .with_context(|| format!("failed to read {}", ops_log.display()))?;

    let stem = canonical
        .file_stem()
        .and_then(|name| name.to_str())
        .context("document path has no UTF-8 file stem")?;
    let doc_tag = format!("doc={stem}");
    let doc_lines: Vec<&str> = log.lines().filter(|line| line.contains(&doc_tag)).collect();

    let detected: Vec<&str> = doc_lines
        .iter()
        .copied()
        .filter(|line| line.contains(DETECTED))
        .collect();
    if detected.is_empty() {
        bail!(
            "no `{DETECTED}` receipt for {doc_tag} in {} — the switch was never observed. \
             Edit the frontmatter `agent:` line and leave the buffer UNSAVED, then wait for a \
             quiet dispatch-ready prompt boundary before verifying.",
            ops_log.display()
        );
    }

    let triggered = doc_lines
        .iter()
        .filter(|line| line.contains(TRIGGERED))
        .count();
    if triggered == 0 {
        let gates: Vec<&str> = detected
            .iter()
            .filter_map(|line| {
                line.split_whitespace()
                    .find_map(|field| field.strip_prefix("gate="))
            })
            .collect();
        bail!(
            "{} `{DETECTED}` receipt(s) for {doc_tag} but no `{TRIGGERED}` — the switch was \
             detected and is HELD pending, not dropped. Observed gate verdict(s): {}. \
             `WaitForBoundary` means no quiet dispatch-ready boundary was reached (a busy turn \
             or paused queue); `None` means agent_change_restart is disabled.",
            detected.len(),
            if gates.is_empty() {
                "none recorded".to_string()
            } else {
                gates.join(", ")
            }
        );
    }

    let spawns = collect_spawns(&doc_lines);
    if spawns.is_empty() {
        bail!(
            "{triggered} `{TRIGGERED}` receipt(s) for {doc_tag} but no \
             `{PERFORMED} ... {SPAWN_ACTION}` — the restart was requested and never completed."
        );
    }

    // The invariant is one spawn per switch. Presence alone passes a respawn storm.
    if let Some(expected) = expect_switches
        && spawns.len() != expected
    {
        bail!(
            "expected exactly {expected} spawn(s) for {doc_tag} (one per switch) but found {}: {}. \
             More than one spawn per switch is a respawn storm; fewer means a switch never \
             completed.",
            spawns.len(),
            render_spawns(&spawns)
        );
    }
    if let Some(index) = spawns
        .iter()
        .position(|spawn| spawn.old_harness == spawn.new_harness)
    {
        bail!(
            "spawn {} for {doc_tag} reports the same old and new harness (`{}`), which is a \
             respawn of the harness already running, not a switch: {}",
            index + 1,
            spawns[index].old_harness,
            render_spawns(&spawns)
        );
    }

    Ok(HarnessSwitchVerification {
        doc_tag,
        ops_log,
        detected_count: detected.len(),
        triggered_count: triggered,
        spawns,
    })
}

fn collect_spawns(doc_lines: &[&str]) -> Vec<SpawnReceipt> {
    doc_lines
        .iter()
        .map(|line| strip_timestamp_prefix(line))
        .filter(|line| line.contains(PERFORMED) && line.contains(SPAWN_ACTION))
        .filter_map(|line| {
            let mut old_harness = None;
            let mut new_harness = None;
            for field in line.split_whitespace() {
                if let Some(value) = field.strip_prefix("old_harness=") {
                    old_harness = Some(value.to_string());
                } else if let Some(value) = field.strip_prefix("new_harness=") {
                    new_harness = Some(value.to_string());
                }
            }
            Some(SpawnReceipt {
                old_harness: old_harness?,
                new_harness: new_harness?,
            })
        })
        .collect()
}

fn render_spawns(spawns: &[SpawnReceipt]) -> String {
    spawns
        .iter()
        .map(|spawn| format!("{}->{}", spawn.old_harness, spawn.new_harness))
        .collect::<Vec<_>>()
        .join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_log(log: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("throwaway.md");
        std::fs::write(&doc, "# throwaway\n").unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(dir.path().join(".agent-doc/logs/ops.log"), log).unwrap();
        (dir, doc)
    }

    const ONE_SWITCH: &str = "[2026-09-27T00:00:00Z] harness_change_detected old=codex new=claude gate=Restart doc=throwaway\n\
         [2026-09-27T00:00:01Z] agent_restart_triggered old=codex new=claude action=request_fresh_restart doc=throwaway\n\
         [2026-09-27T00:00:02Z] agent_restart_performed old_harness=codex new_harness=claude action=spawn_fresh_harness doc=throwaway\n";

    #[test]
    fn verify_accepts_one_spawn_for_one_switch() {
        let (_dir, doc) = setup_log(ONE_SWITCH);
        let report = verify(&doc, Some(1)).unwrap();
        assert_eq!(report.detected_count, 1);
        assert_eq!(report.triggered_count, 1);
        assert_eq!(
            report.spawns,
            vec![SpawnReceipt {
                old_harness: "codex".into(),
                new_harness: "claude".into(),
            }]
        );
    }

    /// The point of the command: a COUNT, not a presence check. A presence check
    /// passes a respawn storm.
    #[test]
    fn verify_rejects_a_respawn_storm_a_presence_check_would_pass() {
        let storm = format!(
            "{ONE_SWITCH}[2026-09-27T00:00:03Z] agent_restart_performed old_harness=codex new_harness=claude action=spawn_fresh_harness doc=throwaway\n"
        );
        let (_dir, doc) = setup_log(&storm);
        // Presence is satisfied either way, so an unbounded verify still passes.
        assert_eq!(verify(&doc, None).unwrap().spawns.len(), 2);
        let err = verify(&doc, Some(1)).unwrap_err().to_string();
        assert!(
            err.contains("expected exactly 1 spawn(s)") && err.contains("respawn storm"),
            "two spawns for one switch must be reported as a storm: {err}"
        );
    }

    #[test]
    fn verify_rejects_a_spawn_of_the_harness_already_running() {
        let (_dir, doc) = setup_log(
            "[2026-09-27T00:00:00Z] harness_change_detected old=codex new=claude gate=Restart doc=throwaway\n\
             [2026-09-27T00:00:01Z] agent_restart_triggered old=codex new=claude action=request_fresh_restart doc=throwaway\n\
             [2026-09-27T00:00:02Z] agent_restart_performed old_harness=codex new_harness=codex action=spawn_fresh_harness doc=throwaway\n",
        );
        let err = verify(&doc, None).unwrap_err().to_string();
        assert!(
            err.contains("same old and new harness"),
            "a no-op respawn must not count as a switch: {err}"
        );
    }

    /// A held switch is a DIFFERENT diagnosis from a dropped one, and the gate
    /// verdict names which boundary condition held it.
    #[test]
    fn verify_reports_a_held_switch_with_its_gate_verdict() {
        let (_dir, doc) = setup_log(
            "[2026-09-27T00:00:00Z] harness_change_detected old=codex new=claude gate=WaitForBoundary doc=throwaway\n",
        );
        let err = verify(&doc, None).unwrap_err().to_string();
        assert!(
            err.contains("HELD pending, not dropped") && err.contains("WaitForBoundary"),
            "a held switch must name its gate verdict: {err}"
        );
    }

    #[test]
    fn verify_reports_a_triggered_switch_that_never_spawned() {
        let (_dir, doc) = setup_log(
            "[2026-09-27T00:00:00Z] harness_change_detected old=codex new=claude gate=Restart doc=throwaway\n\
             [2026-09-27T00:00:01Z] agent_restart_triggered old=codex new=claude action=request_fresh_restart doc=throwaway\n",
        );
        let err = verify(&doc, None).unwrap_err().to_string();
        assert!(
            err.contains("never completed"),
            "a requested restart with no spawn must be reported: {err}"
        );
    }

    #[test]
    fn verify_reports_an_unobserved_switch_rather_than_passing() {
        let (_dir, doc) = setup_log(
            "[2026-09-27T00:00:00Z] preflight_diff_start doc=throwaway\n",
        );
        let err = verify(&doc, None).unwrap_err().to_string();
        assert!(
            err.contains("never observed") && err.contains("UNSAVED"),
            "an absent receipt class must name the operator step that produces it: {err}"
        );
    }

    /// Receipts for another document must not be counted.
    #[test]
    fn verify_counts_only_this_documents_receipts() {
        let mixed = format!(
            "{ONE_SWITCH}[2026-09-27T00:00:04Z] agent_restart_performed old_harness=claude new_harness=codex action=spawn_fresh_harness doc=someone-else\n"
        );
        let (_dir, doc) = setup_log(&mixed);
        assert_eq!(
            verify(&doc, Some(1)).unwrap().spawns.len(),
            1,
            "another document's spawn must not inflate this document's count"
        );
    }
}
