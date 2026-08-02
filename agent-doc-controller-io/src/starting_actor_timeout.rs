//! Route starting-actor timeout state-backbone I/O.
//!
//! The controller dispatch crate owns the actor-ready facts and log-line
//! policy. This module owns the durable per-document timeout record used to
//! coalesce repeated route timeout logs. The record is a typed route fact in
//! the project controller's `state.db`; it never creates a document hot-path
//! file or lock.

use anyhow::{Context, Result};
use std::path::PathBuf;

use agent_doc_controller::dispatch::{ActorDispatchState, AuthoritativeActorReadyFacts};
use agent_doc_state_backbone::{StateEvent, StateFact};

#[derive(Debug, Clone, PartialEq, Eq)]
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

fn state_identity(file_path: &str) -> Result<(PathBuf, String)> {
    let requested = PathBuf::from(file_path);
    let canonical = requested
        .canonicalize()
        .with_context(|| format!("canonicalize route target {}", requested.display()))?;
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(&canonical)?;
    let document_hash = agent_doc_fs::document_state_hash(&canonical)?;
    Ok((project_root, document_hash))
}

pub fn record_starting_actor_timeout(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
    log_line: &str,
) -> Result<StartingActorTimeoutLogDecision> {
    let (project_root, document_hash) = state_identity(file_path)?;
    let ledger = crate::project_controller::load_state_event_ledger(&project_root)?;
    let existing = ledger
        .project_document(&document_hash)
        .and_then(|projection| projection.route.starting_actor_timeout)
        .map(|projection| StartingActorTimeoutRecord {
            pane_id: projection.pane_id,
            generation: projection.generation,
            log_line: projection.log_line,
        });
    if existing.as_ref().is_some_and(|record| {
        record.pane_id == facts.pane_id && record.generation == facts.generation
    }) {
        return Ok(StartingActorTimeoutLogDecision::DuplicateTimeout);
    }

    let next_epoch = ledger.document_epoch(&document_hash).saturating_add(1);
    let event = StateEvent::new(
        format!(
            "starting-actor-timeout:{document_hash}:{}:{}:epoch-{next_epoch}",
            facts.pane_id, facts.generation
        ),
        StateFact::StartingActorTimeoutRecorded {
            document_hash,
            pane_id: facts.pane_id.clone(),
            generation: facts.generation,
            log_line: log_line.to_string(),
        },
    );
    if publish_reactive_state_event(&project_root, &event)? {
        Ok(StartingActorTimeoutLogDecision::NewTimeout)
    } else {
        Ok(StartingActorTimeoutLogDecision::DuplicateTimeout)
    }
}

pub fn load_starting_actor_timeout_record(file_path: &str) -> Option<StartingActorTimeoutRecord> {
    let (project_root, document_hash) = state_identity(file_path).ok()?;
    let ledger = crate::project_controller::load_state_event_ledger(&project_root).ok()?;
    ledger
        .project_document(&document_hash)?
        .route
        .starting_actor_timeout
        .map(|projection| StartingActorTimeoutRecord {
            pane_id: projection.pane_id,
            generation: projection.generation,
            log_line: projection.log_line,
        })
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

pub fn clear_starting_actor_timeout_record(file_path: &str, facts: &AuthoritativeActorReadyFacts) {
    if let Err(error) = clear_starting_actor_timeout_record_inner(file_path, facts) {
        tracing::warn!(file = %file_path, %error, "failed to clear starting-actor timeout state");
    }
}

fn clear_starting_actor_timeout_record_inner(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
) -> Result<()> {
    let (project_root, document_hash) = state_identity(file_path)?;
    let ledger = crate::project_controller::load_state_event_ledger(&project_root)?;
    let has_record = ledger
        .project_document(&document_hash)
        .is_some_and(|projection| projection.route.starting_actor_timeout.is_some());
    if !has_record {
        return Ok(());
    }
    let next_epoch = ledger.document_epoch(&document_hash).saturating_add(1);
    let event = StateEvent::new(
        format!("starting-actor-timeout-cleared:{document_hash}:epoch-{next_epoch}"),
        StateFact::StartingActorTimeoutCleared {
            document_hash,
            pane_id: facts.pane_id.clone(),
            generation: facts.generation,
        },
    );
    publish_reactive_state_event(&project_root, &event)?;
    Ok(())
}

#[cfg(not(test))]
fn publish_reactive_state_event(
    project_root: &std::path::Path,
    event: &StateEvent,
) -> Result<bool> {
    crate::project_controller::publish_state_event(project_root, event)
}

#[cfg(test)]
fn publish_reactive_state_event(
    project_root: &std::path::Path,
    event: &StateEvent,
) -> Result<bool> {
    crate::project_controller::append_state_event(project_root, event)
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
        clear_starting_actor_timeout_record(&file_path, &facts);
        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "after clear").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
    }
}
