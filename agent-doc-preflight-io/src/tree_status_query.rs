//! Preflight answer for the operator's queue status query (`#treestatusquery`).
//!
//! An operator who adds a `status?` child line under a queue row (or a
//! `[status?]` token on it) asks whether that subtree has actionable or
//! deletable work. Preflight answers before the diff is computed: it replaces
//! the query with binary-owned Status reply rows derived from the queue tree
//! ([`agent_doc_queue::tree_status`]) and carries the answer into the snapshot,
//! so the query is never dispatched as a prompt and never opens a model turn.
//! Only `status? deep` with rows the binary cannot classify leaves a prompt
//! for one agent turn.

use std::path::Path;

use agent_doc_queue::free_text_admission::FreeTextCoinGate;
use agent_doc_queue::tree_status::{
    AnsweredStatusQuery, answer_status_queries_settled, carry_status_answers_into_snapshot,
};
use anyhow::Result;

/// Answer every settled status query in `file`. Returns the answered queries;
/// an empty list when there was none. Best-effort at the call site: a failure
/// answers nothing and never blocks admission.
pub fn run_tree_status_query_maintenance(
    file: &Path,
    coin_gate: &FreeTextCoinGate,
) -> Result<Vec<AnsweredStatusQuery>> {
    let source = "preflight_tree_status_query";
    let content = match super::current_text_via_preflight_authority_retrying(file, source)? {
        Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
        Some(agent_doc_crdt_relay_io::CurrentText::Detached) | None => {
            match std::fs::read_to_string(file) {
                Ok(text) => text,
                Err(_) => return Ok(Vec::new()),
            }
        }
        Some(_) => return Ok(Vec::new()),
    };
    let Some(answers) =
        answer_status_queries_settled(&content, &|line| coin_gate.allows_coining(line))?
    else {
        return Ok(Vec::new());
    };
    let project_root = agent_doc_project_root_io::project_root_containing(file);
    let persisted = super::persist_queue_maintenance_doc(
        file,
        &answers.content,
        &content,
        project_root.as_deref(),
        source,
    )?;
    if let Ok(Some(snapshot)) = agent_doc_snapshot_io::load_document_baseline(file)
        && let Some(new_snapshot) =
            carry_status_answers_into_snapshot(&snapshot, &content, &persisted)
        && let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &new_snapshot,
            agent_doc_ops_log_io::log_op,
        )
    {
        eprintln!("[preflight] status query: snapshot sync warning: {err}");
    }
    for answer in &answers.answered {
        eprintln!(
            "[preflight] status query: answered for {} in queue {}: {}{} (#treestatusquery)",
            answer.target,
            answer.queue,
            answer.counts,
            if answer.queued_agent_turn {
                "; deep review turn queued"
            } else {
                ""
            }
        );
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "tree_status_query_answered file={} queue={} deep_turn={}",
                file.display(),
                answer.queue,
                answer.queued_agent_turn
            ),
        );
    }
    Ok(answers.answered)
}
