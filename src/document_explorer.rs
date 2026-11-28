//! Shared document prompt transaction for local Explorer hosts.

use agent_doc_editor_surface_io::DocumentPromptSubmission;
use anyhow::{Context, Result};
use std::path::Path;
use std::process::{Child, Command, Stdio};

pub(crate) const MAX_BODY_BYTES: usize = 4 * 1024 * 1024;

#[derive(Debug)]
pub(crate) enum PromptFailure {
    AlreadyPlaced,
    Conflict(String),
    TooLarge,
    Internal(String),
}

impl std::fmt::Display for PromptFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Conflict(message) | Self::Internal(message) => message,
            Self::AlreadyPlaced => "This prompt was already placed.\n",
            Self::TooLarge => "the document is too large\n",
        })
    }
}

impl std::error::Error for PromptFailure {}

pub(crate) fn load_open_cycle(doc: &Path) -> Result<Option<agent_doc_cycle_state_io::CycleState>> {
    Ok(
        agent_doc_cycle_state_io::load_with_closeout_projection(doc)?
            .filter(|cycle| cycle.is_open()),
    )
}

pub(crate) fn explorer_active_prompts(
    cycle: &agent_doc_cycle_state_io::CycleState,
) -> impl Iterator<Item = &String> {
    cycle
        .prompt_targets
        .iter()
        .chain(&cycle.selected_free_text_queue_heads)
        .chain(&cycle.absorbed_steering_prompts)
        .filter(|prompt| prompt.contains("repo-context-answer:"))
}

pub(crate) fn persist_prompt(
    doc: &Path,
    submission: DocumentPromptSubmission,
    source: &'static str,
) -> std::result::Result<(String, bool), PromptFailure> {
    let project = agent_doc_project_root_io::project_root_containing(doc).ok_or_else(|| {
        PromptFailure::Conflict("prompt submission requires an Agent Doc project\n".into())
    })?;
    let owned_doc = doc.to_path_buf();
    agent_doc_queue_io::write_queue::DocumentWriteQueue::new(
        &agent_doc_document_realtime_io::SESSION_ACTOR_WRITE_QUEUE,
        &project,
        &doc.to_string_lossy(),
    )
    .agent_write(move || {
        let current =
            agent_doc_document_realtime_io::peek_current_document_content(&owned_doc, source)
                .map_err(|error| {
                    PromptFailure::Conflict(format!("document authority unavailable: {error:#}\n"))
                })?;
        if agent_doc_editor_surface_io::document_prompt_receipt_present(
            &current,
            &submission.request_id,
        ) {
            return Err(PromptFailure::AlreadyPlaced);
        }
        let content = agent_doc_editor_surface_io::insert_document_prompt(&current, &submission)
            .map_err(|error| PromptFailure::Conflict(format!("{error:#}\n")))?;
        if content.len() > MAX_BODY_BYTES {
            return Err(PromptFailure::TooLarge);
        }
        let active_cycle = load_open_cycle(&owned_doc)
            .map_err(|error| {
                PromptFailure::Internal(format!("failed to inspect active cycle: {error:#}\n"))
            })?
            .is_some();
        agent_doc_document_realtime_io::persist_exact_operator_edit_through_authority(
            &owned_doc, &current, &content, source,
        )
        .map_err(|error| {
            PromptFailure::Conflict(format!(
                "prompt placement could not be confirmed: {error:#}\n"
            ))
        })?;
        // Admission consumes this operator edit. A save's Git commit
        // would instead establish it as the next baseline.
        Ok((content, active_cycle))
    })
    .map_err(|error| {
        PromptFailure::Internal(format!("document write queue unavailable: {error:#}\n"))
    })?
}

pub(crate) fn spawn_prompt_run(project: &Path, doc: &Path, stderr: Stdio) -> Result<Child> {
    use agent_doc_state_backbone::pane_execution_authority::{
        AuthorityOperation, AuthorityVerdict,
    };

    let scope = agent_doc_state_backbone::DocumentScope::new();
    let verdict = agent_doc_run_io::pane_execution_authority::verdict_in(
        &scope,
        doc,
        AuthorityOperation::Mutation,
    )?;
    let binary = std::env::current_exe().context("locate Agent Doc executable")?;
    let mut command = Command::new(binary);
    if matches!(
        verdict,
        AuthorityVerdict::PermitHeadless | AuthorityVerdict::PermitUnregistered
    ) {
        command.arg("run");
    } else {
        command.args(["route", "--dispatch-only", "--debounce", "0"]);
    }
    command
        .arg(doc)
        .current_dir(project)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(stderr)
        .spawn()
        .context("start Agent Doc")
}
