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
use agent_doc_state_backbone::{EventLedger, StateEvent, StateFact};

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

fn load_starting_actor_timeout_state(
    project_root: &std::path::Path,
    document_hash: &str,
) -> Result<(Option<StartingActorTimeoutRecord>, u64)> {
    let conn = agent_doc_sqlite::state_store::open_state_db(project_root)?;
    let document_high_water =
        agent_doc_sqlite::state_store::state_event_document_high_water_in_db(&conn, document_hash)?;
    let mut ledger = EventLedger::new();
    for row in agent_doc_sqlite::state_store::load_starting_actor_timeout_events_from_db(
        &conn,
        document_hash,
    )? {
        let event: StateEvent = serde_json::from_str(&row.payload_json)
            .context("deserialize starting-actor timeout state event")?;
        ledger.append(event);
    }
    let record = ledger
        .project_document(document_hash)
        .and_then(|projection| projection.route.starting_actor_timeout)
        .map(|projection| StartingActorTimeoutRecord {
            pane_id: projection.pane_id,
            generation: projection.generation,
            log_line: projection.log_line,
        });
    Ok((record, document_high_water))
}

pub fn record_starting_actor_timeout(
    file_path: &str,
    facts: &AuthoritativeActorReadyFacts,
    log_line: &str,
) -> Result<StartingActorTimeoutLogDecision> {
    let (project_root, document_hash) = state_identity(file_path)?;
    let (existing, document_high_water) =
        load_starting_actor_timeout_state(&project_root, &document_hash)?;
    if existing.as_ref().is_some_and(|record| {
        record.pane_id == facts.pane_id && record.generation == facts.generation
    }) {
        return Ok(StartingActorTimeoutLogDecision::DuplicateTimeout);
    }

    let next_epoch = document_high_water.saturating_add(1);
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
    load_starting_actor_timeout_state(&project_root, &document_hash)
        .ok()?
        .0
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
    let (record, document_high_water) =
        load_starting_actor_timeout_state(&project_root, &document_hash)?;
    if record.is_none() {
        return Ok(());
    }
    let next_epoch = document_high_water.saturating_add(1);
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

    #[test]
    fn starting_actor_timeout_projection_ignores_unrelated_state_events() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, "body").unwrap();
        let file_path = doc.to_string_lossy().to_string();
        let document_hash = agent_doc_fs::document_state_hash(&doc).unwrap();
        let conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();

        for (event_id, event_document_hash, fact_type) in [
            (
                "unrelated-same-document",
                document_hash.as_str(),
                "write_applied",
            ),
            (
                "timeout-for-another-document",
                "another-document",
                "starting_actor_timeout_recorded",
            ),
        ] {
            assert!(
                agent_doc_sqlite::state_store::insert_state_event_in_db(
                    &conn,
                    &agent_doc_sqlite::state_store::StateEventInsert {
                        event_id,
                        document_hash: event_document_hash,
                        domain: "test",
                        fact_type,
                        payload_json: "not valid state-event json",
                    },
                )
                .unwrap()
            );
        }

        let facts = ready_facts("pane-a", 7, ActorDispatchState::Starting);
        assert_eq!(
            record_starting_actor_timeout(&file_path, &facts, "timeout").unwrap(),
            StartingActorTimeoutLogDecision::NewTimeout
        );
        assert!(starting_actor_timeout_record_matches(&file_path, &facts));
    }
}
