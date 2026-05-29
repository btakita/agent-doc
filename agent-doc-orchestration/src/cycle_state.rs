//! # Module: cycle_state
//!
//! ## Spec
//! - Persists per-document cycle state under `.agent-doc/state/cycles/<doc-hash>.json`.
//! - Tracks the exact phase of the current or most recent response cycle:
//!   `preflight_started` → `response_captured` → `write_applied` → `committed`.
//!   A stale `preflight_started` cycle with no response artifact may become
//!   `abandoned` so a later preflight can start a fresh cycle for the same
//!   unresolved prompt.
//! - Stores cycle-scoped snapshot/file content hashes so callers can reason
//!   about exact cycle state instead of inferring from file-size drift or only
//!   the last `ops.log` line.
//! - `start_preflight()` opens a new cycle for a document and overwrites any
//!   prior committed state for that document.
//! - `mark_response_captured()` advances the open cycle to `response_captured`
//!   once the final parsed response has been durably stored.
//! - `mark_write_applied()` advances the open cycle to `write_applied` (or
//!   creates a synthetic cycle if a write lands without a prior preflight).
//! - `mark_committed()` advances the cycle to `committed` (or creates a
//!   synthetic committed cycle if commit happens without a prior state file).
//! - Lower-rank bookkeeping and duplicate terminal bookkeeping must never
//!   mutate an already-committed or abandoned cycle.
//! - `load()` returns the current persisted state when present.
//!
//! ## Agentic Contracts
//! - State is per-document, never global across the repo.
//! - Writes are deterministic JSON file replacements.
//! - Missing project root or state file returns `Ok(None)`.
//! - `is_open()` is true for any phase except `Committed`.
//!
//! ## Evals
//! - `start_preflight_persists_open_cycle`
//! - `mark_response_captured_sets_capture_metadata`
//! - `mark_write_applied_advances_existing_cycle`
//! - `mark_committed_closes_cycle`
//! - `mark_write_applied_creates_synthetic_cycle_when_missing`

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum CyclePhase {
    PreflightStarted,
    ResponseCaptured,
    WriteApplied,
    Committed,
    Abandoned,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BacklogTargetRequirement {
    pub path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub component: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub baseline_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub baseline_item_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CycleState {
    pub cycle_id: String,
    pub file: String,
    pub phase: CyclePhase,
    pub last_event: String,
    pub started_at: u64,
    pub updated_at: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub snapshot_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_snapshot_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normalized_file_hash: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub capture_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response_sha256: Option<String>,
    #[serde(default)]
    pub had_pending_mutations: bool,
    #[serde(default)]
    pub requires_backlog_capture: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_backlog_targets: Vec<BacklogTargetRequirement>,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub required_explicit_backlog_item_count: usize,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub required_plan_reference_count: usize,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_done_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub pending_kept_open_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reaped_pending_ids: Vec<String>,
    #[serde(default)]
    pub ipc_snapshot_adoption_blocked: bool,
}

impl CycleState {
    pub fn is_open(&self) -> bool {
        !matches!(self.phase, CyclePhase::Committed | CyclePhase::Abandoned)
    }
}

pub fn load(file: &Path) -> Result<Option<CycleState>> {
    let Some(path) = state_path(file)? else {
        return Ok(None);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(None);
    };
    let state: CycleState = serde_json::from_str(&content)?;
    Ok(Some(state))
}

pub fn start_preflight(
    file: &Path,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let now = now_secs();
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let state = CycleState {
        cycle_id: format!("cycle-{}", now_millis()),
        file: canonical.display().to_string(),
        phase: CyclePhase::PreflightStarted,
        last_event: "preflight_started".to_string(),
        started_at: now,
        updated_at: now,
        snapshot_hash: snapshot_content.map(crate::ops_log::content_hash),
        file_hash: file_content.map(crate::ops_log::content_hash),
        normalized_snapshot_hash: snapshot_content.map(normalized_content_hash),
        normalized_file_hash: file_content.map(normalized_content_hash),
        capture_id: None,
        response_sha256: None,
        had_pending_mutations: false,
        requires_backlog_capture: false,
        required_backlog_targets: Vec::new(),
        required_explicit_backlog_item_count: 0,
        required_plan_reference_count: 0,
        pending_done_ids: Vec::new(),
        pending_kept_open_ids: Vec::new(),
        reaped_pending_ids: Vec::new(),
        ipc_snapshot_adoption_blocked: false,
    };
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    Ok(state)
}

pub fn mark_write_applied(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state =
        load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::PreflightStarted));
    if cycle_phase_rank(CyclePhase::WriteApplied) < cycle_phase_rank(state.phase) {
        return Ok(state);
    }
    state.phase = CyclePhase::WriteApplied;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    state.snapshot_hash = snapshot_content.map(crate::ops_log::content_hash);
    state.file_hash = file_content.map(crate::ops_log::content_hash);
    state.normalized_snapshot_hash = snapshot_content.map(normalized_content_hash);
    state.normalized_file_hash = file_content.map(normalized_content_hash);
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    Ok(state)
}

pub fn mark_response_captured(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
    response_sha256: &str,
    cycle_id_hint: Option<&str>,
) -> Result<CycleState> {
    let mut state = load(file)?.unwrap_or_else(|| {
        synthetic_state_with_id(file, CyclePhase::PreflightStarted, cycle_id_hint)
    });
    if cycle_phase_rank(CyclePhase::ResponseCaptured) < cycle_phase_rank(state.phase) {
        return Ok(state);
    }
    state.phase = CyclePhase::ResponseCaptured;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    state.snapshot_hash = snapshot_content.map(crate::ops_log::content_hash);
    state.file_hash = file_content.map(crate::ops_log::content_hash);
    state.normalized_snapshot_hash = snapshot_content.map(normalized_content_hash);
    state.normalized_file_hash = file_content.map(normalized_content_hash);
    state.capture_id = Some(state.cycle_id.clone());
    state.response_sha256 = Some(response_sha256.to_string());
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    Ok(state)
}

pub fn mark_pending_mutations(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.had_pending_mutations {
        state.had_pending_mutations = true;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_pending_done_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .pending_done_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.pending_done_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_pending_kept_open_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .pending_kept_open_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.pending_kept_open_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_reaped_pending_ids(file: &Path, ids: &[String]) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };

    let mut changed = false;
    for id in ids
        .iter()
        .map(|id| normalize_pending_id(id))
        .filter(|id| !id.is_empty())
    {
        if !state
            .reaped_pending_ids
            .iter()
            .any(|existing| existing == &id)
        {
            state.reaped_pending_ids.push(id);
            changed = true;
        }
    }

    if changed {
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn resolved_pending_ids(file: &Path) -> Result<std::collections::HashSet<String>> {
    let Some(state) = load(file)? else {
        return Ok(std::collections::HashSet::new());
    };

    Ok(state
        .pending_done_ids
        .into_iter()
        .chain(state.reaped_pending_ids)
        .collect())
}

pub fn record_backlog_capture_requirement(
    file: &Path,
    required: bool,
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if state.requires_backlog_capture != required {
        state.requires_backlog_capture = required;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_backlog_target_requirements(
    file: &Path,
    requirements: &[BacklogTargetRequirement],
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if state.required_backlog_targets != requirements {
        state.required_backlog_targets = requirements.to_vec();
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_required_explicit_backlog_item_count(
    file: &Path,
    count: usize,
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if state.required_explicit_backlog_item_count != count {
        state.required_explicit_backlog_item_count = count;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn record_required_plan_reference_count(
    file: &Path,
    count: usize,
) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if state.required_plan_reference_count != count {
        state.required_plan_reference_count = count;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn mark_recoverable_preflight_timeout(file: &Path, event: &str) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.is_open() {
        return Ok(Some(state));
    }
    state.phase = CyclePhase::PreflightStarted;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    Ok(Some(state))
}

pub fn record_open_cycle_progress(file: &Path, event: &str) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.is_open() {
        return Ok(Some(state));
    }
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    Ok(Some(state))
}

pub fn record_ipc_snapshot_adoption_blocked(file: &Path) -> Result<Option<CycleState>> {
    let Some(mut state) = load(file)? else {
        return Ok(None);
    };
    if !state.ipc_snapshot_adoption_blocked {
        state.ipc_snapshot_adoption_blocked = true;
        state.updated_at = now_secs();
        save(file, &state)?;
    }
    Ok(Some(state))
}

pub fn mark_committed(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state = load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::WriteApplied));
    if matches!(state.phase, CyclePhase::Committed)
        && (state.last_event == event || is_stable_commit_event(&state.last_event))
    {
        return Ok(state);
    }
    state.phase = CyclePhase::Committed;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    if let Some(snapshot) = snapshot_content {
        state.snapshot_hash = Some(crate::ops_log::content_hash(snapshot));
        state.normalized_snapshot_hash = Some(normalized_content_hash(snapshot));
    }
    if let Some(content) = file_content {
        state.file_hash = Some(crate::ops_log::content_hash(content));
        state.normalized_file_hash = Some(normalized_content_hash(content));
    }
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    Ok(state)
}

pub fn mark_abandoned(
    file: &Path,
    event: &str,
    snapshot_content: Option<&str>,
    file_content: Option<&str>,
) -> Result<CycleState> {
    let mut state =
        load(file)?.unwrap_or_else(|| synthetic_state(file, CyclePhase::PreflightStarted));
    if !state.is_open() {
        return Ok(state);
    }
    state.phase = CyclePhase::Abandoned;
    state.last_event = event.to_string();
    state.updated_at = now_secs();
    if let Some(snapshot) = snapshot_content {
        state.snapshot_hash = Some(crate::ops_log::content_hash(snapshot));
        state.normalized_snapshot_hash = Some(normalized_content_hash(snapshot));
    }
    if let Some(content) = file_content {
        state.file_hash = Some(crate::ops_log::content_hash(content));
        state.normalized_file_hash = Some(normalized_content_hash(content));
    }
    save(file, &state)?;
    append_phase_event_to_session_log(file, &state);
    Ok(state)
}

fn append_phase_event_to_session_log(file: &Path, state: &CycleState) {
    let Ok(content) = std::fs::read_to_string(file) else {
        return;
    };
    let Ok((fm, _)) = crate::frontmatter::parse(&content) else {
        return;
    };
    let Some(session_id) = fm
        .session
        .as_deref()
        .map(str::trim)
        .filter(|id| !id.is_empty())
    else {
        return;
    };

    let mut event = format!(
        "document_cycle phase={} cycle={} event={}",
        cycle_phase_label(state.phase),
        state.cycle_id,
        state.last_event
    );
    if let Some(capture_id) = state.capture_id.as_deref() {
        event.push_str(&format!(" capture_id={capture_id}"));
    }
    let _ = crate::startup_miss::append_session_log_event(file, session_id, &event);
}

fn save(file: &Path, state: &CycleState) -> Result<()> {
    let Some(path) = state_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(state)?;
    std::fs::write(path, json)?;
    Ok(())
}

fn state_path(file: &Path) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(p) => p,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(&canonical)?;
    Ok(Some(
        root.join(".agent-doc/state/cycles")
            .join(format!("{hash}.json")),
    ))
}

fn synthetic_state(file: &Path, phase: CyclePhase) -> CycleState {
    synthetic_state_with_id(file, phase, None)
}

fn synthetic_state_with_id(
    file: &Path,
    phase: CyclePhase,
    cycle_id_hint: Option<&str>,
) -> CycleState {
    let now = now_secs();
    CycleState {
        cycle_id: cycle_id_hint
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("synthetic-{}", now_millis())),
        file: file.display().to_string(),
        phase,
        last_event: "synthetic_state".to_string(),
        started_at: now,
        updated_at: now,
        snapshot_hash: None,
        file_hash: None,
        normalized_snapshot_hash: None,
        normalized_file_hash: None,
        capture_id: None,
        response_sha256: None,
        had_pending_mutations: false,
        requires_backlog_capture: false,
        required_backlog_targets: Vec::new(),
        required_explicit_backlog_item_count: 0,
        required_plan_reference_count: 0,
        pending_done_ids: Vec::new(),
        pending_kept_open_ids: Vec::new(),
        reaped_pending_ids: Vec::new(),
        ipc_snapshot_adoption_blocked: false,
    }
}

fn cycle_phase_label(phase: CyclePhase) -> &'static str {
    match phase {
        CyclePhase::PreflightStarted => "preflight_started",
        CyclePhase::ResponseCaptured => "response_captured",
        CyclePhase::WriteApplied => "write_applied",
        CyclePhase::Committed => "committed",
        CyclePhase::Abandoned => "abandoned",
    }
}

fn cycle_phase_rank(phase: CyclePhase) -> u8 {
    match phase {
        CyclePhase::PreflightStarted => 0,
        CyclePhase::ResponseCaptured => 1,
        CyclePhase::WriteApplied => 2,
        CyclePhase::Committed => 3,
        CyclePhase::Abandoned => 4,
    }
}

fn is_stable_commit_event(event: &str) -> bool {
    matches!(
        event,
        "commit" | "commit_success" | "commit_already_current"
    )
}

fn is_zero(value: &usize) -> bool {
    *value == 0
}

fn normalized_content_hash(content: &str) -> String {
    // Neutralizes transient markers AND the independently-maintained agent:queue
    // component so response-replay / stale-lock recovery stays stable across
    // queue-maintenance churn (#adoc-queue-ipc-buffer-divergence #4). Must match
    // repair.rs's compare-side normalization exactly.
    crate::ops_log::content_hash(&crate::git::normalize_for_replay_hash(content))
}

fn normalize_pending_id(id: &str) -> String {
    id.trim().trim_start_matches('#').to_ascii_lowercase()
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn now_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn setup_project() -> tempfile::TempDir {
        let dir = tempfile::TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    #[test]
    fn start_preflight_persists_open_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::PreflightStarted);
        assert_eq!(
            load(&doc).unwrap().unwrap().phase,
            CyclePhase::PreflightStarted
        );
        assert!(load(&doc).unwrap().unwrap().is_open());
    }

    #[test]
    fn mark_write_applied_advances_existing_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_write_applied(&doc, "write_template", Some("new"), Some("new")).unwrap();
        assert_eq!(state.phase, CyclePhase::WriteApplied);
        assert_eq!(state.last_event, "write_template");
        assert!(state.snapshot_hash.is_some());
    }

    #[test]
    fn record_ipc_snapshot_adoption_blocked_sets_cycle_flag() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_ipc_snapshot_adoption_blocked(&doc)
            .unwrap()
            .expect("state should exist");

        assert!(state.ipc_snapshot_adoption_blocked);
        assert!(
            load(&doc)
                .unwrap()
                .expect("state should persist")
                .ipc_snapshot_adoption_blocked
        );
    }

    #[test]
    fn mark_response_captured_sets_capture_metadata() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "abc",
            None,
        )
        .unwrap();
        assert_eq!(state.phase, CyclePhase::ResponseCaptured);
        assert_eq!(state.capture_id.as_deref(), Some(state.cycle_id.as_str()));
        assert_eq!(state.response_sha256.as_deref(), Some("abc"));
    }

    #[test]
    fn mark_pending_mutations_sets_flag() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = mark_pending_mutations(&doc).unwrap().unwrap();
        assert!(state.had_pending_mutations);
        assert!(load(&doc).unwrap().unwrap().had_pending_mutations);
    }

    #[test]
    fn record_pending_done_ids_persists_normalized_ids() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_pending_done_ids(
            &doc,
            &["#AbC1".to_string(), "abc1".to_string(), "z9".to_string()],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            state.pending_done_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
        assert_eq!(
            load(&doc).unwrap().unwrap().pending_done_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
    }

    #[test]
    fn record_pending_kept_open_ids_persists_normalized_ids() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_pending_kept_open_ids(
            &doc,
            &["#AbC1".to_string(), "abc1".to_string(), "z9".to_string()],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            state.pending_kept_open_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
        assert_eq!(
            load(&doc).unwrap().unwrap().pending_kept_open_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
    }

    #[test]
    fn record_reaped_pending_ids_persists_normalized_ids() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_reaped_pending_ids(
            &doc,
            &["#AbC1".to_string(), "abc1".to_string(), "z9".to_string()],
        )
        .unwrap()
        .unwrap();

        assert_eq!(
            state.reaped_pending_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
        assert_eq!(
            load(&doc).unwrap().unwrap().reaped_pending_ids,
            vec!["abc1".to_string(), "z9".to_string()]
        );
    }

    #[test]
    fn record_backlog_capture_requirement_sets_flag() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_backlog_capture_requirement(&doc, true)
            .unwrap()
            .unwrap();
        assert!(state.requires_backlog_capture);
        assert!(load(&doc).unwrap().unwrap().requires_backlog_capture);
    }

    #[test]
    fn record_backlog_target_requirements_persists_targets() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let requirements = vec![BacklogTargetRequirement {
            path: dir.path().join("tasks/bugs.md").display().to_string(),
            component: Some("backlog".to_string()),
            baseline_hash: Some("abc".to_string()),
            baseline_item_ids: vec!["bug1".to_string()],
        }];

        let state = record_backlog_target_requirements(&doc, &requirements)
            .unwrap()
            .unwrap();
        assert_eq!(state.required_backlog_targets, requirements);
        assert_eq!(
            load(&doc).unwrap().unwrap().required_backlog_targets,
            requirements
        );
    }

    #[test]
    fn record_required_explicit_backlog_item_count_persists_count() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_required_explicit_backlog_item_count(&doc, 3)
            .unwrap()
            .unwrap();
        assert_eq!(state.required_explicit_backlog_item_count, 3);
        assert_eq!(
            load(&doc)
                .unwrap()
                .unwrap()
                .required_explicit_backlog_item_count,
            3
        );
    }

    #[test]
    fn record_required_plan_reference_count_persists_count() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state = record_required_plan_reference_count(&doc, 2)
            .unwrap()
            .unwrap();
        assert_eq!(state.required_plan_reference_count, 2);
        assert_eq!(
            load(&doc).unwrap().unwrap().required_plan_reference_count,
            2
        );
    }

    #[test]
    fn mark_committed_closes_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_write_applied(&doc, "write_template", Some("new"), Some("new")).unwrap();

        let state = mark_committed(&doc, "commit", Some("new"), Some("new")).unwrap();
        assert_eq!(state.phase, CyclePhase::Committed);
        assert!(!state.is_open());
    }

    #[test]
    fn mark_committed_is_idempotent_for_terminal_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();
        let replay = mark_committed(&doc, "repair_applied", Some("new"), Some("new")).unwrap();

        assert_eq!(replay, committed);
        assert_eq!(load(&doc).unwrap().unwrap(), committed);
    }

    #[test]
    fn abandoned_and_timeout_bookkeeping_do_not_reopen_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();
        let abandoned = mark_abandoned(&doc, "stale_empty", Some("new"), Some("new")).unwrap();
        let timeout = mark_recoverable_preflight_timeout(&doc, "recoverable_timeout")
            .unwrap()
            .unwrap();

        assert_eq!(abandoned, committed);
        assert_eq!(timeout, committed);
        assert_eq!(load(&doc).unwrap().unwrap(), committed);
    }

    #[test]
    fn mark_abandoned_closes_cycle_without_commit() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        let state =
            mark_abandoned(&doc, "abandon_empty_preflight", Some("snap"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::Abandoned);
        assert_eq!(state.last_event, "abandon_empty_preflight");
        assert!(!state.is_open());
    }

    #[test]
    fn mark_write_applied_creates_synthetic_cycle_when_missing() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();

        let state = mark_write_applied(&doc, "recover_apply", Some("body"), Some("body")).unwrap();
        assert_eq!(state.phase, CyclePhase::WriteApplied);
        assert!(state.cycle_id.starts_with("synthetic-"));
    }

    #[test]
    fn mark_write_applied_does_not_regress_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

        let state = mark_write_applied(&doc, "repair_applied", Some("new"), Some("new")).unwrap();
        let body_hash = crate::ops_log::content_hash("body");
        assert_eq!(state.phase, CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_success");
        assert_eq!(state.snapshot_hash.as_deref(), Some(body_hash.as_str()));
        assert_eq!(state.file_hash.as_deref(), Some(body_hash.as_str()));
    }

    #[test]
    fn mark_response_captured_does_not_regress_committed_cycle() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body").unwrap();
        start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

        let state = mark_response_captured(
            &doc,
            "response_captured",
            Some("new"),
            Some("new"),
            "abc",
            Some(&committed.cycle_id),
        )
        .unwrap();
        assert_eq!(state.phase, CyclePhase::Committed);
        assert_eq!(state.last_event, "commit_success");
        assert_eq!(state.capture_id, committed.capture_id);
        assert_eq!(state.response_sha256, committed.response_sha256);
    }

    #[test]
    fn cycle_phase_transitions_append_to_session_log() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "---\nagent_doc_session: sess-123\n---\n\nbody\n").unwrap();

        let started = start_preflight(&doc, Some("snap"), Some("body")).unwrap();
        let captured = mark_response_captured(
            &doc,
            "response_captured",
            Some("snap"),
            Some("body"),
            "abc123",
            Some(&started.cycle_id),
        )
        .unwrap();
        let written =
            mark_write_applied(&doc, "write_template", Some("body"), Some("body")).unwrap();
        let committed = mark_committed(&doc, "commit_success", Some("body"), Some("body")).unwrap();

        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/sess-123.log")).unwrap();
        assert!(log.contains(&format!(
            "document_cycle phase=preflight_started cycle={} event=preflight_started",
            started.cycle_id
        )));
        assert!(log.contains(&format!(
            "document_cycle phase=response_captured cycle={} event=response_captured capture_id={}",
            captured.cycle_id, captured.cycle_id
        )));
        assert!(log.contains(&format!(
            "document_cycle phase=write_applied cycle={} event=write_template",
            written.cycle_id
        )));
        assert!(log.contains(&format!(
            "document_cycle phase=committed cycle={} event=commit_success",
            committed.cycle_id
        )));
    }

    #[test]
    fn cycle_phase_transitions_skip_session_log_without_session_id() {
        let dir = setup_project();
        let doc = dir.path().join("doc.md");
        fs::write(&doc, "body\n").unwrap();

        start_preflight(&doc, Some("snap"), Some("body")).unwrap();

        assert!(
            !dir.path().join(".agent-doc/logs").exists(),
            "plain documents without a session id should not create session logs"
        );
    }
}
