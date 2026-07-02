//! Route starting-actor timeout sidecar I/O.
//!
//! The controller dispatch crate owns the actor-ready facts and log-line
//! policy. This module owns the durable per-document timeout record used to
//! coalesce repeated route timeout logs.

use anyhow::Result;
use fs2::FileExt;
use serde::{Deserialize, Serialize};
use std::fs::OpenOptions;
use std::path::PathBuf;

use agent_doc_controller::dispatch::{ActorDispatchState, AuthoritativeActorReadyFacts};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StartingActorTimeoutRecord {
    pub pane_id: String,
    pub generation: u64,
    pub log_line: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartingActorTimeoutLogDecision {
    NewTimeout,
    DuplicateTimeout,
}

pub fn starting_actor_timeout_paths(file_path: &str) -> Option<(PathBuf, PathBuf)> {
    let requested = PathBuf::from(file_path);
    let root = agent_doc_fs::find_project_root(&requested)?;
    let hash = agent_doc_fs::document_state_hash_from_str(file_path);
    let state_dir = root.join(".agent-doc/state/route-starting-timeouts");
    let lock_dir = root.join(".agent-doc/locks");
    Some((
        state_dir.join(format!("{hash}.json")),
        lock_dir.join(format!("route-starting-timeout-{hash}.lock")),
    ))
}

pub fn record_starting_actor_timeout(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
    log_line: &str,
) -> Result<StartingActorTimeoutLogDecision> {
    let Some((state_path, lock_path)) = starting_actor_timeout_paths(file_path) else {
        return Ok(StartingActorTimeoutLogDecision::NewTimeout);
    };

    if let Some(parent) = state_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .read(true)
        .truncate(false)
        .write(true)
        .open(&lock_path)?;
    lock.lock_exclusive()?;

    let existing = std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|content| serde_json::from_str::<StartingActorTimeoutRecord>(&content).ok());
    if existing.as_ref().is_some_and(|record| {
        record.pane_id == facts.pane_id && record.generation == facts.generation
    }) {
        let _ = lock.unlock();
        return Ok(StartingActorTimeoutLogDecision::DuplicateTimeout);
    }

    let record = StartingActorTimeoutRecord {
        pane_id: facts.pane_id.clone(),
        generation: facts.generation,
        log_line: log_line.to_string(),
    };
    std::fs::write(&state_path, serde_json::to_string_pretty(&record)?)?;
    let _ = lock.unlock();
    Ok(StartingActorTimeoutLogDecision::NewTimeout)
}

pub fn load_starting_actor_timeout_record(file_path: &str) -> Option<StartingActorTimeoutRecord> {
    let (state_path, _) = starting_actor_timeout_paths(file_path)?;
    std::fs::read_to_string(&state_path)
        .ok()
        .and_then(|content| serde_json::from_str::<StartingActorTimeoutRecord>(&content).ok())
}

pub fn starting_actor_timeout_record_matches(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
) -> bool {
    if facts.actor_state != ActorDispatchState::Starting {
        return false;
    }
    starting_actor_timeout_record_identity_matches(file_path, facts)
}

pub fn starting_actor_timeout_record_identity_matches(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
) -> bool {
    load_starting_actor_timeout_record(file_path).is_some_and(|record| {
        record.pane_id == facts.pane_id && record.generation == facts.generation
    })
}

pub fn clear_starting_actor_timeout_record(file_path: &str) {
    let Some((state_path, _)) = starting_actor_timeout_paths(file_path) else {
        return;
    };
    let _ = std::fs::remove_file(state_path);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready_facts(
        pane_id: &str,
        generation: u64,
        actor_state: ActorDispatchState,
    ) -> AuthoritativeActorReadyFacts {
        AuthoritativeActorReadyFacts {
            pane_id: pane_id.to_string(),
            generation,
            actor_state,
            supervisor_health: "healthy".to_string(),
            runtime_state: "ready".to_string(),
            prompt_ready: false,
            last_transition_reason: "test".to_string(),
            last_transition_caller: "test".to_string(),
        }
    }

    #[test]
    fn record_starting_actor_timeout_coalesces_same_generation_and_pane() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.to_string_lossy().to_string();
        let facts = ready_facts("pane-a", 7, ActorDispatchState::Starting);

        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "repeat timeout").unwrap(),
            StartingActorTimeoutLogDecision::DuplicateTimeout
        );

        let next_generation = ready_facts("pane-a", 8, ActorDispatchState::Starting);
        assert_eq!(
            record_starting_actor_timeout(&file_path, &next_generation, "next timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
    }

    #[test]
    fn starting_actor_timeout_record_requires_starting_state_for_match() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.to_string_lossy().to_string();
        let starting = ready_facts("pane-a", 7, ActorDispatchState::Starting);
        record_starting_actor_timeout(&file_path, &starting, "timeout").unwrap();

        assert!(starting_actor_timeout_record_matches(&file_path, &starting));
        let busy = ready_facts("pane-a", 7, ActorDispatchState::Busy);
        assert!(starting_actor_timeout_record_identity_matches(
            &file_path, &busy
        ));
        assert!(!starting_actor_timeout_record_matches(&file_path, &busy));
    }

    #[test]
    fn clear_starting_actor_timeout_allows_new_record() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.to_string_lossy().to_string();
        let facts = ready_facts("pane-a", 7, ActorDispatchState::Starting);

        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
        clear_starting_actor_timeout_record(&file_path);
        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "after clear").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
    }
}
