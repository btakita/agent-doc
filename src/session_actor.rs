//! # Module: session_actor
//!
//! ## Spec
//! - Freezes the phase-1 single-owner session-actor contract while introducing
//!   the phase-2 durable actor store.
//! - Ownership generations are monotonic per document session and start at `1`.
//! - A new authoritative generation is recorded whenever a new `agent-doc start`
//!   session begins or an existing document session is rebound to a different pane.
//! - Generation inference remains backward-compatible with legacy logs that only
//!   have `session_start` lines and no explicit generation markers.
//! - Ownership-transition events render stable machine-readable fields:
//!   `caller`, `reason`, `prior_generation`, `new_generation`, `old_pane`,
//!   `new_pane`, `old_window`, `new_window`.
//! - The durable actor store lives behind the Project Controller's SQLite state,
//!   keyed by canonical document path, while `session-actors.json` and
//!   `sessions.json` remain compatibility projections during migration.
//!
//! ## Agentic Contracts
//! - The actor store is authoritative for the latest generation once present.
//! - Store updates must be monotonic and fail closed on generation regressions.
//! - Log parsing must tolerate unrelated session-log events and malformed lines.
//! - Legacy logs with at least one `session_start` but no explicit generation
//!   markers infer the current generation from the number of starts.
//!
//! ## Evals
//! - `infer_latest_generation_counts_legacy_session_starts`
//! - `infer_latest_generation_prefers_explicit_generation_markers`
//! - `format_transition_event_uses_stable_placeholders`
//! - `project_binding_stores_initial_generation`
//! - `project_binding_advances_generation_on_new_pane`
//! - `record_session_start_persists_authoritative_record`

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipGeneration {
    pub prior_generation: u64,
    pub new_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipTransitionEvent<'a> {
    pub caller: &'a str,
    pub reason: &'a str,
    pub prior_generation: u64,
    pub new_generation: u64,
    pub old_pane: Option<&'a str>,
    pub new_pane: &'a str,
    pub old_window: Option<&'a str>,
    pub new_window: Option<&'a str>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActorState {
    Starting,
    Ready,
    Busy,
    WaitingInput,
    Closed,
    Blocked,
}

impl ActorState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Starting => "starting",
            Self::Ready => "ready",
            Self::Busy => "busy",
            Self::WaitingInput => "waiting_input",
            Self::Closed => "closed",
            Self::Blocked => "blocked",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "starting" => Some(Self::Starting),
            "ready" => Some(Self::Ready),
            "busy" => Some(Self::Busy),
            "waiting_input" => Some(Self::WaitingInput),
            "closed" => Some(Self::Closed),
            "blocked" => Some(Self::Blocked),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorLastTransition {
    pub caller: String,
    pub reason: String,
    pub timestamp: u64,
    pub prior_generation: u64,
    pub new_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorRecord {
    pub document_id: String,
    pub session_id: String,
    pub generation: u64,
    pub pane_id: String,
    pub window_id: String,
    pub harness: String,
    pub state: ActorState,
    pub last_transition: ActorLastTransition,
}

type ActorStore = std::collections::BTreeMap<String, ActorRecord>;

#[derive(Debug, Clone)]
struct ActorRecordUpdate<'a> {
    session_id: &'a str,
    pane_id: &'a str,
    window_id: &'a str,
    harness: &'a str,
    state: ActorState,
    caller: &'a str,
    reason: &'a str,
    generation: u64,
    expected_prior_generation: Option<u64>,
}

fn log_path(file: &Path, session_id: &str) -> Result<Option<PathBuf>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    let Some(root) = crate::snapshot::find_project_root(&canonical) else {
        return Ok(None);
    };
    Ok(Some(
        root.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
    ))
}

fn timestamp_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn extract_event(raw_line: &str) -> &str {
    raw_line
        .split_once("] ")
        .map(|(_, event)| event)
        .unwrap_or(raw_line)
        .trim()
}

fn parse_u64_field(event: &str, name: &str) -> Option<u64> {
    event.split_whitespace().find_map(|part| {
        part.strip_prefix(name)
            .and_then(|value| value.parse::<u64>().ok())
    })
}

fn render_field<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    value.filter(|value| !value.is_empty()).unwrap_or(fallback)
}

pub(crate) fn normalize_harness_name(raw: &str) -> String {
    match raw.trim() {
        "" => "default".to_string(),
        "claude" => "claude-code".to_string(),
        other => other.to_string(),
    }
}

fn document_harness_from_content(content: &str) -> Option<String> {
    crate::frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.agent)
        .map(|value| normalize_harness_name(&value))
}

fn document_path_from_base_dir(base_dir: &Path, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn canonical_document_id(file: &Path) -> Result<Option<String>> {
    let canonical = match file.canonicalize() {
        Ok(path) => path,
        Err(_) => return Ok(None),
    };
    Ok(Some(canonical.to_string_lossy().to_string()))
}

pub fn canonical_document_id_in(base_dir: &Path, file: &str) -> String {
    canonical_document_id(&document_path_from_base_dir(base_dir, file))
        .ok()
        .flatten()
        .unwrap_or_else(|| crate::sessions::canonical_registry_key_in(base_dir, file))
}

fn load_store_in(base_dir: &Path) -> Result<ActorStore> {
    crate::project_controller::load_actor_store(base_dir)
}

fn legacy_generation_for_document(file: &Path, session_id_hint: Option<&str>) -> Result<u64> {
    let Some(canonical) = file.canonicalize().ok() else {
        return Ok(0);
    };
    let Some(session_id) = session_id_hint
        .map(ToOwned::to_owned)
        .or_else(|| crate::frontmatter::read_session_id(&canonical))
    else {
        return Ok(0);
    };
    let Some(path) = log_path(&canonical, &session_id)? else {
        return Ok(0);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(0);
    };
    Ok(infer_latest_generation_from_content(&content))
}

fn current_generation_in(
    base_dir: &Path,
    file: &str,
    session_id_hint: Option<&str>,
) -> Result<u64> {
    let document_id = canonical_document_id_in(base_dir, file);
    let store = load_store_in(base_dir)?;
    if let Some(record) = store.get(&document_id) {
        return Ok(record.generation);
    }
    legacy_generation_for_document(
        &document_path_from_base_dir(base_dir, file),
        session_id_hint,
    )
}

fn store_record_in(
    base_dir: &Path,
    file: &str,
    update: ActorRecordUpdate<'_>,
) -> Result<ActorRecord> {
    let document_id = canonical_document_id_in(base_dir, file);
    let current_record = crate::project_controller::load_actor_record(base_dir, &document_id)?;
    let prior_generation = current_record
        .as_ref()
        .map(|record| record.generation)
        .unwrap_or_else(|| {
            current_generation_in(base_dir, file, Some(update.session_id)).unwrap_or(0)
        });
    if let Some(expected) = update.expected_prior_generation
        && prior_generation != expected
    {
        anyhow::bail!(
            "actor generation compare-and-swap failed for {}: expected {}, found {}",
            document_id,
            expected,
            prior_generation
        );
    }
    if update.generation < prior_generation {
        anyhow::bail!(
            "actor generation regression for {}: attempted {}, current {}",
            document_id,
            update.generation,
            prior_generation
        );
    }

    let record = ActorRecord {
        document_id: document_id.clone(),
        session_id: update.session_id.to_string(),
        generation: update.generation,
        pane_id: update.pane_id.to_string(),
        window_id: update.window_id.to_string(),
        harness: normalize_harness_name(update.harness),
        state: update.state,
        last_transition: ActorLastTransition {
            caller: update.caller.to_string(),
            reason: update.reason.to_string(),
            timestamp: timestamp_secs(),
            prior_generation,
            new_generation: update.generation,
        },
    };
    crate::project_controller::store_actor_record(
        base_dir,
        update.expected_prior_generation,
        &record,
    )
}

pub fn detect_document_harness_in(base_dir: &Path, file: &str) -> String {
    let document_path = document_path_from_base_dir(base_dir, file);
    if let Ok(content) = std::fs::read_to_string(&document_path)
        && let Some(harness) = document_harness_from_content(&content)
    {
        return harness;
    }
    normalize_harness_name(&agent_doc::model_tier::detect_harness())
}

pub fn load_record_in(base_dir: &Path, file: &str) -> Result<Option<ActorRecord>> {
    let document_id = canonical_document_id_in(base_dir, file);
    Ok(load_store_in(base_dir)?.get(&document_id).cloned())
}

pub fn project_binding_in(
    base_dir: &Path,
    file: &str,
    session_id: &str,
    pane_id: &str,
    window_id: &str,
    caller: &str,
    reason: &str,
) -> Result<OwnershipGeneration> {
    let prior_generation = current_generation_in(base_dir, file, Some(session_id))?;
    let generation = load_record_in(base_dir, file)?
        .filter(|record| record.pane_id == pane_id)
        .map(|record| record.generation)
        .unwrap_or_else(|| prior_generation.saturating_add(1).max(1));
    let harness = detect_document_harness_in(base_dir, file);
    let _ = store_record_in(
        base_dir,
        file,
        ActorRecordUpdate {
            session_id,
            pane_id,
            window_id,
            harness: &harness,
            state: ActorState::Ready,
            caller,
            reason,
            generation,
            expected_prior_generation: Some(prior_generation),
        },
    )?;
    Ok(OwnershipGeneration {
        prior_generation,
        new_generation: generation,
    })
}

pub(crate) fn record_session_start_direct(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    window_id: &str,
    generation: u64,
) -> Result<ActorRecord> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(base_dir) = crate::snapshot::find_project_root(&canonical) else {
        anyhow::bail!(
            "failed to locate project root for actor record {}",
            canonical.display()
        );
    };
    let harness = detect_document_harness_in(&base_dir, &canonical.to_string_lossy());
    store_record_in(
        &base_dir,
        &canonical.to_string_lossy(),
        ActorRecordUpdate {
            session_id,
            pane_id,
            window_id,
            harness: &harness,
            state: ActorState::Starting,
            caller: "start",
            reason: "session_start",
            generation,
            expected_prior_generation: Some(generation.saturating_sub(1)),
        },
    )
}

#[allow(dead_code)] // Compatibility API for direct callers; supervisor path uses controller IPC.
pub fn record_session_start(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    window_id: &str,
    generation: u64,
) -> Result<ActorRecord> {
    record_session_start_direct(file, session_id, pane_id, window_id, generation)
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn transition_state_in(
    base_dir: &Path,
    document_id: &str,
    session_id: &str,
    pane_id: &str,
    expected_generation: Option<u64>,
    state: ActorState,
    caller: &str,
    reason: &str,
) -> Result<ActorRecord> {
    let Some(current) = load_record_in(base_dir, document_id)? else {
        anyhow::bail!(
            "missing authoritative actor record for {}",
            document_id
        );
    };
    if current.session_id != session_id {
        anyhow::bail!(
            "stale actor transition for {}: session {} no longer owns generation {} (current session {})",
            document_id,
            session_id,
            current.generation,
            current.session_id
        );
    }
    if let Some(expected) = expected_generation
        && current.generation != expected
    {
        if current.pane_id != pane_id {
            anyhow::bail!(
                "stale actor transition for {}: generation {} pane {} no longer current (current generation {} pane {})",
                document_id,
                expected,
                pane_id,
                current.generation,
                current.pane_id
            );
        }
    }
    if current.pane_id != pane_id {
        anyhow::bail!(
            "stale actor transition for {}: pane {} no longer owns generation {} (current pane {})",
            document_id,
            pane_id,
            current.generation,
            current.pane_id
        );
    }
    let harness = if current.harness.trim().is_empty() {
        detect_document_harness_in(base_dir, document_id)
    } else {
        current.harness.clone()
    };
    store_record_in(
        base_dir,
        document_id,
        ActorRecordUpdate {
            session_id,
            pane_id,
            window_id: &current.window_id,
            harness: &harness,
            state,
            caller,
            reason,
            generation: current.generation,
            expected_prior_generation: Some(current.generation),
        },
    )
}

pub(crate) fn transition_state_direct(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    expected_generation: Option<u64>,
    state: ActorState,
    caller: &str,
    reason: &str,
) -> Result<ActorRecord> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let Some(base_dir) = crate::snapshot::find_project_root(&canonical) else {
        anyhow::bail!(
            "failed to locate project root for actor record {}",
            canonical.display()
        );
    };
    let file_key = canonical.to_string_lossy().to_string();
    transition_state_in(
        &base_dir,
        &file_key,
        session_id,
        pane_id,
        expected_generation,
        state,
        caller,
        reason,
    )
}

#[allow(dead_code)] // Compatibility API for direct callers; supervisor path uses controller IPC.
pub fn transition_state(
    file: &Path,
    session_id: &str,
    pane_id: &str,
    state: ActorState,
    caller: &str,
    reason: &str,
) -> Result<ActorRecord> {
    transition_state_direct(file, session_id, pane_id, None, state, caller, reason)
}

pub fn infer_latest_generation_from_content(content: &str) -> u64 {
    let mut latest_explicit = 0u64;
    let mut legacy_session_starts = 0u64;

    for raw_line in content.lines() {
        let event = extract_event(raw_line);
        if event.is_empty() {
            continue;
        }
        if event.starts_with("session_start ") {
            legacy_session_starts += 1;
            if let Some(generation) = parse_u64_field(event, "generation=") {
                latest_explicit = latest_explicit.max(generation);
            }
            continue;
        }
        if event.starts_with("ownership_transition ")
            && let Some(generation) = parse_u64_field(event, "new_generation=")
        {
            latest_explicit = latest_explicit.max(generation);
        }
    }

    if latest_explicit > 0 {
        latest_explicit
    } else {
        legacy_session_starts
    }
}

pub fn infer_latest_generation(file: &Path, session_id: &str) -> Result<u64> {
    let actor_generation = file
        .canonicalize()
        .ok()
        .and_then(|canonical| {
            crate::snapshot::find_project_root(&canonical).and_then(|root| {
                load_record_in(&root, &canonical.to_string_lossy())
                    .ok()
                    .flatten()
                    .map(|record| record.generation)
            })
        })
        .unwrap_or(0);
    let Some(path) = log_path(file, session_id)? else {
        return Ok(actor_generation);
    };
    let Some(content) = crate::fs_util::read_optional_text(&path)? else {
        return Ok(actor_generation);
    };
    Ok(actor_generation.max(infer_latest_generation_from_content(&content)))
}

pub fn next_generation(file: &Path, session_id: &str) -> Result<OwnershipGeneration> {
    let prior_generation = infer_latest_generation(file, session_id)?;
    Ok(OwnershipGeneration {
        prior_generation,
        new_generation: prior_generation.saturating_add(1).max(1),
    })
}

pub fn format_transition_event(event: OwnershipTransitionEvent<'_>) -> String {
    format!(
        "ownership_transition caller={} reason={} prior_generation={} new_generation={} old_pane={} new_pane={} old_window={} new_window={}",
        event.caller,
        event.reason,
        event.prior_generation,
        event.new_generation,
        render_field(event.old_pane, "none"),
        render_field(Some(event.new_pane), "none"),
        render_field(event.old_window, "none"),
        render_field(event.new_window, "unknown"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn seed_project_file(dir: &tempfile::TempDir, rel: &str, content: &str) -> PathBuf {
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join(rel);
        if let Some(parent) = file.parent() {
            fs::create_dir_all(parent).unwrap();
        }
        fs::write(&file, content).unwrap();
        file
    }

    #[test]
    fn infer_latest_generation_counts_legacy_session_starts() {
        let content = concat!(
            "[1] session_start file=doc.md pane=%41 session=session-1\n",
            "[2] codex_start mode=fresh restart_count=0\n",
            "[10] session_start file=doc.md pane=%52 session=session-1\n",
        );

        assert_eq!(infer_latest_generation_from_content(content), 2);
    }

    #[test]
    fn infer_latest_generation_prefers_explicit_generation_markers() {
        let content = concat!(
            "[1] ownership_transition caller=start reason=session_start prior_generation=0 new_generation=1 old_pane=none new_pane=%41 old_window=none new_window=@1\n",
            "[2] session_start file=doc.md pane=%41 session=session-1 generation=1\n",
            "[10] ownership_transition caller=route reason=registry_rebind prior_generation=1 new_generation=2 old_pane=%41 new_pane=%52 old_window=@1 new_window=@2\n",
        );

        assert_eq!(infer_latest_generation_from_content(content), 2);
    }

    #[test]
    fn format_transition_event_uses_stable_placeholders() {
        let rendered = format_transition_event(OwnershipTransitionEvent {
            caller: "start",
            reason: "session_start",
            prior_generation: 0,
            new_generation: 1,
            old_pane: None,
            new_pane: "%41",
            old_window: None,
            new_window: Some("@1"),
        });

        assert_eq!(
            rendered,
            "ownership_transition caller=start reason=session_start prior_generation=0 new_generation=1 old_pane=none new_pane=%41 old_window=none new_window=@1"
        );
    }

    #[test]
    fn project_binding_stores_initial_generation() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-1\nagent: codex\n---\nBody\n",
        );

        let generations = project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "session-1",
            "%41",
            "@1",
            "route",
            "dispatch_bind",
        )
        .unwrap();

        assert_eq!(
            generations,
            OwnershipGeneration {
                prior_generation: 0,
                new_generation: 1
            }
        );
        let record = load_record_in(tmp.path(), &file.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(record.generation, 1);
        assert_eq!(record.pane_id, "%41");
        assert_eq!(record.state, ActorState::Ready);
        assert_eq!(record.harness, "codex");
    }

    #[test]
    fn project_binding_advances_generation_on_new_pane() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-2\nagent: codex\n---\nBody\n",
        );

        project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "session-2",
            "%41",
            "@1",
            "route",
            "dispatch_bind",
        )
        .unwrap();
        let generations = project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "session-2",
            "%52",
            "@2",
            "sync",
            "recover_owner",
        )
        .unwrap();

        assert_eq!(
            generations,
            OwnershipGeneration {
                prior_generation: 1,
                new_generation: 2
            }
        );
        let record = load_record_in(tmp.path(), &file.to_string_lossy())
            .unwrap()
            .unwrap();
        assert_eq!(record.generation, 2);
        assert_eq!(record.pane_id, "%52");
        assert_eq!(record.window_id, "@2");
    }

    #[test]
    fn record_session_start_persists_authoritative_record() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-3\nagent: codex\n---\nBody\n",
        );

        let record = record_session_start(&file, "session-3", "%61", "@7", 1).unwrap();

        assert_eq!(record.generation, 1);
        assert_eq!(record.state, ActorState::Starting);
        assert_eq!(record.last_transition.prior_generation, 0);
        assert_eq!(record.last_transition.new_generation, 1);
        assert_eq!(record.harness, "codex");
    }

    #[test]
    fn transition_state_updates_same_generation_without_rebinding() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-4\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-4", "%71", "@8", 1).unwrap();
        let record = transition_state(
            &file,
            "session-4",
            "%71",
            ActorState::Ready,
            "supervisor",
            "prompt_ready",
        )
        .unwrap();

        assert_eq!(record.generation, 1);
        assert_eq!(record.state, ActorState::Ready);
        assert_eq!(record.last_transition.prior_generation, 1);
        assert_eq!(record.last_transition.new_generation, 1);
        assert_eq!(record.last_transition.caller, "supervisor");
        assert_eq!(record.last_transition.reason, "prompt_ready");
    }

    #[test]
    fn transition_state_preserves_generation_for_supervisor_lifecycle_updates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-4b\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-4b", "%72", "@8", 1).unwrap();

        let waiting = transition_state(
            &file,
            "session-4b",
            "%72",
            ActorState::WaitingInput,
            "supervisor",
            "clean_exit_prompt",
        )
        .unwrap();
        assert_eq!(waiting.generation, 1);
        assert_eq!(waiting.state, ActorState::WaitingInput);
        assert_eq!(waiting.last_transition.prior_generation, 1);
        assert_eq!(waiting.last_transition.new_generation, 1);
        assert_eq!(waiting.last_transition.reason, "clean_exit_prompt");

        let blocked = transition_state(
            &file,
            "session-4b",
            "%72",
            ActorState::Blocked,
            "supervisor",
            "supervisor_halted",
        )
        .unwrap();
        assert_eq!(blocked.generation, 1);
        assert_eq!(blocked.state, ActorState::Blocked);
        assert_eq!(blocked.last_transition.prior_generation, 1);
        assert_eq!(blocked.last_transition.new_generation, 1);
        assert_eq!(blocked.last_transition.reason, "supervisor_halted");

        let closed = transition_state(
            &file,
            "session-4b",
            "%72",
            ActorState::Closed,
            "supervisor",
            "user_quit_clean_exit",
        )
        .unwrap();
        assert_eq!(closed.generation, 1);
        assert_eq!(closed.state, ActorState::Closed);
        assert_eq!(closed.last_transition.prior_generation, 1);
        assert_eq!(closed.last_transition.new_generation, 1);
        assert_eq!(closed.last_transition.reason, "user_quit_clean_exit");
    }

    #[test]
    fn transition_state_rejects_stale_pane_updates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-5\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-5", "%81", "@9", 1).unwrap();
        let err = transition_state(
            &file,
            "session-5",
            "%82",
            ActorState::Closed,
            "supervisor",
            "session_end",
        )
        .expect_err("stale pane must not update authoritative actor state");

        assert!(
            format!("{err}").contains("no longer owns generation"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn record_session_start_rejects_stale_generation_after_new_owner_rebind() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-6\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-6", "%91", "@10", 1).unwrap();
        project_binding_in(
            tmp.path(),
            &file.to_string_lossy(),
            "session-6",
            "%92",
            "@11",
            "sync",
            "recover_owner",
        )
        .unwrap();

        let err = record_session_start(&file, "session-6", "%91", "@10", 1)
            .expect_err("stale generation must not overwrite the newer authoritative owner");
        assert!(
            format!("{err}").contains("compare-and-swap failed"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn transition_state_rejects_stale_session_updates() {
        let tmp = tempfile::TempDir::new().unwrap();
        let file = seed_project_file(
            &tmp,
            "tasks/test.md",
            "---\nagent_doc_session: session-7\nagent: codex\n---\nBody\n",
        );

        record_session_start(&file, "session-7", "%101", "@12", 1).unwrap();
        let err = transition_state(
            &file,
            "session-7-stale",
            "%101",
            ActorState::Closed,
            "supervisor",
            "session_end",
        )
        .expect_err("stale session updates must not mutate the authoritative record");

        assert!(
            format!("{err}").contains("current session"),
            "unexpected error: {err}"
        );
    }
}
