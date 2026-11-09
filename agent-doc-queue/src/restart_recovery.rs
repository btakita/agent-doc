//! Bounded restart-recovery projection (`#restartjournal`).
//!
//! Queue claims, queue lineage, and the supervisor session lineage remain the
//! transition authorities. This module owns the compact, typed projection that
//! cold-start hydration uses after an editor, harness, or OS restart. It never
//! stores prompts, responses, tool output, or repository payloads; large
//! artifacts are referenced by content hash or Git commit only.

use serde::{Deserialize, Serialize};

pub const RESTART_JOURNAL_CONTRACT: &str = "agent-doc-restart-journal-v1";
pub const DEFAULT_MAX_TRANSITIONS: usize = 64;
pub const DEFAULT_RECENT_GENERATIONS: usize = 3;
pub const DEFAULT_COMPACT_EVERY: usize = 16;
pub const DEFAULT_TERMINAL_RETENTION_SECS: u64 = 6 * 60 * 60;
pub const DEFAULT_DOCUMENT_BYTE_CAP: usize = 64 * 1024;
pub const DEFAULT_GLOBAL_BYTE_CAP: usize = 8 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RestartJournalLimits {
    pub max_transitions: usize,
    pub recent_generations: usize,
    pub compact_every: usize,
    pub terminal_retention_secs: u64,
    pub document_byte_cap: usize,
    pub global_byte_cap: usize,
}

impl Default for RestartJournalLimits {
    fn default() -> Self {
        Self {
            max_transitions: DEFAULT_MAX_TRANSITIONS,
            recent_generations: DEFAULT_RECENT_GENERATIONS,
            compact_every: DEFAULT_COMPACT_EVERY,
            terminal_retention_secs: DEFAULT_TERMINAL_RETENTION_SECS,
            document_byte_cap: DEFAULT_DOCUMENT_BYTE_CAP,
            global_byte_cap: DEFAULT_GLOBAL_BYTE_CAP,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MainAgentPhase {
    Starting,
    Running,
    WaitingInput,
    CloseoutPending,
    Completed,
    NeedsOperator,
}

impl MainAgentPhase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Completed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerPhase {
    Dispatching,
    Running,
    Reported,
    Lost,
    QuotaBlocked,
    Cancelled,
    Completed,
}

impl WorkerPhase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Cancelled | Self::Completed)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RecoveryAction {
    None,
    Reattach,
    ResumeMain,
    Redispatch,
    IntegrateArtifacts,
    PreserveReceipt,
    NeedsOperator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArtifactReference {
    /// Hash of the external locator (worktree/repository/ref), never its body.
    pub locator_hash: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git_commit: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MainAgentCheckpoint {
    pub harness: String,
    /// Supervisor/process identity that may advance this checkpoint. The
    /// closeout writer compares it before marking the row terminal so a stale
    /// predecessor cannot overwrite a replacement supervisor.
    #[serde(default)]
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_id: Option<String>,
    pub boot_id: String,
    pub phase: MainAgentPhase,
    pub updated_at_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerCheckpoint {
    /// Hash of the queue identity. The authoritative row remains in the claim
    /// ledger; this projection deliberately does not duplicate operator text.
    pub identity_hash: String,
    pub generation: u64,
    pub owner: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    pub boot_id: String,
    pub phase: WorkerPhase,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ArtifactReference>,
    pub updated_at_secs: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "subject", rename_all = "snake_case")]
pub enum RestartTransitionSubject {
    Main {
        phase: MainAgentPhase,
    },
    Worker {
        identity_hash: String,
        generation: u64,
        phase: WorkerPhase,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartTransition {
    pub sequence: u64,
    pub at_secs: u64,
    #[serde(flatten)]
    pub subject: RestartTransitionSubject,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartJournal {
    #[serde(default = "contract_version")]
    pub contract_version: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<MainAgentCheckpoint>,
    #[serde(default)]
    pub workers: Vec<WorkerCheckpoint>,
    #[serde(default)]
    pub transitions: Vec<RestartTransition>,
    #[serde(default)]
    pub next_sequence: u64,
    #[serde(default)]
    pub compacted_through: u64,
    #[serde(default)]
    pub updated_at_secs: u64,
}

fn contract_version() -> String {
    RESTART_JOURNAL_CONTRACT.to_string()
}

impl Default for RestartJournal {
    fn default() -> Self {
        Self {
            contract_version: contract_version(),
            main: None,
            workers: Vec::new(),
            transitions: Vec::new(),
            next_sequence: 1,
            compacted_through: 0,
            updated_at_secs: 0,
        }
    }
}

impl RestartJournal {
    pub fn record_main(&mut self, checkpoint: MainAgentCheckpoint, limits: RestartJournalLimits) {
        // The transition log records lifecycle edges, not heartbeat/metadata
        // refreshes. Current owner/resume/time still update in the snapshot.
        let changed = self
            .main
            .as_ref()
            .is_none_or(|current| current.phase != checkpoint.phase);
        self.updated_at_secs = self.updated_at_secs.max(checkpoint.updated_at_secs);
        if changed {
            self.push_transition(
                checkpoint.updated_at_secs,
                RestartTransitionSubject::Main {
                    phase: checkpoint.phase,
                },
            );
        }
        self.main = Some(checkpoint);
        self.compact(limits, self.updated_at_secs);
    }

    pub fn record_worker(&mut self, checkpoint: WorkerCheckpoint, limits: RestartJournalLimits) {
        let position = self.workers.iter().position(|worker| {
            worker.identity_hash == checkpoint.identity_hash
                && worker.generation == checkpoint.generation
        });
        let changed = position
            .and_then(|index| self.workers.get(index))
            .is_none_or(|current| current.phase != checkpoint.phase);
        self.updated_at_secs = self.updated_at_secs.max(checkpoint.updated_at_secs);
        if changed {
            self.push_transition(
                checkpoint.updated_at_secs,
                RestartTransitionSubject::Worker {
                    identity_hash: checkpoint.identity_hash.clone(),
                    generation: checkpoint.generation,
                    phase: checkpoint.phase,
                },
            );
        }
        match position {
            Some(index) => self.workers[index] = checkpoint,
            None => self.workers.push(checkpoint),
        }
        self.compact(limits, self.updated_at_secs);
    }

    pub fn reconcile(&self, current_boot_id: &str) -> RestartRecoveryProjection {
        let main = self.main.as_ref().map(|checkpoint| {
            let action = if checkpoint.phase.terminal() {
                RecoveryAction::None
            } else if checkpoint.boot_id == current_boot_id {
                RecoveryAction::Reattach
            } else if checkpoint.resume_id.is_some() {
                RecoveryAction::ResumeMain
            } else {
                RecoveryAction::NeedsOperator
            };
            MainRecovery {
                phase: checkpoint.phase,
                action,
                resume_id: checkpoint.resume_id.clone(),
            }
        });
        let workers = self
            .workers
            .iter()
            .filter_map(|checkpoint| {
                let action = match checkpoint.phase {
                    WorkerPhase::Completed | WorkerPhase::Cancelled => {
                        RecoveryAction::PreserveReceipt
                    }
                    WorkerPhase::Reported => RecoveryAction::PreserveReceipt,
                    WorkerPhase::Lost => RecoveryAction::Redispatch,
                    WorkerPhase::Dispatching if checkpoint.boot_id != current_boot_id => {
                        RecoveryAction::Redispatch
                    }
                    WorkerPhase::Running if checkpoint.boot_id != current_boot_id => {
                        if checkpoint.artifact.is_some() {
                            RecoveryAction::IntegrateArtifacts
                        } else {
                            RecoveryAction::NeedsOperator
                        }
                    }
                    WorkerPhase::QuotaBlocked => RecoveryAction::NeedsOperator,
                    WorkerPhase::Dispatching | WorkerPhase::Running => RecoveryAction::Reattach,
                };
                (action != RecoveryAction::None).then(|| WorkerRecovery {
                    identity_hash: checkpoint.identity_hash.clone(),
                    generation: checkpoint.generation,
                    phase: checkpoint.phase,
                    action,
                    worker_id: checkpoint.worker_id.clone(),
                    artifact: checkpoint.artifact.clone(),
                })
            })
            .collect();
        RestartRecoveryProjection { main, workers }
    }

    pub fn compact(&mut self, limits: RestartJournalLimits, now_secs: u64) {
        self.workers.retain(|worker| {
            !worker.phase.terminal()
                || now_secs.saturating_sub(worker.updated_at_secs) <= limits.terminal_retention_secs
        });
        self.workers.sort_by(|left, right| {
            left.identity_hash
                .cmp(&right.identity_hash)
                .then_with(|| right.generation.cmp(&left.generation))
        });
        let mut previous_identity = String::new();
        let mut kept_for_identity = 0usize;
        self.workers.retain(|worker| {
            if worker.identity_hash != previous_identity {
                previous_identity.clone_from(&worker.identity_hash);
                kept_for_identity = 0;
            }
            kept_for_identity += 1;
            kept_for_identity <= limits.recent_generations.max(1)
        });
        if self.transitions.len() >= limits.compact_every.max(1) {
            let retain = limits.max_transitions.max(1);
            if self.transitions.len() > retain {
                let drop_count = self.transitions.len() - retain;
                if let Some(last) = self.transitions.get(drop_count - 1) {
                    self.compacted_through = self.compacted_through.max(last.sequence);
                }
                self.transitions.drain(..drop_count);
            }
        }
    }

    /// Enforce the serialized per-document cap without dropping live recovery
    /// state. Old transitions go first, then terminal worker snapshots. If the
    /// live snapshot alone exceeds the cap, return `false` and let the I/O edge
    /// fail closed instead of truncating resumable work.
    pub fn enforce_serialized_cap(&mut self, limits: RestartJournalLimits) -> bool {
        while serialized_len(self) > limits.document_byte_cap && !self.transitions.is_empty() {
            let removed = self.transitions.remove(0);
            self.compacted_through = self.compacted_through.max(removed.sequence);
        }
        while serialized_len(self) > limits.document_byte_cap {
            let Some(index) = self
                .workers
                .iter()
                .position(|worker| worker.phase.terminal())
            else {
                return false;
            };
            self.workers.remove(index);
        }
        true
    }

    fn push_transition(&mut self, at_secs: u64, subject: RestartTransitionSubject) {
        let sequence = self.next_sequence.max(1);
        self.next_sequence = sequence.saturating_add(1);
        self.transitions.push(RestartTransition {
            sequence,
            at_secs,
            subject,
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MainRecovery {
    pub phase: MainAgentPhase,
    pub action: RecoveryAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resume_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkerRecovery {
    pub identity_hash: String,
    pub generation: u64,
    pub phase: WorkerPhase,
    pub action: RecoveryAction,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub worker_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifact: Option<ArtifactReference>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RestartRecoveryProjection {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub main: Option<MainRecovery>,
    #[serde(default)]
    pub workers: Vec<WorkerRecovery>,
}

fn serialized_len(journal: &RestartJournal) -> usize {
    serde_json::to_vec(journal).map_or(usize::MAX, |bytes| bytes.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn worker(identity: &str, generation: u64, phase: WorkerPhase, at: u64) -> WorkerCheckpoint {
        WorkerCheckpoint {
            identity_hash: identity.to_string(),
            generation,
            owner: "subagent:test".to_string(),
            worker_id: Some(format!("worker-{generation}")),
            boot_id: "boot-a".to_string(),
            phase,
            artifact: None,
            updated_at_secs: at,
        }
    }

    #[test]
    fn restart_projects_resume_redispatch_and_fail_closed_rows() {
        let limits = RestartJournalLimits::default();
        let mut journal = RestartJournal::default();
        journal.record_main(
            MainAgentCheckpoint {
                harness: "codex".to_string(),
                owner: "supervisor:1".to_string(),
                resume_id: Some("thread-1".to_string()),
                boot_id: "boot-a".to_string(),
                phase: MainAgentPhase::Running,
                updated_at_secs: 1,
            },
            limits,
        );
        journal.record_worker(worker("dispatch", 1, WorkerPhase::Dispatching, 2), limits);
        journal.record_worker(worker("running", 1, WorkerPhase::Running, 3), limits);
        let mut with_artifact = worker("artifact", 1, WorkerPhase::Running, 4);
        with_artifact.artifact = Some(ArtifactReference {
            locator_hash: "path-hash".to_string(),
            git_commit: Some("abc123".to_string()),
        });
        journal.record_worker(with_artifact, limits);

        let projection = journal.reconcile("boot-b");
        assert_eq!(
            projection.main.as_ref().map(|main| main.action),
            Some(RecoveryAction::ResumeMain)
        );
        assert_eq!(
            projection.workers[0].action,
            RecoveryAction::IntegrateArtifacts
        );
        assert_eq!(projection.workers[1].action, RecoveryAction::Redispatch);
        assert_eq!(projection.workers[2].action, RecoveryAction::NeedsOperator);
    }

    #[test]
    fn metadata_refresh_does_not_duplicate_phase_transition() {
        let limits = RestartJournalLimits::default();
        let mut journal = RestartJournal::default();
        journal.record_worker(worker("same", 1, WorkerPhase::Running, 1), limits);
        let mut refreshed = worker("same", 1, WorkerPhase::Running, 2);
        refreshed.worker_id = Some("reattached-worker".to_string());
        journal.record_worker(refreshed, limits);
        assert_eq!(journal.transitions.len(), 1);
        assert_eq!(journal.workers[0].updated_at_secs, 2);
        assert_eq!(
            journal.workers[0].worker_id.as_deref(),
            Some("reattached-worker")
        );
    }

    #[test]
    fn compaction_keeps_recent_generations_and_gcs_old_terminal_workers() {
        let limits = RestartJournalLimits {
            recent_generations: 2,
            terminal_retention_secs: 10,
            max_transitions: 4,
            compact_every: 1,
            ..RestartJournalLimits::default()
        };
        let mut journal = RestartJournal::default();
        for generation in 1..=5 {
            journal.record_worker(
                worker("same", generation, WorkerPhase::Completed, generation),
                limits,
            );
        }
        journal.compact(limits, 20);
        assert!(journal.workers.is_empty(), "old terminal workers are GC'd");
        assert!(journal.transitions.len() <= 4);
        assert!(journal.compacted_through > 0);
    }

    #[test]
    fn document_cap_drops_history_before_terminal_snapshots_and_never_live_state() {
        let limits = RestartJournalLimits {
            document_byte_cap: 900,
            compact_every: usize::MAX,
            max_transitions: usize::MAX,
            ..RestartJournalLimits::default()
        };
        let mut journal = RestartJournal::default();
        for generation in 1..=20 {
            journal.record_worker(
                worker("same", generation, WorkerPhase::Completed, generation),
                limits,
            );
        }
        journal.record_worker(worker("live", 1, WorkerPhase::Running, 30), limits);
        assert!(journal.enforce_serialized_cap(limits));
        assert!(serialized_len(&journal) <= limits.document_byte_cap);
        assert!(
            journal
                .workers
                .iter()
                .any(|worker| worker.identity_hash == "live")
        );
    }

    #[test]
    fn live_snapshot_over_cap_fails_closed() {
        let limits = RestartJournalLimits {
            document_byte_cap: 64,
            ..RestartJournalLimits::default()
        };
        let mut journal = RestartJournal::default();
        journal.record_worker(worker("live", 1, WorkerPhase::Running, 1), limits);
        assert!(!journal.enforce_serialized_cap(limits));
        assert_eq!(journal.workers.len(), 1);
    }
}
