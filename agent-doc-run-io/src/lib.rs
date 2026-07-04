//! Direct-run prompt, diff, and typed auto-queue IO graph.

use agent_doc_diff as diff;
use agent_doc_document::queue_projection::strip_in_progress_marker;
use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_prompt_cache::PromptCacheBlocks;
use agent_doc_queue_io::queue_consume;
use agent_doc_session_accretion::SessionAccretionReport;
use anyhow::{Context, Result};
use std::path::Path;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RunMode {
    Append,
    Template,
}

impl RunMode {
    pub fn from_frontmatter(fm: &frontmatter::Frontmatter) -> Self {
        if fm.resolve_mode().is_template() {
            Self::Template
        } else {
            Self::Append
        }
    }

    fn cache_label(self) -> &'static str {
        match self {
            Self::Append => "append",
            Self::Template => "template",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunCycleOutcome {
    pub dispatched: bool,
    pub queue_synthetic_diff: bool,
    pub queue_consumption: Option<queue_consume::QueueConsumptionOutcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoQueueContinuation {
    Stop,
    Continue { force_fresh_agent_session: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ActiveQueuePromptState {
    Ready {
        prompt: String,
    },
    Inactive,
    StopFence {
        next_prompt: Option<String>,
    },
    TimeGate {
        start_at: String,
        next_prompt: Option<String>,
    },
    ItemModified {
        snapshot_head: Option<String>,
        document_head: Option<String>,
    },
    Unproven {
        reason: String,
        document_head: Option<String>,
    },
    Empty,
}

pub fn current_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

pub fn compute_run_diff(file: &Path) -> Result<Option<(String, bool)>> {
    if let Some(d) = agent_doc_diff_io::compute(
        &agent_doc_snapshot_io::DiffSnapshotStore::new(agent_doc_ops_log_io::log_op),
        file,
    )? {
        eprintln!("[run] diff computed ({} bytes)", d.len());
        return Ok(Some((d, false)));
    }

    if let Some(d) = active_queue_prompt_diff(file)? {
        eprintln!("[run] active queue head synthesized as prompt diff");
        return Ok(Some((d, true)));
    }

    Ok(None)
}

pub fn active_queue_prompt_diff(file: &Path) -> Result<Option<String>> {
    let ActiveQueuePromptState::Ready { prompt } = active_queue_prompt_state(file)? else {
        return Ok(None);
    };
    if let Some(command) = agent_doc_queue::queue_command::slash_command_text(&prompt) {
        eprintln!(
            "[run] active queue head is slash command {command:?}; leaving it for the managed supervisor to submit after the owner pane is idle"
        );
        return Ok(None);
    }
    Ok(Some(diff::synthetic_added_lines_diff(&prompt, "queue")))
}

pub fn active_queue_prompt_state(file: &Path) -> Result<ActiveQueuePromptState> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let (fm, _) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        &content,
        file,
        &rc.ssh_context(),
    )?;
    if fm.queue_active != Some(true) {
        return Ok(ActiveQueuePromptState::Inactive);
    }

    let components = element::parse(&content)?;
    let Some(queue_component) = components
        .iter()
        .find(|component| component.name == "queue")
    else {
        return Ok(ActiveQueuePromptState::Inactive);
    };
    if !agent_doc_queue::control_binding::explicit_queue_go_mode(
        &queue_component.attrs,
        fm.queue.as_deref(),
    ) {
        return Ok(ActiveQueuePromptState::Inactive);
    }
    let body = &content[queue_component.open_end..queue_component.close_start];
    let entries = agent_doc_queue::document_queue::parse(body)
        .context("run queue resume: failed to parse document queue")?;
    let has_auto = agent_doc_queue::document_queue::has_auto_attr(&queue_component.attrs);
    let activation =
        agent_doc_queue::document_queue::resolve_activation(&entries, has_auto, false, true);
    if !activation.active {
        return Ok(ActiveQueuePromptState::Inactive);
    }
    let document_head = agent_doc_queue::document_queue::first_prompt(&activation.entries_after)
        .map(|prompt| strip_in_progress_marker(&prompt.text));
    if document_head.is_none() {
        return Ok(ActiveQueuePromptState::Empty);
    };

    if let Some(state) = typed_queue_prompt_state(file, &content) {
        return Ok(state);
    }

    eprintln!(
        "[run] active queue has no current typed selected/deferred head; refusing markdown fallback"
    );
    Ok(ActiveQueuePromptState::Unproven {
        reason: "missing_or_stale_typed_queue_head".to_string(),
        document_head,
    })
}

pub fn typed_queue_prompt_state(file: &Path, content: &str) -> Option<ActiveQueuePromptState> {
    let canonical = file.canonicalize().ok()?;
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let document_hash = agent_doc_fs::document_state_hash(&canonical).ok()?;
    let ledger =
        agent_doc_controller_io::project_controller::load_state_event_ledger(&project_root).ok()?;
    let projection = ledger.project_document(&document_hash)?;
    let current_nodes = agent_doc_markdown_ast::mutations::item_nodes(content, "queue").ok()?;
    let current_head = current_nodes.iter().find(|node| !node.item.struck)?;
    let head = projection.queue.heads.get(&current_head.node_key)?;
    match head.phase {
        agent_doc_state_backbone::QueueHeadPhase::Selected => {
            if projection.queue.active_head.as_deref() != Some(current_head.node_key.as_str()) {
                return None;
            }
            let prompt = head.prompt_text.clone()?;
            Some(ActiveQueuePromptState::Ready { prompt })
        }
        agent_doc_state_backbone::QueueHeadPhase::Deferred => {
            let reason = head.defer_reason.as_deref()?;
            if reason == "stop_fence" {
                eprintln!("[run] active queue halted by typed stop-fence state");
                return Some(ActiveQueuePromptState::StopFence {
                    next_prompt: head.prompt_text.clone(),
                });
            }
            if let Some(start_at) = reason.strip_prefix("time_gate:") {
                eprintln!("[run] active queue deferred by typed time-gate state: {start_at}");
                return Some(ActiveQueuePromptState::TimeGate {
                    start_at: start_at.to_string(),
                    next_prompt: head.prompt_text.clone(),
                });
            }
            if reason == "item_modified" {
                eprintln!("[run] active queue halted by typed item-modified state");
                return Some(ActiveQueuePromptState::ItemModified {
                    snapshot_head: None,
                    document_head: head.prompt_text.clone(),
                });
            }
            None
        }
        _ => None,
    }
}

pub fn should_continue_auto_queue(
    file: &Path,
    outcome: &RunCycleOutcome,
    completed_queue_items: usize,
    no_git: bool,
    last_context_clear_at: Option<u64>,
) -> Result<AutoQueueContinuation> {
    if no_git || !outcome.queue_synthetic_diff {
        return Ok(AutoQueueContinuation::Stop);
    }
    let Some(queue) = outcome.queue_consumption.as_ref() else {
        return Ok(AutoQueueContinuation::Stop);
    };
    // `auto` and `start` are start triggers only. Continuation is driven by
    // typed active queue state plus explicit `go` mode; a persisted
    // `queue_active: true` plain queue stays inert. The
    // `active_queue_prompt_state` re-check below still halts on typed stop fence
    // / time gate / head-modified / inactive / empty, and refuses markdown-only
    // fallback.
    if queue.drained || queue.remaining == 0 {
        return Ok(AutoQueueContinuation::Stop);
    }

    match active_queue_prompt_state(file)? {
        ActiveQueuePromptState::Ready { prompt } => {
            let force_fresh_agent_session =
                match agent_doc_session_accretion_io::queue_context_reset_reason_if_opted_in(
                    file,
                    last_context_clear_at,
                ) {
                    Ok(Some(reason)) => {
                        eprintln!(
                            "[queue] queue continuation will start a fresh agent session before next prompt: {}",
                            reason
                        );
                        true
                    }
                    Ok(None) => false,
                    Err(err) => {
                        eprintln!(
                            "[queue] warning: failed to inspect queue context reset policy for {}: {}",
                            file.display(),
                            err
                        );
                        false
                    }
                };
            eprintln!(
                "[queue] queue continuation: completed {} item(s); launching next prompt: {:?}",
                completed_queue_items, prompt
            );
            Ok(AutoQueueContinuation::Continue {
                force_fresh_agent_session,
            })
        }
        ActiveQueuePromptState::StopFence { next_prompt } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): stop_fence before next prompt {:?}",
                completed_queue_items, next_prompt
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::TimeGate {
            start_at,
            next_prompt,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): time_gate {} before next prompt {:?}",
                completed_queue_items, start_at, next_prompt
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::ItemModified {
            snapshot_head,
            document_head,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): item_modified snapshot_head={:?} document_head={:?}",
                completed_queue_items, snapshot_head, document_head
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Unproven {
            reason,
            document_head,
        } => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): unproven_typed_queue_state reason={} document_head={:?}",
                completed_queue_items, reason, document_head
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Inactive => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): queue_inactive",
                completed_queue_items
            );
            Ok(AutoQueueContinuation::Stop)
        }
        ActiveQueuePromptState::Empty => {
            eprintln!(
                "[queue] queue continuation stopped after {} completed item(s): no_remaining_prompt",
                completed_queue_items
            );
            Ok(AutoQueueContinuation::Stop)
        }
    }
}

pub fn build_prompt(
    file: &Path,
    run_mode: RunMode,
    fm: &frontmatter::Frontmatter,
    the_diff: &str,
    content: &str,
    session_accretion: Option<&SessionAccretionReport>,
) -> String {
    let stable_prefix = build_prompt_stable_prefix(run_mode);
    let volatile_suffix =
        build_prompt_volatile_suffix(file, run_mode, fm, the_diff, content, session_accretion);
    PromptCacheBlocks::new(stable_prefix, volatile_suffix).render()
}

pub fn prompt_cache_routing_affinity(
    run_mode: RunMode,
    agent_name: &str,
    resolved_model: Option<&str>,
) -> String {
    format!(
        "agent_doc_run:v1;agent={agent_name};model={};mode={}",
        resolved_model.unwrap_or("<default>"),
        run_mode.cache_label()
    )
}

fn build_prompt_stable_prefix(run_mode: RunMode) -> String {
    let response_format = match run_mode {
        RunMode::Template => {
            "Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->"
        }
        RunMode::Append => {
            "Write your response in markdown.\n\
             Do not include a ## Assistant heading - it will be added automatically.\n\
             If the volatile payload contains inline prompt-bearing edits, classify them as prompt targets vs content edits before responding."
        }
    };
    format!(
        "<agent_doc_prompt_stable_prefix>\n\
         You are responding inside an agent-doc markdown session.\n\n\
         <response_contract>\n{}\n\
         </response_contract>\n\n\
         <turn_payload_contract>\n\
         Read the volatile turn payload after the cache boundary before acting. Queue heads, status advisories, compaction/accretion diagnostics, diffs, and document excerpts in that payload are current for this turn.\n\
         </turn_payload_contract>\n\
         </agent_doc_prompt_stable_prefix>",
        response_format
    )
}

fn build_prompt_volatile_suffix(
    file: &Path,
    run_mode: RunMode,
    fm: &frontmatter::Frontmatter,
    the_diff: &str,
    content: &str,
    session_accretion: Option<&SessionAccretionReport>,
) -> String {
    let prompt_bearing_changes = diff::format_prompt_bearing_changes(the_diff)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements =
        agent_doc_prompt_context::format_active_format_requirements(content)
            .map(|section| format!("\n\n{}\n", section))
            .unwrap_or_default();
    let rc = agent_doc_run_context_io::RunContext::new(file.to_path_buf());
    let ssh_context = rc.ssh_context();
    let document_section = agent_doc_prompt_context_io::build_document_section_with_ssh_context(
        file,
        the_diff,
        content,
        session_accretion,
        &ssh_context,
    );
    match (run_mode, fm.resume.is_some()) {
        (RunMode::Template, true) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content.\n\
             </agent_doc_prompt_volatile_suffix>",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
        ),
        (RunMode::Template, false) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content.\n\
             </agent_doc_prompt_volatile_suffix>",
            active_format_requirements, content
        ),
        (RunMode::Append, true) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content.\n\
             </agent_doc_prompt_volatile_suffix>",
            the_diff, prompt_bearing_changes, active_format_requirements, document_section
        ),
        (RunMode::Append, false) => format!(
            "<agent_doc_prompt_volatile_suffix>\n\
             The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. If the user asked questions or prompt-bearing edits inline (e.g., in blockquotes or prior responses), address those too.\n\
             </agent_doc_prompt_volatile_suffix>",
            active_format_requirements, content
        ),
    }
}
