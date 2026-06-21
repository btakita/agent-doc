//! `#fullboundary` Fix 2 — loud force-disk fallback diagnostics (operation playback).
//!
//! When the convergence boundary ([`crate::convergence_gate`]) cannot be satisfied
//! within its bounded timeout (editor IPC wedged/dead — the `inflight=5` /
//! `send_failed` state), the closeout MAY fall back to a direct `--force-disk` write
//! so work is never permanently stuck. Per the operator directive, that fallback is
//! an **error condition, not a routine path**: it must be loud and fully diagnosable.
//!
//! This module writes a replayable **operation playback** artifact capturing the full
//! sequence that led to the wedge — the ordered IPC attempt sequence, inflight,
//! snapshot/baseline/HEAD hashes, candidate vs `content_ours` lengths/hashes, the
//! cycle/run/actor/supervisor identity, and the closeout state-machine transitions —
//! to `.agent-doc/playback/<doc-hash>/<cycle-id>.json`, and emits an ERROR-level
//! `ops.log` line referencing it so `session-check` / a recovering agent can load it
//! directly instead of guessing from a generic `send_failed`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// One editor-IPC delivery attempt in the ordered sequence that led to the wedge.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct IpcAttempt {
    /// Patch id for the attempt.
    pub patch_id: String,
    /// Transport used: `socket` or `file`.
    pub transport: String,
    /// ACK outcome: `ok`, `timeout`, `send_failed`, `no_ack`, …
    pub ack: String,
    /// Unix-seconds timestamp of the attempt (stamped by the caller — this module
    /// performs no wall-clock reads so it stays deterministic under test/replay).
    pub ts: u64,
    /// Inflight count observed at this attempt (per-attempt backpressure).
    pub inflight: u64,
}

/// The full replayable diagnostic for a force-disk fallback at a failed convergence
/// boundary. Serialized to `.agent-doc/playback/<doc-hash>/<cycle-id>.json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConvergencePlayback {
    /// Schema version for forward-compatible readers.
    pub schema_version: u32,
    /// Canonical document path.
    pub document: String,
    /// Cycle id this fallback occurred in.
    pub cycle_id: String,
    /// Pipeline run id, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    /// Authoritative actor generation, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actor_generation: Option<u64>,
    /// Supervisor pid that owned the boundary, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervisor_pid: Option<u32>,
    /// Whether the supervisor binary matched the installed build (freshness).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub supervisor_binary_fresh: Option<bool>,
    /// Inflight count at the moment the fallback tripped.
    pub inflight: u64,
    /// Bounded timeout (ms) that was exceeded.
    pub timeout_ms: u64,
    /// Convergence proofs that never satisfied (from [`crate::convergence_gate`]).
    pub unmet_proofs: Vec<String>,
    /// Ordered IPC attempt sequence (patch_id, transport, ack, ts, inflight).
    pub attempts: Vec<IpcAttempt>,
    /// Snapshot content hash, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    /// Baseline content hash, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub baseline_hash: Option<String>,
    /// HEAD blob hash, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub head_hash: Option<String>,
    /// Candidate (response) length, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_len: Option<usize>,
    /// Candidate (response) content hash, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_hash: Option<String>,
    /// `content_ours` length, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_ours_len: Option<usize>,
    /// `content_ours` content hash, if known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content_ours_hash: Option<String>,
    /// Closeout state-machine transitions (preflight → write → commit →
    /// postcommit_worktree_check → fallback), in order.
    pub state_transitions: Vec<String>,
}

impl ConvergencePlayback {
    /// Construct a playback record with the required identity + outcome fields; the
    /// optional diagnostic fields default to absent and are filled via the builder
    /// setters below.
    pub fn new(
        document: impl Into<String>,
        cycle_id: impl Into<String>,
        inflight: u64,
        timeout_ms: u64,
        unmet_proofs: Vec<String>,
    ) -> Self {
        Self {
            schema_version: 1,
            document: document.into(),
            cycle_id: cycle_id.into(),
            run_id: None,
            actor_generation: None,
            supervisor_pid: None,
            supervisor_binary_fresh: None,
            inflight,
            timeout_ms,
            unmet_proofs,
            attempts: Vec::new(),
            snapshot_hash: None,
            baseline_hash: None,
            head_hash: None,
            candidate_len: None,
            candidate_hash: None,
            content_ours_len: None,
            content_ours_hash: None,
            state_transitions: Vec::new(),
        }
    }

    pub fn with_attempts(mut self, attempts: Vec<IpcAttempt>) -> Self {
        self.attempts = attempts;
        self
    }

    pub fn with_state_transitions(mut self, transitions: Vec<String>) -> Self {
        self.state_transitions = transitions;
        self
    }

    pub fn with_run_id(mut self, run_id: Option<String>) -> Self {
        self.run_id = run_id;
        self
    }

    pub fn with_actor_generation(mut self, generation: Option<u64>) -> Self {
        self.actor_generation = generation;
        self
    }

    pub fn with_supervisor(mut self, pid: Option<u32>, binary_fresh: Option<bool>) -> Self {
        self.supervisor_pid = pid;
        self.supervisor_binary_fresh = binary_fresh;
        self
    }

    pub fn with_hashes(
        mut self,
        snapshot_hash: Option<String>,
        baseline_hash: Option<String>,
        head_hash: Option<String>,
    ) -> Self {
        self.snapshot_hash = snapshot_hash;
        self.baseline_hash = baseline_hash;
        self.head_hash = head_hash;
        self
    }

    pub fn with_candidate(
        mut self,
        candidate_len: Option<usize>,
        candidate_hash: Option<String>,
        content_ours_len: Option<usize>,
        content_ours_hash: Option<String>,
    ) -> Self {
        self.candidate_len = candidate_len;
        self.candidate_hash = candidate_hash;
        self.content_ours_len = content_ours_len;
        self.content_ours_hash = content_ours_hash;
        self
    }
}

/// Resolve the playback artifact path for a document + cycle without writing.
/// `.agent-doc/playback/<doc-hash>/<cycle-id>.json`, mirroring the captures layout.
pub fn playback_artifact_path(file: &Path, cycle_id: &str) -> Result<PathBuf> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let hash = crate::snapshot::doc_hash(&canonical)?;
    let project_root = crate::snapshot::find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let dir = project_root.join(".agent-doc/playback").join(hash);
    Ok(dir.join(format!("{cycle_id}.json")))
}

/// Persist the playback artifact to `.agent-doc/playback/<doc-hash>/<cycle-id>.json`
/// and return its path. The directory is created if absent.
pub fn write_convergence_playback(file: &Path, playback: &ConvergencePlayback) -> Result<PathBuf> {
    let path = playback_artifact_path(file, &playback.cycle_id)?;
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create playback dir {}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(playback)
        .context("failed to serialize convergence playback")?;
    std::fs::write(&path, json)
        .with_context(|| format!("failed to write playback artifact {}", path.display()))?;
    Ok(path)
}

/// The ERROR-level ops.log line for a force-disk fallback at a failed convergence
/// boundary. Greppable (`severity=error`) and references the playback artifact so a
/// recovering agent can load the full sequence directly.
pub fn force_disk_fallback_ops_message(
    file: &Path,
    playback: &ConvergencePlayback,
    playback_path: &Path,
) -> String {
    format!(
        "convergence_gate_force_disk_fallback severity=error file={} reason=editor_ipc_convergence_boundary_failed cycle={} inflight={} timeout_ms={} unmet={} playback={}",
        file.display(),
        playback.cycle_id,
        playback.inflight,
        playback.timeout_ms,
        playback.unmet_proofs.join(","),
        playback_path.display(),
    )
}

/// Write the playback artifact AND emit the ERROR-level ops.log line referencing it.
/// Best-effort on the ops.log side (it never fails), but the artifact write is
/// surfaced so a failure to persist diagnostics is visible rather than swallowed.
pub fn record_force_disk_fallback(file: &Path, playback: &ConvergencePlayback) -> Result<PathBuf> {
    let path = write_convergence_playback(file, playback)?;
    crate::ops_log::log_op(
        file,
        &force_disk_fallback_ops_message(file, playback, &path),
    );
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_playback() -> ConvergencePlayback {
        ConvergencePlayback::new(
            "/tmp/doc.md",
            "cycle-123",
            5,
            5_000,
            vec![
                "editor_converged".to_string(),
                "inflight_drained".to_string(),
            ],
        )
        .with_attempts(vec![
            IpcAttempt {
                patch_id: "p1".into(),
                transport: "socket".into(),
                ack: "ok".into(),
                ts: 1_000,
                inflight: 1,
            },
            IpcAttempt {
                patch_id: "p2".into(),
                transport: "socket".into(),
                ack: "send_failed".into(),
                ts: 1_005,
                inflight: 5,
            },
        ])
        .with_state_transitions(vec![
            "preflight".into(),
            "write".into(),
            "commit".into(),
            "postcommit_worktree_check:match=false".into(),
            "force_disk_fallback".into(),
        ])
        .with_hashes(
            Some("snap".into()),
            Some("base".into()),
            Some("head".into()),
        )
        .with_candidate(
            Some(40_279),
            Some("cand".into()),
            Some(39_011),
            Some("ours".into()),
        )
    }

    #[test]
    fn playback_round_trips_through_json() {
        let pb = sample_playback();
        let json = serde_json::to_string(&pb).unwrap();
        let back: ConvergencePlayback = serde_json::from_str(&json).unwrap();
        assert_eq!(pb, back);
        // The attempt sequence, hashes, and transitions must survive — the whole
        // point is a faithful replay.
        assert_eq!(back.attempts.len(), 2);
        assert_eq!(back.attempts[1].ack, "send_failed");
        assert_eq!(back.attempts[1].inflight, 5);
        assert_eq!(back.candidate_len, Some(40_279));
        assert!(
            back.state_transitions
                .contains(&"force_disk_fallback".to_string())
        );
    }

    #[test]
    fn ops_message_is_error_level_and_references_playback() {
        let pb = sample_playback();
        let path = Path::new("/tmp/.agent-doc/playback/abc/cycle-123.json");
        let msg = force_disk_fallback_ops_message(Path::new("/tmp/doc.md"), &pb, path);
        assert!(msg.contains("severity=error"));
        assert!(msg.contains("reason=editor_ipc_convergence_boundary_failed"));
        assert!(msg.contains("unmet=editor_converged,inflight_drained"));
        assert!(msg.contains("playback=/tmp/.agent-doc/playback/abc/cycle-123.json"));
        assert!(msg.contains("inflight=5"));
    }

    #[test]
    fn write_then_read_back_artifact_in_tempdir() {
        let tmp =
            std::env::temp_dir().join(format!("agentdoc-playback-test-{}", std::process::id()));
        let agent_doc = tmp.join(".agent-doc");
        std::fs::create_dir_all(&agent_doc).unwrap();
        let doc = tmp.join("doc.md");
        std::fs::write(&doc, "# doc\n").unwrap();

        let pb = ConvergencePlayback::new(
            doc.to_string_lossy().to_string(),
            "cycle-xyz",
            5,
            5_000,
            vec!["editor_converged".to_string()],
        );
        let path = write_convergence_playback(&doc, &pb).unwrap();
        assert!(path.exists(), "artifact should be written to disk");
        let content = std::fs::read_to_string(&path).unwrap();
        let back: ConvergencePlayback = serde_json::from_str(&content).unwrap();
        assert_eq!(back.cycle_id, "cycle-xyz");
        assert_eq!(back.unmet_proofs, vec!["editor_converged".to_string()]);

        let _ = std::fs::remove_dir_all(&tmp);
    }
}
