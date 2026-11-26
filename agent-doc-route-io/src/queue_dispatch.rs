//! Route dispatch queue writeback I/O.

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_run_context_io::AgentDocContextExt;
use agent_doc_turn::prompt_bearing_route::PromptBearingRouteContext;

pub type RouteWriteDocumentFn =
    fn(file: &Path, next_content: &str, previous_content: &str, reason: &str) -> Result<()>;

#[derive(Debug, Clone, Copy)]
pub struct RouteQueueEffects {
    pub write_document: RouteWriteDocumentFn,
    /// `Run Agent Doc` records its explicit-send proof before route admission.
    /// Refresh that proof after this route-owned queue append converges.
    pub refresh_explicit_send_after_enqueue: bool,
}

const ROUTE_QUEUE_MAX_WRITE_ATTEMPTS: u32 = 3;
const ROUTE_QUEUE_MAX_RESOLVE_ATTEMPTS: u32 = 2;

/// `#witnessretrygate` — how long the refused resolve retry waits for the editor
/// replica registration to actually land before re-asking.
///
/// Matches `DOCUMENT_MODEL_ENSURE_TIMEOUT_MS`, the full-window budget the model
/// ensure grants a registration that arrives late. This is spent only on a path
/// that otherwise fails the route outright, so it trades latency for a dispatch
/// that used to be a hard `Run Agent Doc` error.
const ROUTE_QUEUE_RESOLVE_WITNESS_WAIT: std::time::Duration =
    std::time::Duration::from_millis(5_000);

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
    // `#implementlazilyowned`: Lazily owns the re-observe loop. Each attempt
    // resolves the live authority, derives the queue successor, and
    // compare-and-applies it; a `retry_crdt_merge` refusal is `Superseded` and
    // the still-wanted dispatch is re-desired from a fresh cut.
    let drive = agent_doc_effect_cas::drive_effect_epochs(
        &format!("route-dispatch-queue:{}", file.display()),
        ROUTE_QUEUE_UNOBSERVED_AUTHORITY,
        ROUTE_QUEUE_MAX_WRITE_ATTEMPTS,
        &mut |_attempt| {
            let original =
                resolve_route_queue_document_content(file, "route_dispatch_queue_enqueue", || {
                    agent_doc_document_realtime_io::try_resolve_current_document_content(
                        file,
                        "route_dispatch_queue_enqueue",
                    )
                })?;
            let update = agent_doc_queue::route_dispatch::prepare_route_dispatch_queue_update(
                &original,
                prompt_text,
                priority,
            )?;
            // GH #40 / `#ghrecvpolicy`: creating `agent:run` on a document
            // follows its workspace's `[receive] auto_create_components`
            // policy for senders other than the workspace owner. The sender
            // is this process's user, or the project controller request's
            // principal when the route runs inside the controller.
            if update.component_created {
                agent_doc_project_config_io::workspace_access::enforce_component_auto_create(
                    agent_doc_project_config_io::workspace_access::sender_principal(),
                    file,
                    "queue",
                )?;
            }
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
            let activated = update.content != original;
            if activated {
                let refused = (effects.write_document)(
                    file,
                    &update.content,
                    &original,
                    "route_dispatch_queue",
                );
                if let Some(step) = route_queue_write_step(refused, &update.content, &original)
                    .map_err(|err| {
                        err.context(format!(
                            "failed to converge queued dispatch for {} through editor IPC/disk",
                            file.display()
                        ))
                    })?
                {
                    return Ok(step);
                }
            }
            Ok(agent_doc_effect_cas::EpochStep::Applied {
                authority: agent_doc_hash::content_hash(&update.content),
                receipt: (update, original, activated),
            })
        },
        &mut |attempt, refusal: &RouteQueueWriteRefusal| {
            refusal.log(file, "route_dispatch_queue", attempt);
            Ok(agent_doc_effect_cas::Pace::Continue)
        },
    )?;
    let (update, original, activated) = match drive {
        agent_doc_effect_cas::EpochDrive::Applied(applied) => applied,
        agent_doc_effect_cas::EpochDrive::Exhausted { reason, .. }
        | agent_doc_effect_cas::EpochDrive::Stopped { reason, .. } => {
            return Err(reason.error).with_context(|| {
                format!(
                    "failed to converge queued dispatch for {} through editor IPC/disk",
                    file.display()
                )
            });
        }
    };
    let content = update.content;
    if activated {
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
        if effects.refresh_explicit_send_after_enqueue {
            agent_doc_session_check_io::midturn_steering::refresh_explicit_send_after_route_enqueue(
                file,
                &original,
                &content,
            )?;
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
    Ok(RouteQueueEnqueueOutcome {
        prompt_text: update.prompt_text,
        appended: update.appended,
        already_present: update.already_present,
        superseded: update.superseded,
        component_created: update.component_created,
        activated,
    })
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
    if crate::invocation::trigger_only() {
        return Ok(None);
    }
    // `#implementlazilyowned`: same Lazily-owned re-observe epochs as
    // [`enqueue_route_dispatch_prompt`].
    let drive = agent_doc_effect_cas::drive_effect_epochs(
        &format!("route-queue-activation:{}", file.display()),
        ROUTE_QUEUE_UNOBSERVED_AUTHORITY,
        ROUTE_QUEUE_MAX_WRITE_ATTEMPTS,
        &mut |_attempt| {
            let original =
                resolve_route_queue_document_content(file, "route_queue_activation", || {
                    agent_doc_document_realtime_io::try_resolve_current_document_content(
                        file,
                        "route_queue_activation",
                    )
                })?;
            let Some(prompt_text) = inactive_route_queue_head_in_content(file, &original)? else {
                return Ok(agent_doc_effect_cas::EpochStep::Applied {
                    authority: agent_doc_hash::content_hash(&original),
                    receipt: None,
                });
            };
            let content =
                agent_doc_queue::route_dispatch::activate_existing_route_queue_content(&original)?;
            let activated = content != original;
            if activated {
                let refused =
                    (effects.write_document)(file, &content, &original, "route_queue_activation");
                if let Some(step) =
                    route_queue_write_step(refused, &content, &original).map_err(|err| {
                        err.context(format!("failed to activate queue in {}", file.display()))
                    })?
                {
                    return Ok(step);
                }
            }
            Ok(agent_doc_effect_cas::EpochStep::Applied {
                authority: agent_doc_hash::content_hash(&content),
                receipt: Some((prompt_text, content, activated)),
            })
        },
        &mut |attempt, refusal: &RouteQueueWriteRefusal| {
            refusal.log(file, "route_queue_activation", attempt);
            Ok(agent_doc_effect_cas::Pace::Continue)
        },
    )?;
    let (prompt_text, content, activated) = match drive {
        agent_doc_effect_cas::EpochDrive::Applied(Some(applied)) => applied,
        agent_doc_effect_cas::EpochDrive::Applied(None) => return Ok(None),
        agent_doc_effect_cas::EpochDrive::Exhausted { reason, .. }
        | agent_doc_effect_cas::EpochDrive::Stopped { reason, .. } => {
            return Err(reason.error)
                .with_context(|| format!("failed to activate queue in {}", file.display()));
        }
    };
    if activated {
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
    Ok(Some(RouteQueueEnqueueOutcome {
        prompt_text,
        appended: false,
        already_present: true,
        superseded: false,
        component_created: false,
        activated,
    }))
}

/// Opaque initial authority of a route queue drive: every attempt resolves the
/// live cut itself, so the first epoch compares against nothing observed yet.
const ROUTE_QUEUE_UNOBSERVED_AUTHORITY: &str = "route-queue:unobserved";

/// A `retry_crdt_merge` refusal of one route queue write: the reason Lazily
/// records with the supersession and the diagnostic `between` renders.
struct RouteQueueWriteRefusal {
    attempted: String,
    expected: String,
    error: anyhow::Error,
}

impl RouteQueueWriteRefusal {
    fn log(&self, file: &Path, reason: &str, attempt: u32) {
        log_route_queue_write_retry(
            file,
            reason,
            attempt as usize,
            &self.attempted,
            &self.expected,
            &self.error,
        );
    }
}

/// Adapter mapping of one route queue write: `Ok(None)` = applied, a
/// `retry_crdt_merge` refusal = Lazily `Superseded` (the relay current moved
/// past `expected`; it reports no newer cut, so the next attempt re-resolves),
/// any other error = rejection.
#[allow(clippy::type_complexity)]
fn route_queue_write_step<R>(
    written: Result<()>,
    attempted: &str,
    expected: &str,
) -> Result<Option<agent_doc_effect_cas::EpochStep<R, RouteQueueWriteRefusal>>> {
    match written {
        Ok(()) => Ok(None),
        Err(error) if is_retryable_crdt_merge_error(&error) => {
            Ok(Some(agent_doc_effect_cas::EpochStep::Superseded {
                observed: format!("refused:{}", agent_doc_hash::content_hash(expected)),
                reason: RouteQueueWriteRefusal {
                    attempted: attempted.to_string(),
                    expected: expected.to_string(),
                    error,
                },
            }))
        }
        Err(error) => Err(error),
    }
}

fn is_retryable_crdt_merge_error(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains("recovery=retry_crdt_merge")
}

fn resolve_route_queue_document_content(
    file: &Path,
    reason: &str,
    resolve: impl FnMut() -> Result<String>,
) -> Result<String> {
    resolve_route_queue_document_content_observing(
        file,
        reason,
        resolve,
        |file| {
            agent_doc_document_realtime_io::wait_for_editor_replica_liveness_change(
                file,
                ROUTE_QUEUE_RESOLVE_WITNESS_WAIT,
            )
        },
        route_sender_recycle_observation,
        |file| wait_for_route_sender_installed(file, ROUTE_QUEUE_RESOLVE_WITNESS_WAIT),
    )
}

/// Read-only view of an already-requested install-fanout recycle.
///
/// `sender_on_installed_build` is false when the host supervisor is stale or
/// its binary cannot be proved. This does not write a recycle request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RouteSenderRecycleObservation {
    outstanding_install_fanout: bool,
    sender_on_installed_build: bool,
    resolve_blocks_sender_reexec: bool,
}

fn route_sender_recycle_observation(file: &Path) -> RouteSenderRecycleObservation {
    let facts =
        agent_doc_controller_io::project_controller::route_install_fanout_sender_facts(file);
    RouteSenderRecycleObservation {
        outstanding_install_fanout: facts.outstanding_install_fanout,
        // Unproven freshness is not "on the installed build": an outstanding
        // install-fanout recycle must not fall through to the editor-library refusal.
        sender_on_installed_build: facts.sender_on_installed_build.unwrap_or(false),
        resolve_blocks_sender_reexec: facts.resolve_blocks_sender_reexec,
    }
}

fn wait_for_route_sender_installed(file: &Path, budget: std::time::Duration) -> bool {
    wait_until_with(
        budget,
        || {
            let observed = route_sender_recycle_observation(file);
            // The request is consumed when the supervisor takes the re-exec.
            // That is the same signal as the sender mapping the installed build:
            // retry the resolve instead of prescribing reload-lib.
            observed.sender_on_installed_build || !observed.outstanding_install_fanout
        },
        std::thread::sleep,
    )
}

fn wait_until_with(
    budget: std::time::Duration,
    mut ready: impl FnMut() -> bool,
    mut sleep: impl FnMut(std::time::Duration),
) -> bool {
    const POLL: std::time::Duration = std::time::Duration::from_millis(50);
    let deadline = std::time::Instant::now() + budget;
    loop {
        if ready() {
            return true;
        }
        let now = std::time::Instant::now();
        if now >= deadline {
            return false;
        }
        sleep(POLL.min(deadline - now));
    }
}

fn outstanding_install_fanout_resolve_error(file: &Path, reason: &str) -> anyhow::Error {
    anyhow::anyhow!(
        "{reason}: failed to resolve current document {} while the editor is still attached. {}",
        file.display(),
        agent_doc_turn::write_ownership::install_fanout_outstanding_replica_recovery(),
    )
}

/// `#witnessretrygate` — resolve the document, retrying once across a *proven*
/// editor replica registration change.
///
/// The witness wait is injected so tests drive the retry gate without a live
/// reliable-sync plane. Sender-recycle observations are absent here; production
/// resolution uses [`resolve_route_queue_document_content`].
#[cfg(test)]
fn resolve_route_queue_document_content_awaiting(
    file: &Path,
    reason: &str,
    resolve: impl FnMut() -> Result<String>,
    await_liveness_change: impl FnMut(&Path) -> bool,
) -> Result<String> {
    resolve_route_queue_document_content_observing(
        file,
        reason,
        resolve,
        await_liveness_change,
        |_file| RouteSenderRecycleObservation {
            outstanding_install_fanout: false,
            sender_on_installed_build: false,
            resolve_blocks_sender_reexec: false,
        },
        |_file| false,
    )
}

/// Resolve the document, accounting for an already-requested `install_fanout`
/// recycle of the route-owned sender.
///
/// A missing-replica refusal still retries when the editor witness changes.
/// When that refusal would prescribe `reload-lib` and an install-fanout recycle
/// is already outstanding, the route waits for the scheduled re-exec (or
/// retries once the sender is on the installed build) instead of failing closed
/// on that remedy. It does not start a second recycle and it does not read disk.
fn resolve_route_queue_document_content_observing(
    file: &Path,
    reason: &str,
    mut resolve: impl FnMut() -> Result<String>,
    mut await_liveness_change: impl FnMut(&Path) -> bool,
    mut observe_sender_recycle: impl FnMut(&Path) -> RouteSenderRecycleObservation,
    mut await_sender_installed: impl FnMut(&Path) -> bool,
) -> Result<String> {
    // `#implementlazilyowned`: Lazily owns the bounded resolve. A recovery
    // refusal is `Retryable` (the authority is unchanged until a registration
    // lands); the witness wait is the pace, and an unchanged witness stops the
    // drive fail-closed unless an outstanding install-fanout recycle says the
    // stale sender is the cause.
    let drive = agent_doc_effect_cas::drive_effect_epochs(
        &format!("route-queue-resolve:{reason}:{}", file.display()),
        ROUTE_QUEUE_UNOBSERVED_AUTHORITY,
        ROUTE_QUEUE_MAX_RESOLVE_ATTEMPTS,
        &mut |_attempt| match resolve() {
            Ok(content) => Ok(agent_doc_effect_cas::EpochStep::Applied {
                authority: agent_doc_hash::content_hash(&content),
                receipt: content,
            }),
            Err(err) if is_retryable_editor_authority_recovery_error(&err) => {
                Ok(agent_doc_effect_cas::EpochStep::Retryable(err))
            }
            Err(err) => Err(err),
        },
        &mut |attempt, err: &anyhow::Error| {
            let observed = observe_sender_recycle(file);
            let attached_reload_editor_refusal =
                agent_doc_document_realtime_io::attached_editor_disk_read_refusal(err).is_some_and(
                    |refusal| {
                        refusal.recovery
                            == agent_doc_turn::authority_recovery::AttachedEditorRecoveryDecision::ReloadEditor
                    },
                );
            let decision =
                agent_doc_turn::authority_recovery::decide_route_missing_replica_recovery(
                    agent_doc_turn::authority_recovery::RouteMissingReplicaRecoveryFacts {
                        attached_reload_editor_refusal,
                        outstanding_install_fanout: observed.outstanding_install_fanout,
                        sender_on_installed_build: observed.sender_on_installed_build,
                        resolve_blocks_sender_reexec: observed.resolve_blocks_sender_reexec,
                    },
                );
            use agent_doc_turn::authority_recovery::RouteMissingReplicaRecovery::{
                DeferToScheduledRecycle, RetrySenderInstalled, WaitForScheduledRecycle, WitnessGate,
            };
            if !matches!(decision, WitnessGate) {
                let sender_installed = match decision {
                    WaitForScheduledRecycle => await_sender_installed(file),
                    RetrySenderInstalled => true,
                    DeferToScheduledRecycle | WitnessGate => false,
                };
                let terminal = match decision {
                    WaitForScheduledRecycle if sender_installed => false,
                    RetrySenderInstalled => false,
                    WaitForScheduledRecycle | DeferToScheduledRecycle => true,
                    WitnessGate => false,
                };
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "{reason}_resolve_retry file={} attempt={} max_attempts={} reason=install_fanout_sender_recycle decision={} sender_installed={} error={}",
                        file.display(),
                        attempt,
                        ROUTE_QUEUE_MAX_RESOLVE_ATTEMPTS,
                        match decision {
                            WaitForScheduledRecycle if sender_installed => "retry_sender_installed",
                            WaitForScheduledRecycle => "defer_scheduled_recycle",
                            RetrySenderInstalled => "retry_sender_installed",
                            DeferToScheduledRecycle => "defer_scheduled_recycle",
                            WitnessGate => "witness_gate",
                        },
                        sender_installed,
                        agent_doc_secret_redact::redact(&format!("{err:#}")).replace('\n', " "),
                    ),
                );
                return if terminal {
                    Err(outstanding_install_fanout_resolve_error(file, reason))
                } else {
                    Ok(agent_doc_effect_cas::Pace::Continue)
                };
            }
            // Re-asking against the same liveness witness is a provable
            // no-op: both missing-replica recovery latches are keyed on it,
            // so the second attempt would skip the re-registration loop and
            // the terminal rebuild and bail on this same refusal. Only a
            // registration that actually landed earns the retry.
            let witness_changed = await_liveness_change(file);
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "{reason}_resolve_retry file={} attempt={} max_attempts={} reason=editor_authority_recovery witness_changed={} decision={} error={}",
                    file.display(),
                    attempt,
                    ROUTE_QUEUE_MAX_RESOLVE_ATTEMPTS,
                    witness_changed,
                    if witness_changed {
                        "retry"
                    } else {
                        "fail_closed"
                    },
                    agent_doc_secret_redact::redact(&format!("{err:#}")).replace('\n', " "),
                ),
            );
            Ok(if witness_changed {
                agent_doc_effect_cas::Pace::Continue
            } else {
                agent_doc_effect_cas::Pace::Stop
            })
        },
    )?;
    match drive {
        agent_doc_effect_cas::EpochDrive::Applied(content) => Ok(content),
        agent_doc_effect_cas::EpochDrive::Exhausted { reason, .. }
        | agent_doc_effect_cas::EpochDrive::Stopped { reason, .. } => Err(reason),
    }
}

/// Whether `err` is the attached-editor refusal raised when authority recovery
/// exhausts, for either family the resolver can report.
///
/// `resolve_editor_unavailable_disk_read_fallback` interpolates the observation
/// family into one message — `"<reason> recovery exhausted and disk read
/// authority is refused"` — and `reason` is `missing_replica` or `sync_pending`.
/// Matching the `sync_pending` spelling alone left the whole missing-replica
/// family without the bounded retry, which is the family that turns a JetBrains
/// `Run Agent Doc` into `route_queue_activation: failed to resolve current
/// document ...` on the first refusal.
fn is_retryable_editor_authority_recovery_error(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains("recovery exhausted and disk read authority is refused")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A witness wait that reports a landed registration, so the retry is armed.
    fn witness_advanced(_file: &Path) -> bool {
        true
    }

    /// A witness wait that reports nothing landed inside the budget.
    fn witness_unchanged(_file: &Path) -> bool {
        false
    }

    #[test]
    fn route_queue_resolution_retries_editor_authority_recovery_once() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;

        let content = resolve_route_queue_document_content_awaiting(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                if calls == 1 {
                    anyhow::bail!(
                        "sync_pending recovery exhausted and disk read authority is refused"
                    );
                }
                Ok("live editor content".to_string())
            },
            witness_advanced,
        )
        .unwrap();

        assert_eq!(content, "live editor content");
        assert_eq!(calls, 2);
    }

    /// `#witnessretrygate` — the `missing_replica` family is the one a JetBrains
    /// `Run Agent Doc` actually hits, and the predicate used to match only the
    /// `sync_pending` spelling, so route failed the dispatch on the first refusal.
    #[test]
    fn route_queue_resolution_retries_missing_replica_authority_recovery() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;

        let content = resolve_route_queue_document_content_awaiting(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                if calls == 1 {
                    anyhow::bail!(
                        "editor is still attached for {}; missing_replica recovery exhausted and disk read authority is refused",
                        file.display()
                    );
                }
                Ok("live editor content".to_string())
            },
            witness_advanced,
        )
        .unwrap();

        assert_eq!(content, "live editor content");
        assert_eq!(calls, 2);
    }

    /// Re-asking against an unchanged witness is a no-op by construction, so the
    /// gate must fail closed instead of spending a second controller round-trip.
    #[test]
    fn route_queue_resolution_does_not_retry_without_a_witness_change() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;

        let err = resolve_route_queue_document_content_awaiting(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                anyhow::bail!(
                    "editor is still attached for {}; missing_replica recovery exhausted and disk read authority is refused",
                    file.display()
                )
            },
            witness_unchanged,
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        assert!(format!("{err:#}").contains("missing_replica recovery exhausted"));
    }

    #[test]
    fn route_queue_resolution_does_not_retry_unrelated_errors() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;
        let mut waits = 0usize;

        let err = resolve_route_queue_document_content_awaiting(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                anyhow::bail!("unrelated resolver failure")
            },
            |_| {
                waits += 1;
                true
            },
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        // An unrelated failure must not even pay the witness wait.
        assert_eq!(waits, 0);
        assert!(format!("{err:#}").contains("unrelated resolver failure"));
    }

    #[test]
    fn route_queue_resolution_stops_after_second_editor_authority_exhaustion() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;

        let err = resolve_route_queue_document_content_awaiting(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                anyhow::bail!("sync_pending recovery exhausted and disk read authority is refused")
            },
            witness_advanced,
        )
        .unwrap_err();

        assert_eq!(calls, ROUTE_QUEUE_MAX_RESOLVE_ATTEMPTS as usize);
        assert!(format!("{err:#}").contains("disk read authority is refused"));
    }

    fn reload_editor_refusal(file: &Path) -> anyhow::Error {
        anyhow::Error::new(
            agent_doc_document_realtime_io::AttachedEditorDiskReadRefused {
                file: file.to_path_buf(),
                reason: "missing_replica".into(),
                remedy: agent_doc_turn::write_ownership::editor_replica_recovery().to_string(),
                recovery:
                    agent_doc_turn::authority_recovery::AttachedEditorRecoveryDecision::ReloadEditor,
            },
        )
    }

    fn stale_install_fanout(blocks_reexec: bool) -> RouteSenderRecycleObservation {
        RouteSenderRecycleObservation {
            outstanding_install_fanout: true,
            sender_on_installed_build: false,
            resolve_blocks_sender_reexec: blocks_reexec,
        }
    }

    fn assert_install_fanout_deferral(err: &anyhow::Error) {
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("install_fanout safe-boundary recycle is already requested"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("Run `agent-doc admin reload-lib`"),
            "{rendered}"
        );
        assert!(
            !rendered.contains("recovery exhausted and disk read authority is refused"),
            "{rendered}"
        );
        assert!(
            rendered.contains("Do not adopt the on-disk copy"),
            "{rendered}"
        );
        assert!(
            agent_doc_document_realtime_io::attached_editor_disk_read_refusal(err).is_none(),
            "the reload-lib refusal must not remain the terminal error: {rendered}"
        );
    }

    #[test]
    fn route_queue_resolution_waits_for_scheduled_install_fanout_then_retries() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;
        let mut sender_waits = 0usize;
        let mut witness_waits = 0usize;

        let content = resolve_route_queue_document_content_observing(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                if calls == 1 {
                    return Err(reload_editor_refusal(&file));
                }
                Ok("live editor content".to_string())
            },
            |_| {
                witness_waits += 1;
                false
            },
            |_| stale_install_fanout(false),
            |_| {
                sender_waits += 1;
                true
            },
        )
        .unwrap();

        assert_eq!(content, "live editor content");
        assert_eq!(calls, 2);
        assert_eq!(sender_waits, 1);
        assert_eq!(
            witness_waits, 0,
            "an outstanding install_fanout recycle is not an unchanged-witness refusal"
        );
    }

    #[test]
    fn route_queue_resolution_defers_when_scheduled_install_fanout_does_not_land() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;

        let err = resolve_route_queue_document_content_observing(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                Err(reload_editor_refusal(&file))
            },
            |_| panic!("witness wait must not decide an outstanding install_fanout recycle"),
            |_| stale_install_fanout(false),
            |_| false,
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        assert_install_fanout_deferral(&err);
    }

    #[test]
    fn route_queue_resolution_does_not_wait_inside_the_stale_sender() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;
        let mut sender_waits = 0usize;

        let err = resolve_route_queue_document_content_observing(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                Err(reload_editor_refusal(&file))
            },
            |_| panic!("witness wait must not decide an outstanding install_fanout recycle"),
            |_| stale_install_fanout(true),
            |_| {
                sender_waits += 1;
                true
            },
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        assert_eq!(sender_waits, 0);
        assert_install_fanout_deferral(&err);
    }

    #[test]
    fn route_queue_resolution_retries_once_when_install_fanout_sender_is_installed() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;
        let mut sender_waits = 0usize;

        let err = resolve_route_queue_document_content_observing(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                Err(reload_editor_refusal(&file))
            },
            |_| false,
            |_| RouteSenderRecycleObservation {
                outstanding_install_fanout: true,
                sender_on_installed_build: true,
                resolve_blocks_sender_reexec: false,
            },
            |_| {
                sender_waits += 1;
                false
            },
        )
        .unwrap_err();

        assert_eq!(
            calls, 2,
            "retry once after the sender is on the installed build"
        );
        assert_eq!(sender_waits, 0);
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("Run `agent-doc admin reload-lib`"),
            "a fresh sender with the replica still missing keeps #84: {rendered}"
        );
    }

    #[test]
    fn route_queue_resolution_keeps_reload_lib_without_an_install_fanout_recycle() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;

        let err = resolve_route_queue_document_content_observing(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                Err(reload_editor_refusal(&file))
            },
            |_| false,
            |_| RouteSenderRecycleObservation {
                outstanding_install_fanout: false,
                sender_on_installed_build: false,
                resolve_blocks_sender_reexec: false,
            },
            |_| panic!("no install_fanout recycle is scheduled"),
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("Run `agent-doc admin reload-lib`"),
            "{rendered}"
        );
        assert!(
            rendered.contains("recovery exhausted and disk read authority is refused"),
            "{rendered}"
        );
    }

    #[test]
    fn route_queue_resolution_does_not_defer_a_restart_refusal_for_install_fanout() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;

        let err = resolve_route_queue_document_content_observing(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                Err(anyhow::Error::new(
                    agent_doc_document_realtime_io::AttachedEditorDiskReadRefused {
                        file: file.clone(),
                        reason: "missing_replica".into(),
                        remedy: "Restart that editor so it re-registers its replica".into(),
                        recovery: agent_doc_turn::authority_recovery::AttachedEditorRecoveryDecision::RestartEditor,
                    },
                ))
            },
            |_| false,
            |_| stale_install_fanout(false),
            |_| panic!("restart is not the install_fanout reload-lib conflict"),
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        let rendered = format!("{err:#}");
        assert!(rendered.contains("Restart that editor"), "{rendered}");
        assert!(
            !rendered.contains("install_fanout safe-boundary recycle is already requested"),
            "{rendered}"
        );
    }

    #[test]
    fn route_queue_resolution_does_not_treat_prose_only_refusal_as_install_fanout() {
        let dir = tempfile::TempDir::new().unwrap();
        let file = dir.path().join("session.md");
        let mut calls = 0usize;

        let err = resolve_route_queue_document_content_observing(
            &file,
            "route_queue_activation",
            || {
                calls += 1;
                anyhow::bail!(
                    "editor is still attached for {}; missing_replica recovery exhausted and disk read authority is refused. {}",
                    file.display(),
                    agent_doc_turn::write_ownership::editor_replica_recovery(),
                )
            },
            |_| false,
            |_| stale_install_fanout(false),
            |_| panic!("an untyped refusal keeps the witness gate"),
        )
        .unwrap_err();

        assert_eq!(calls, 1);
        let rendered = format!("{err:#}");
        assert!(
            rendered.contains("Run `agent-doc admin reload-lib`"),
            "{rendered}"
        );
        assert!(
            rendered.contains("recovery exhausted and disk read authority is refused"),
            "{rendered}"
        );
    }

    #[test]
    fn route_queue_resolution_install_fanout_does_not_request_another_recycle() {
        let source = include_str!("queue_dispatch.rs");
        let start = source
            .find("fn route_sender_recycle_observation")
            .expect("observation");
        let end = source
            .find("fn wait_for_route_sender_installed")
            .expect("wait");
        let body = &source[start..end];
        assert!(!body.contains("request_recycle"), "{body}");
        assert!(!body.contains("reload-lib"), "{body}");
        assert!(!body.contains("restart-supervisor"), "{body}");
    }
}
