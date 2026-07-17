//! Route dispatch queue writeback I/O.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};

use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_turn::cycle_ack::PromptBearingRouteContext;

pub type RouteWriteDocumentFn =
    fn(file: &Path, next_content: &str, previous_content: &str, reason: &str) -> Result<()>;

#[derive(Clone, Copy)]
pub struct RouteQueueEffects {
    pub write_document: RouteWriteDocumentFn,
}

const ROUTE_QUEUE_MAX_WRITE_ATTEMPTS: usize = 3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteQueueEnqueueOutcome {
    pub prompt_text: String,
    pub appended: bool,
    pub already_present: bool,
    pub superseded: bool,
    pub component_created: bool,
    pub activated: bool,
}

/// Enqueue a routed dispatch prompt into a document's `agent:queue`.
///
/// `priority` marks a manual operator dispatch into a busy/blocked pane: it
/// preempts pending auto-loop items by inserting ahead of the first queued
/// prompt. Non-priority callers keep the tail-append plus lone stale-prompt
/// supersede behavior.
pub fn enqueue_route_dispatch_prompt(
    file: &Path,
    prompt_text: &str,
    source: &str,
    priority: bool,
    effects: RouteQueueEffects,
) -> Result<RouteQueueEnqueueOutcome> {
    let _lock = acquire_route_queue_lock(file)?;
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        let original = agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "route_dispatch_queue_enqueue",
        )?;
        let update = agent_doc_queue::route_dispatch::prepare_route_dispatch_queue_update(
            &original,
            prompt_text,
            priority,
        )?;
        if let Some(parse_err) = update.unparseable_queue_error.as_deref() {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_queue_dispatch_unparseable_preserved file={} prompt_hash={} reason={}",
                    file.display(),
                    agent_doc_hash::content_hash(&update.prompt_text),
                    parse_err
                ),
            );
        }

        let content = update.content;
        let activated = content != original;
        if activated {
            match (effects.write_document)(file, &content, &original, "route_dispatch_queue") {
                Ok(()) => {
                    agent_doc_snapshot_io::checkpoint_document_baseline(
                        file,
                        &content,
                        agent_doc_ops_log_io::log_op,
                    )
                    .with_context(|| {
                        format!(
                            "failed to sync snapshot after queueing dispatch for {}",
                            file.display()
                        )
                    })?;
                }
                Err(err)
                    if attempt < ROUTE_QUEUE_MAX_WRITE_ATTEMPTS
                        && is_retryable_crdt_merge_error(&err) =>
                {
                    log_route_queue_write_retry(
                        file,
                        "route_dispatch_queue",
                        attempt,
                        &content,
                        &original,
                        &err,
                    );
                    continue;
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!(
                            "failed to converge queued dispatch for {} through editor IPC/disk",
                            file.display()
                        )
                    });
                }
            }
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_dispatch_queued file={} source={} appended={} already_present={} superseded={} component_created={} activated={} prompt={:?}",
                file.display(),
                source,
                update.appended,
                update.already_present,
                update.superseded,
                update.component_created,
                activated,
                update.prompt_text
            ),
        );
        return Ok(RouteQueueEnqueueOutcome {
            prompt_text: update.prompt_text,
            appended: update.appended,
            already_present: update.already_present,
            superseded: update.superseded,
            component_created: update.component_created,
            activated,
        });
    }
}

pub fn enqueue_exchange_slash_command_for_idle_drain(
    file: &Path,
    context: &PromptBearingRouteContext,
    source: &str,
    effects: RouteQueueEffects,
) -> Result<Option<RouteQueueEnqueueOutcome>> {
    let Some(command) = context.slash_command.as_deref() else {
        return Ok(None);
    };
    let queued = enqueue_route_dispatch_prompt(file, command, source, true, effects)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "route_exchange_slash_command_queued file={} source={} command={:?} appended={} already_present={} superseded={} activated={}",
            file.display(),
            source,
            command,
            queued.appended,
            queued.already_present,
            queued.superseded,
            queued.activated
        ),
    );
    Ok(Some(queued))
}

pub fn inactive_route_queue_head(file: &Path) -> Result<Option<String>> {
    if !file.exists() {
        return Ok(None);
    }
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "route_inactive_queue_head",
    )?;
    inactive_route_queue_head_in_content(file, &content)
}

pub fn inactive_route_queue_head_in_content(file: &Path, content: &str) -> Result<Option<String>> {
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    let (fm, _) = agent_doc_frontmatter_io::session::parse_for_file_with_context(
        content,
        file,
        &rc.ssh_context(),
    )?;
    let committed_snapshot = match agent_doc_snapshot_io::load_document_baseline(file) {
        Ok(snapshot) => snapshot,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_uncommitted_head_snapshot_unreadable file={} err={} decision=allow",
                    file.display(),
                    err
                ),
            );
            None
        }
    };
    match agent_doc_queue::route_dispatch::inactive_route_queue_head(
        content,
        fm.queue_active,
        committed_snapshot.as_deref(),
    )? {
        agent_doc_queue::route_dispatch::RouteInactiveQueueHead::None => Ok(None),
        agent_doc_queue::route_dispatch::RouteInactiveQueueHead::Dispatchable(head_text) => {
            Ok(Some(head_text))
        }
        agent_doc_queue::route_dispatch::RouteInactiveQueueHead::Uncommitted(head_text) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_dispatch_uncommitted_head file={} decision=defer reason=head_not_in_committed_snapshot head={:?}",
                    file.display(),
                    agent_doc_secret_redact::redact(&head_text)
                ),
            );
            Ok(None)
        }
    }
}

pub fn activate_existing_route_queue_head(
    file: &Path,
    source: &str,
    effects: RouteQueueEffects,
) -> Result<Option<RouteQueueEnqueueOutcome>> {
    let _lock = acquire_route_queue_lock(file)?;
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        let original = agent_doc_document_realtime_io::try_resolve_current_document_content(
            file,
            "route_queue_activation",
        )?;
        let Some(prompt_text) = inactive_route_queue_head_in_content(file, &original)? else {
            return Ok(None);
        };
        let content =
            agent_doc_queue::route_dispatch::activate_existing_route_queue_content(&original)?;
        let activated = content != original;
        if activated {
            match (effects.write_document)(file, &content, &original, "route_queue_activation") {
                Ok(()) => {
                    agent_doc_snapshot_io::checkpoint_document_baseline(
                        file,
                        &content,
                        agent_doc_ops_log_io::log_op,
                    )
                    .with_context(|| {
                        format!(
                            "failed to sync snapshot after activating queue for {}",
                            file.display()
                        )
                    })?;
                }
                Err(err)
                    if attempt < ROUTE_QUEUE_MAX_WRITE_ATTEMPTS
                        && is_retryable_crdt_merge_error(&err) =>
                {
                    log_route_queue_write_retry(
                        file,
                        "route_queue_activation",
                        attempt,
                        &content,
                        &original,
                        &err,
                    );
                    continue;
                }
                Err(err) => {
                    return Err(err).with_context(|| {
                        format!("failed to activate queue in {}", file.display())
                    });
                }
            }
        }
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "route_existing_queue_head_activated file={} source={} activated={} prompt={:?}",
                file.display(),
                source,
                activated,
                prompt_text
            ),
        );
        return Ok(Some(RouteQueueEnqueueOutcome {
            prompt_text,
            appended: false,
            already_present: true,
            superseded: false,
            component_created: false,
            activated,
        }));
    }
}

fn is_retryable_crdt_merge_error(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains("recovery=retry_crdt_merge")
}

fn log_route_queue_write_retry(
    file: &Path,
    reason: &str,
    attempt: usize,
    attempted_content: &str,
    expected_current: &str,
    err: &anyhow::Error,
) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{reason}_retry file={} attempt={} max_attempts={} expected_hash={} attempted_hash={} reason=retry_crdt_merge error={}",
            file.display(),
            attempt,
            ROUTE_QUEUE_MAX_WRITE_ATTEMPTS,
            agent_doc_hash::content_hash(expected_current),
            agent_doc_hash::content_hash(attempted_content),
            agent_doc_secret_redact::redact(&format!("{err:#}")).replace('\n', " "),
        ),
    );
}

fn route_queue_lock_path(file: &Path) -> Result<PathBuf> {
    let canonical = std::fs::canonicalize(file)
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let base = agent_doc_project_root_io::project_root_containing(&canonical)
        .or_else(|| canonical.parent().map(Path::to_path_buf))
        .ok_or_else(|| {
            anyhow::anyhow!("failed to resolve queue lock root for {}", file.display())
        })?;
    let hash = agent_doc_fs::document_state_hash_from_str(&canonical.to_string_lossy());
    Ok(base
        .join(".agent-doc/route-queue")
        .join(format!("{hash}.lock")))
}

fn acquire_route_queue_lock(file: &Path) -> Result<File> {
    let lock_path = route_queue_lock_path(file)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let lock = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open route queue lock {}", lock_path.display()))?;
    lock.lock_exclusive()
        .with_context(|| format!("failed to acquire route queue lock {}", lock_path.display()))?;
    Ok(lock)
}
