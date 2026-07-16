//! Machine-readable catalog for workflow invariants.
//!
//! The catalog is intentionally data-only. Doctor/autofix command surfaces can
//! evaluate the same ids, fact sources, predicates, disproof markers, and
//! remediation classes without scraping prose from specs or session responses.

use serde::{Deserialize, Serialize};
use std::fmt;

pub const WORKFLOW_INVARIANT_CATALOG_SCHEMA_VERSION: u8 = 1;
pub const WORKFLOW_INVARIANT_CATALOG_VERSION: &str = "workflow-invariant-catalog-v1";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowInvariantCatalog {
    pub schema_version: u8,
    pub contract_version: String,
    pub invariants: Vec<WorkflowInvariant>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorkflowInvariant {
    pub id: WorkflowInvariantId,
    pub title: String,
    pub severity: InvariantSeverity,
    pub fact_sources: Vec<InvariantFactSource>,
    pub ok_predicate: OkPredicate,
    pub disproof_markers: Vec<DisproofMarker>,
    pub safe_remediation: Vec<RemediationStep>,
    pub operator_gated_remediation: Vec<RemediationStep>,
    pub regression_coverage: Vec<RegressionCoverage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkflowInvariantId {
    QueueContinuation,
    StaleSupervisor,
    CloseoutCommit,
    EditorConvergence,
    GenerationRedirect,
    ParentGitlink,
}

impl WorkflowInvariantId {
    pub const REQUIRED_INITIAL: [Self; 6] = [
        Self::QueueContinuation,
        Self::StaleSupervisor,
        Self::CloseoutCommit,
        Self::EditorConvergence,
        Self::GenerationRedirect,
        Self::ParentGitlink,
    ];

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::QueueContinuation => "queue_continuation",
            Self::StaleSupervisor => "stale_supervisor",
            Self::CloseoutCommit => "closeout_commit",
            Self::EditorConvergence => "editor_convergence",
            Self::GenerationRedirect => "generation_redirect",
            Self::ParentGitlink => "parent_gitlink",
        }
    }
}

impl fmt::Display for WorkflowInvariantId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum InvariantSeverity {
    Critical,
    High,
    Medium,
    Low,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FactSourceKind {
    PreflightJson,
    SessionCheckJson,
    OpsLog,
    CycleState,
    ActorState,
    QueueState,
    EditorProofSidecar,
    GitState,
    ParentGitlink,
    WorkflowStateKernel,
    ProofLedger,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InvariantFactSource {
    pub source: FactSourceKind,
    pub field: String,
    pub required: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OkPredicate {
    pub all: Vec<PredicateClause>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PredicateClause {
    pub source: FactSourceKind,
    pub field: String,
    pub relation: PredicateRelation,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub expected: Option<String>,
    pub note: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PredicateRelation {
    Equals,
    NotEquals,
    GreaterThan,
    IsTrue,
    IsFalse,
    Present,
    Absent,
    Contains,
    Matches,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DisproofMarker {
    pub source: FactSourceKind,
    pub field: String,
    pub relation: PredicateRelation,
    pub marker: String,
    pub consequence: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemediationStep {
    pub action: RemediationAction,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub required_proof: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RemediationAction {
    ContinueQueueDrain,
    RestartSupervisorOnce,
    FinalizeOrWriteCommit,
    UseEditorIpcWriteback,
    RetryOnCurrentGeneration,
    CommitParentGitlink,
    AskOperatorLiveEditorProof,
    AskOperatorResolveGitState,
    AskOperatorResolveConflict,
}

impl RemediationAction {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ContinueQueueDrain => "continue_queue_drain",
            Self::RestartSupervisorOnce => "restart_supervisor_once",
            Self::FinalizeOrWriteCommit => "finalize_or_write_commit",
            Self::UseEditorIpcWriteback => "use_editor_ipc_writeback",
            Self::RetryOnCurrentGeneration => "retry_on_current_generation",
            Self::CommitParentGitlink => "commit_parent_gitlink",
            Self::AskOperatorLiveEditorProof => "ask_operator_live_editor_proof",
            Self::AskOperatorResolveGitState => "ask_operator_resolve_git_state",
            Self::AskOperatorResolveConflict => "ask_operator_resolve_conflict",
        }
    }

    pub fn accepts_autofix_command(self, command: &str) -> bool {
        match self {
            Self::RestartSupervisorOnce => {
                command == "agent-doc admin recycle --all-projects --json"
            }
            Self::FinalizeOrWriteCommit => {
                command.starts_with("agent-doc write --commit ")
                    && !command.contains("&&")
                    && !command.contains('<')
                    && !command.contains('>')
            }
            Self::UseEditorIpcWriteback => {
                command.starts_with("agent-doc session-check ")
                    && !command.contains("&&")
                    && !command.contains('<')
                    && !command.contains('>')
            }
            _ => false,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegressionCoverage {
    pub kind: RegressionCoverageKind,
    pub target: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub command: Option<String>,
    pub required: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegressionCoverageKind {
    UnitTest,
    IntegrationTest,
    SimWorld,
    EditorPluginTest,
}

pub fn workflow_invariant_catalog() -> WorkflowInvariantCatalog {
    WorkflowInvariantCatalog {
        schema_version: WORKFLOW_INVARIANT_CATALOG_SCHEMA_VERSION,
        contract_version: WORKFLOW_INVARIANT_CATALOG_VERSION.to_string(),
        invariants: vec![
            queue_continuation_invariant(),
            stale_supervisor_invariant(),
            closeout_commit_invariant(),
            editor_convergence_invariant(),
            generation_redirect_invariant(),
            parent_gitlink_invariant(),
        ],
    }
}

pub fn workflow_invariant_catalog_json() -> serde_json::Result<String> {
    serde_json::to_string_pretty(&workflow_invariant_catalog())
}

fn queue_continuation_invariant() -> WorkflowInvariant {
    WorkflowInvariant {
        id: WorkflowInvariantId::QueueContinuation,
        title: text("Drainable queue heads advance or receive durable handoff proof"),
        severity: InvariantSeverity::Critical,
        fact_sources: vec![
            fact(FactSourceKind::PreflightJson, "queue_continuation_required"),
            fact(FactSourceKind::PreflightJson, "queue_drainable_head_count"),
            fact(FactSourceKind::QueueState, "active_head.id"),
            fact(
                FactSourceKind::OpsLog,
                "idle_queue_watch_drain|drain_claim|go_queue_skip_undrainable",
            ),
            fact(FactSourceKind::ProofLedger, "queue_head terminal proof"),
        ],
        ok_predicate: OkPredicate {
            all: vec![
                clause(
                    FactSourceKind::PreflightJson,
                    "queue_continuation_required",
                    PredicateRelation::IsTrue,
                    None,
                    "The binary has classified the queue head as agent-drainable.",
                ),
                clause(
                    FactSourceKind::PreflightJson,
                    "queue_drainable_head_count",
                    PredicateRelation::GreaterThan,
                    Some("0"),
                    "At least one head is eligible for the in-session drain loop.",
                ),
                clause(
                    FactSourceKind::QueueState,
                    "active_head.id",
                    PredicateRelation::Present,
                    None,
                    "The continuation is tied to a stable queue head identity.",
                ),
            ],
        },
        disproof_markers: vec![
            disproof(
                FactSourceKind::OpsLog,
                "idle_queue_watch",
                PredicateRelation::Contains,
                "no_drainable_work",
                "Queue continuation was asserted even though the next head was not drainable.",
            ),
            disproof(
                FactSourceKind::ProofLedger,
                "queue_head terminal proof",
                PredicateRelation::Absent,
                "no_consumed_done_deferred_or_handoff_proof",
                "A queue head disappeared without a terminal proof row.",
            ),
        ],
        safe_remediation: vec![remediation(
            RemediationAction::ContinueQueueDrain,
            Some("agent-doc drain-claim <FILE> && agent-doc <FILE>"),
            &[
                "queue_continuation_required=true",
                "queue_drainable_head_count>0",
                "stable queue head id/hash",
            ],
        )],
        operator_gated_remediation: vec![remediation(
            RemediationAction::AskOperatorResolveConflict,
            None,
            &[
                "queue_continuation_required=false",
                "only operator-verify heads or unresolved prompt-bearing user edits remain",
            ],
        )],
        regression_coverage: vec![
            coverage(
                RegressionCoverageKind::SimWorld,
                "queue continuation remains eligible after file-IPC closeout fallback",
                Some("cargo test -p agent-doc-workflow"),
            ),
            coverage(
                RegressionCoverageKind::IntegrationTest,
                "agent-doc preflight/session-check queue continuation JSON contract",
                Some("cargo test -p agent-doc --test run_integration preflight_"),
            ),
        ],
    }
}

fn stale_supervisor_invariant() -> WorkflowInvariant {
    WorkflowInvariant {
        id: WorkflowInvariantId::StaleSupervisor,
        title: text("Stale route supervisors recover once before surfacing failure"),
        severity: InvariantSeverity::High,
        fact_sources: vec![
            fact(FactSourceKind::ActorState, "supervisor freshness"),
            fact(
                FactSourceKind::OpsLog,
                "supervisor_binary_stale_detected|supervisor_restart_redirect",
            ),
            fact(FactSourceKind::PreflightJson, "owned_pane_self_invocation"),
            fact(
                FactSourceKind::WorkflowStateKernel,
                "decide_stale_supervisor",
            ),
        ],
        ok_predicate: OkPredicate {
            all: vec![
                clause(
                    FactSourceKind::ActorState,
                    "supervisor freshness",
                    PredicateRelation::Equals,
                    Some("fresh"),
                    "The serving supervisor binary matches the installed agent-doc binary.",
                ),
                clause(
                    FactSourceKind::WorkflowStateKernel,
                    "decide_stale_supervisor",
                    PredicateRelation::NotEquals,
                    Some("surface_stale_without_restart"),
                    "A stale supervisor at a turn boundary is classified as recoverable when restart proof is available.",
                ),
            ],
        },
        disproof_markers: vec![
            disproof(
                FactSourceKind::OpsLog,
                "route dispatch",
                PredicateRelation::Contains,
                "failed_stage=queue_paused reason=#qchurn",
                "Legacy stale queue pause surfaced as a hard route/JB error.",
            ),
            disproof(
                FactSourceKind::OpsLog,
                "supervisor freshness",
                PredicateRelation::Contains,
                "stale host supervisor",
                "A stale supervisor still owned the route without a restart redirect marker.",
            ),
        ],
        safe_remediation: vec![remediation(
            RemediationAction::RestartSupervisorOnce,
            Some("agent-doc admin recycle --all-projects --json"),
            &[
                "installed binary identity differs from supervisor launch identity",
                "turn boundary or idle supervisor proof",
            ],
        )],
        operator_gated_remediation: vec![remediation(
            RemediationAction::AskOperatorResolveConflict,
            None,
            &[
                "restart failed",
                "fresh supervisor cannot prove prompt-ready pane",
                "live pane contains unsent operator draft",
            ],
        )],
        regression_coverage: vec![
            coverage(
                RegressionCoverageKind::UnitTest,
                "agent-doc-workflow stale supervisor transition rows",
                Some("cargo test -p agent-doc-workflow"),
            ),
            coverage(
                RegressionCoverageKind::IntegrationTest,
                "JB Run Agent Doc stale queue pause recovers through route retry",
                Some("cargo test -p agent-doc-orchestration stale_supervisor"),
            ),
        ],
    }
}

fn closeout_commit_invariant() -> WorkflowInvariant {
    WorkflowInvariant {
        id: WorkflowInvariantId::CloseoutCommit,
        title: text(
            "Closeout is complete only after document, snapshot, git, cycle state, and session-check agree",
        ),
        severity: InvariantSeverity::Critical,
        fact_sources: vec![
            fact(FactSourceKind::CycleState, "phase"),
            fact(FactSourceKind::GitState, "HEAD contains response commit"),
            fact(FactSourceKind::SessionCheckJson, "ok"),
            fact(FactSourceKind::OpsLog, "postcommit_worktree_check"),
            fact(FactSourceKind::ProofLedger, "terminal closeout proof"),
        ],
        ok_predicate: OkPredicate {
            all: vec![
                clause(
                    FactSourceKind::CycleState,
                    "phase",
                    PredicateRelation::Equals,
                    Some("committed"),
                    "The cycle state reached the committed boundary.",
                ),
                clause(
                    FactSourceKind::SessionCheckJson,
                    "ok",
                    PredicateRelation::IsTrue,
                    None,
                    "Post-closeout session-check reports no open response cycle.",
                ),
                clause(
                    FactSourceKind::OpsLog,
                    "postcommit_worktree_check.match",
                    PredicateRelation::IsTrue,
                    None,
                    "The committed blob and visible working tree agree after boundary cleanup.",
                ),
            ],
        },
        disproof_markers: vec![
            disproof(
                FactSourceKind::CycleState,
                "phase",
                PredicateRelation::Contains,
                "response_captured|write_applied|preflight_started",
                "The prior response was captured or written but not committed.",
            ),
            disproof(
                FactSourceKind::SessionCheckJson,
                "open_cycle",
                PredicateRelation::IsTrue,
                "open_cycle_after_finalize",
                "Finalize or write returned before the strict closeout boundary.",
            ),
        ],
        safe_remediation: vec![remediation(
            RemediationAction::FinalizeOrWriteCommit,
            Some("agent-doc finalize <FILE> --baseline-file <BASELINE> --stream --origin skill"),
            &[
                "captured response body exists",
                "baseline hash matches or replay classifier proves safe recovery",
                "session-check can run after commit",
            ],
        )],
        operator_gated_remediation: vec![remediation(
            RemediationAction::AskOperatorResolveConflict,
            None,
            &[
                "response body missing",
                "snapshot drift not covered by replay classifier",
                "real component conflict requires user choice",
            ],
        )],
        regression_coverage: vec![
            coverage(
                RegressionCoverageKind::IntegrationTest,
                "MCP finalize close-after-capture recovers on next preflight exactly once",
                Some(
                    "cargo test -p agent-doc --test test_cli mcp_finalize_close_after_capture_recovers_on_next_preflight_once",
                ),
            ),
            coverage(
                RegressionCoverageKind::UnitTest,
                "closeout recovery transition table",
                Some("cargo test -p agent-doc-orchestration closeout"),
            ),
        ],
    }
}

fn editor_convergence_invariant() -> WorkflowInvariant {
    WorkflowInvariant {
        id: WorkflowInvariantId::EditorConvergence,
        title: text(
            "Editor-visible writes rebase on Lazily/CPC authority and settle without operator save",
        ),
        severity: InvariantSeverity::High,
        fact_sources: vec![
            fact(
                FactSourceKind::WorkflowStateKernel,
                "Lazily/CPC authority hash and replica ACK frontier",
            ),
            fact(
                FactSourceKind::OpsLog,
                "transport=editor_ipc|disk_fallback|blocked|File Cache Conflict",
            ),
            fact(FactSourceKind::WorkflowStateKernel, "decide_live_buffer"),
            fact(FactSourceKind::GitState, "working tree hash"),
        ],
        ok_predicate: OkPredicate {
            all: vec![
                clause(
                    FactSourceKind::WorkflowStateKernel,
                    "canonical authority hash",
                    PredicateRelation::Present,
                    None,
                    "Lazily/CPC owns the live editor text used for convergence.",
                ),
                clause(
                    FactSourceKind::OpsLog,
                    "writeback transport",
                    PredicateRelation::Contains,
                    Some("transport=editor_ipc"),
                    "When a listener is active, document mutation is sent through editor IPC.",
                ),
                clause(
                    FactSourceKind::WorkflowStateKernel,
                    "decide_live_buffer",
                    PredicateRelation::NotEquals,
                    Some("block_unattributed_drift"),
                    "The write either applies cleanly or retains and rebases the same intent before requesting a native editor save.",
                ),
            ],
        },
        disproof_markers: vec![
            disproof(
                FactSourceKind::OpsLog,
                "writeback transport",
                PredicateRelation::Contains,
                "auto_recovery_disk_write_during_ipc_listener",
                "A direct disk write bypassed an active editor listener.",
            ),
            disproof(
                FactSourceKind::OpsLog,
                "editor conflict",
                PredicateRelation::Contains,
                "File Cache Conflict",
                "The editor rejected convergence and surfaced a user-visible cache conflict.",
            ),
        ],
        safe_remediation: vec![remediation(
            RemediationAction::UseEditorIpcWriteback,
            None,
            &[
                "live CPC replica registered",
                "retained intent has a content-bearing merge base",
                "rebased canonical target passes the shared structural validator",
            ],
        )],
        operator_gated_remediation: Vec::new(),
        regression_coverage: vec![
            coverage(
                RegressionCoverageKind::SimWorld,
                "multi-editor CRDT broadcast converges without file cache conflict",
                Some(
                    "cargo test -p agent-doc-orchestration multi_editor_crdt_broadcast_converges_without_file_cache_conflict",
                ),
            ),
            coverage(
                RegressionCoverageKind::EditorPluginTest,
                "JetBrains patch generation fence",
                Some(
                    "GRADLE_USER_HOME=/tmp/agent-doc-gradle ./gradlew test --tests '*PatchGenerationFenceTest'",
                ),
            ),
        ],
    }
}

fn generation_redirect_invariant() -> WorkflowInvariant {
    WorkflowInvariant {
        id: WorkflowInvariantId::GenerationRedirect,
        title: text(
            "Superseded actor generations redirect to the current generation or fail closed",
        ),
        severity: InvariantSeverity::High,
        fact_sources: vec![
            fact(FactSourceKind::ActorState, "generation"),
            fact(FactSourceKind::CycleState, "active cycle generation"),
            fact(
                FactSourceKind::OpsLog,
                "retry_on_current_generation|stale_generation",
            ),
            fact(
                FactSourceKind::WorkflowStateKernel,
                "RetryOnCurrentGeneration",
            ),
            fact(FactSourceKind::ProofLedger, "actor generation proof"),
        ],
        ok_predicate: OkPredicate {
            all: vec![
                clause(
                    FactSourceKind::ActorState,
                    "generation",
                    PredicateRelation::Equals,
                    Some("current"),
                    "Dispatch/write authority belongs to the current document actor generation.",
                ),
                clause(
                    FactSourceKind::ProofLedger,
                    "actor generation proof",
                    PredicateRelation::Present,
                    None,
                    "The accepted generation has a durable proof row.",
                ),
            ],
        },
        disproof_markers: vec![
            disproof(
                FactSourceKind::OpsLog,
                "dispatch",
                PredicateRelation::Contains,
                "stale_generation",
                "A superseded actor generation attempted to mutate or dispatch.",
            ),
            disproof(
                FactSourceKind::WorkflowStateKernel,
                "captured response transition",
                PredicateRelation::NotEquals,
                "retry_on_current_generation",
                "Captured response recovery did not redirect through the current generation.",
            ),
        ],
        safe_remediation: vec![remediation(
            RemediationAction::RetryOnCurrentGeneration,
            None,
            &[
                "current generation can be found",
                "superseded generation did not mutate document",
                "retry is bounded to one redirect",
            ],
        )],
        operator_gated_remediation: vec![remediation(
            RemediationAction::AskOperatorResolveConflict,
            None,
            &[
                "current generation missing",
                "two live generations both claim write authority",
                "retry would duplicate a response or queue consume",
            ],
        )],
        regression_coverage: vec![
            coverage(
                RegressionCoverageKind::UnitTest,
                "captured-response retry-on-current-generation transition",
                Some("cargo test -p agent-doc-workflow"),
            ),
            coverage(
                RegressionCoverageKind::IntegrationTest,
                "controller rejects stale generation and reports exact unblocker",
                Some("cargo test -p agent-doc --test run_integration codex_owned_pane_"),
            ),
        ],
    }
}

fn parent_gitlink_invariant() -> WorkflowInvariant {
    WorkflowInvariant {
        id: WorkflowInvariantId::ParentGitlink,
        title: text(
            "Submodule source commits are represented by matching parent gitlink commits before closeout",
        ),
        severity: InvariantSeverity::High,
        fact_sources: vec![
            fact(FactSourceKind::GitState, "submodule HEAD"),
            fact(FactSourceKind::GitState, "parent worktree status"),
            fact(FactSourceKind::ParentGitlink, "parent index/gitlink sha"),
            fact(FactSourceKind::SessionCheckJson, "changed_paths"),
            fact(FactSourceKind::ProofLedger, "terminal closeout proof"),
        ],
        ok_predicate: OkPredicate {
            all: vec![
                clause(
                    FactSourceKind::GitState,
                    "submodule HEAD",
                    PredicateRelation::Present,
                    None,
                    "Submodule implementation work has a committed source revision.",
                ),
                clause(
                    FactSourceKind::ParentGitlink,
                    "parent index/gitlink sha",
                    PredicateRelation::Equals,
                    Some("submodule HEAD"),
                    "The parent repository records the exact submodule commit used for closeout.",
                ),
                clause(
                    FactSourceKind::SessionCheckJson,
                    "changed_paths",
                    PredicateRelation::Contains,
                    Some("parent gitlink"),
                    "Closeout proof names the parent gitlink when submodule source changed.",
                ),
            ],
        },
        disproof_markers: vec![
            disproof(
                FactSourceKind::GitState,
                "parent worktree status",
                PredicateRelation::Contains,
                "modified: src/agent-doc",
                "The submodule commit exists but the parent gitlink was not committed.",
            ),
            disproof(
                FactSourceKind::ParentGitlink,
                "parent index/gitlink sha",
                PredicateRelation::NotEquals,
                "submodule HEAD",
                "Parent repository points at a different submodule revision than the verified source.",
            ),
        ],
        safe_remediation: vec![remediation(
            RemediationAction::CommitParentGitlink,
            Some("git add src/agent-doc && git commit -m '<message>'"),
            &[
                "submodule source commit exists",
                "parent gitlink diff contains only intended submodule path",
                "session document will close through agent-doc finalize",
            ],
        )],
        operator_gated_remediation: vec![remediation(
            RemediationAction::AskOperatorResolveGitState,
            None,
            &[
                "submodule has uncommitted source edits",
                "parent worktree includes unrelated dirty paths",
                "operator requested no git commit",
            ],
        )],
        regression_coverage: vec![
            coverage(
                RegressionCoverageKind::IntegrationTest,
                "agent-doc implementation closeout checks parent gitlink proof",
                Some(
                    "cargo test -p agent-doc-orchestration required_closeout_fails_when_parent_submodule_pointer_commit_fails",
                ),
            ),
            coverage(
                RegressionCoverageKind::UnitTest,
                "workflow invariant catalog parent_gitlink entry stays complete",
                Some("cargo test -p agent-doc-workflow workflow_invariant_catalog"),
            ),
        ],
    }
}

fn fact(source: FactSourceKind, field: &str) -> InvariantFactSource {
    InvariantFactSource {
        source,
        field: text(field),
        required: true,
    }
}

fn clause(
    source: FactSourceKind,
    field: &str,
    relation: PredicateRelation,
    expected: Option<&str>,
    note: &str,
) -> PredicateClause {
    PredicateClause {
        source,
        field: text(field),
        relation,
        expected: expected.map(text),
        note: text(note),
    }
}

fn disproof(
    source: FactSourceKind,
    field: &str,
    relation: PredicateRelation,
    marker: &str,
    consequence: &str,
) -> DisproofMarker {
    DisproofMarker {
        source,
        field: text(field),
        relation,
        marker: text(marker),
        consequence: text(consequence),
    }
}

fn remediation(
    action: RemediationAction,
    command: Option<&str>,
    required_proof: &[&str],
) -> RemediationStep {
    RemediationStep {
        action,
        command: command.map(text),
        required_proof: required_proof.iter().map(|proof| text(proof)).collect(),
    }
}

fn coverage(
    kind: RegressionCoverageKind,
    target: &str,
    command: Option<&str>,
) -> RegressionCoverage {
    RegressionCoverage {
        kind,
        target: text(target),
        command: command.map(text),
        required: true,
    }
}

fn text(value: &str) -> String {
    value.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;
    use std::collections::BTreeSet;

    #[test]
    fn workflow_invariant_catalog_covers_required_initial_ids() {
        let catalog = workflow_invariant_catalog();
        let ids = catalog
            .invariants
            .iter()
            .map(|invariant| invariant.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            ids.len(),
            catalog.invariants.len(),
            "duplicate invariant id"
        );
        for id in WorkflowInvariantId::REQUIRED_INITIAL {
            assert!(ids.contains(&id), "missing required invariant {id}");
        }
    }

    #[test]
    fn workflow_invariant_catalog_entries_are_complete() {
        let catalog = workflow_invariant_catalog();
        assert_eq!(
            catalog.schema_version,
            WORKFLOW_INVARIANT_CATALOG_SCHEMA_VERSION
        );
        assert_eq!(catalog.contract_version, WORKFLOW_INVARIANT_CATALOG_VERSION);

        for invariant in &catalog.invariants {
            assert!(!invariant.title.trim().is_empty(), "{:?}", invariant.id);
            assert!(!invariant.fact_sources.is_empty(), "{:?}", invariant.id);
            assert!(!invariant.ok_predicate.all.is_empty(), "{:?}", invariant.id);
            assert!(!invariant.disproof_markers.is_empty(), "{:?}", invariant.id);
            assert!(!invariant.safe_remediation.is_empty(), "{:?}", invariant.id);
            assert!(
                !invariant.regression_coverage.is_empty(),
                "{:?}",
                invariant.id
            );
            assert!(
                invariant
                    .regression_coverage
                    .iter()
                    .all(|coverage| !coverage.target.trim().is_empty()),
                "{:?}",
                invariant.id
            );
        }
        let editor = catalog
            .invariants
            .iter()
            .find(|invariant| invariant.id == WorkflowInvariantId::EditorConvergence)
            .expect("editor convergence invariant");
        assert!(editor.operator_gated_remediation.is_empty());
    }

    #[test]
    fn remediation_actions_render_stable_names_and_autofix_whitelist() {
        assert_eq!(
            RemediationAction::ContinueQueueDrain.as_str(),
            "continue_queue_drain"
        );
        assert_eq!(
            RemediationAction::RestartSupervisorOnce.as_str(),
            "restart_supervisor_once"
        );
        assert_eq!(
            RemediationAction::AskOperatorResolveConflict.as_str(),
            "ask_operator_resolve_conflict"
        );

        assert!(
            RemediationAction::RestartSupervisorOnce
                .accepts_autofix_command("agent-doc admin recycle --all-projects --json")
        );
        assert!(
            RemediationAction::FinalizeOrWriteCommit
                .accepts_autofix_command("agent-doc write --commit /tmp/session.md")
        );
        assert!(
            RemediationAction::UseEditorIpcWriteback
                .accepts_autofix_command("agent-doc session-check /tmp/session.md")
        );
        assert!(
            !RemediationAction::FinalizeOrWriteCommit
                .accepts_autofix_command("agent-doc write --commit /tmp/session.md && rm -rf /")
        );
        assert!(
            !RemediationAction::ContinueQueueDrain
                .accepts_autofix_command("agent-doc drain-claim /tmp/session.md")
        );
    }

    #[test]
    fn workflow_invariant_catalog_references_declared_fact_sources() {
        let catalog = workflow_invariant_catalog();
        for invariant in &catalog.invariants {
            let declared = invariant
                .fact_sources
                .iter()
                .map(|source| source.source)
                .collect::<BTreeSet<_>>();
            for clause in &invariant.ok_predicate.all {
                assert!(
                    declared.contains(&clause.source),
                    "{:?} predicate references undeclared source {:?}",
                    invariant.id,
                    clause.source
                );
            }
            for marker in &invariant.disproof_markers {
                assert!(
                    declared.contains(&marker.source),
                    "{:?} disproof references undeclared source {:?}",
                    invariant.id,
                    marker.source
                );
            }
        }
    }

    #[test]
    fn workflow_invariant_catalog_serializes_as_machine_readable_json() {
        let json = serde_json::to_value(workflow_invariant_catalog()).unwrap();
        assert_eq!(json["schema_version"], Value::from(1));
        assert_eq!(
            json["contract_version"],
            Value::from(WORKFLOW_INVARIANT_CATALOG_VERSION)
        );
        let first = &json["invariants"][0];
        for field in [
            "id",
            "fact_sources",
            "ok_predicate",
            "disproof_markers",
            "severity",
            "safe_remediation",
            "operator_gated_remediation",
            "regression_coverage",
        ] {
            assert!(first.get(field).is_some(), "missing JSON field {field}");
        }

        let pretty = workflow_invariant_catalog_json().unwrap();
        assert!(pretty.contains("\"queue_continuation\""));
        assert!(pretty.contains("\"parent_gitlink\""));
    }
}
