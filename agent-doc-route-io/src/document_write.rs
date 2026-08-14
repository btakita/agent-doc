use anyhow::Result;
use std::path::Path;
use std::time::{Duration, Instant};

#[cfg(test)]
const RETAINED_ROUTE_WRITE_TIMEOUT: Duration = Duration::from_millis(250);
#[cfg(not(test))]
const RETAINED_ROUTE_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
const RETAINED_ROUTE_WRITE_AWAIT_SLICE: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RetainedRouteProjection {
    Pending,
    ConvergedTarget,
    ConvergedDrift,
}

fn retained_route_projection(
    current: Option<&agent_doc_crdt_relay_io::CurrentText>,
    target: &str,
) -> RetainedRouteProjection {
    match current {
        Some(agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            delivery_converged: true,
            ..
        }) if text == target => RetainedRouteProjection::ConvergedTarget,
        Some(agent_doc_crdt_relay_io::CurrentText::Current {
            delivery_converged: true,
            ..
        }) => RetainedRouteProjection::ConvergedDrift,
        _ => RetainedRouteProjection::Pending,
    }
}

fn retained_route_write_error(error: &anyhow::Error) -> bool {
    error
        .downcast_ref::<agent_doc_document_realtime_io::AwaitEditorReplicaNoDiskWrite>()
        .is_some()
}

/// Finish a route-owned mutation that the CRDT plane accepted lazily.
///
/// Route has no response capture yet: the queue activation is what authorizes
/// the subsequent prompt dispatch. Keep the route call alive until the exact
/// target is editor-visible, so the caller can checkpoint it and continue into
/// preflight instead of returning an owner-pane-only `commit` remedy.
fn await_retained_route_write(file: &Path, target: &str, source: &str) -> Result<()> {
    let deadline = Instant::now() + RETAINED_ROUTE_WRITE_TIMEOUT;
    loop {
        let current =
            agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
                file,
                "route_retained_write_projection",
            )
            .unwrap_or(None);
        match retained_route_projection(current.as_ref(), target) {
            RetainedRouteProjection::ConvergedTarget => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "route_retained_write_converged file={} source={} target_hash={} recovery=continue_route_dispatch",
                        file.display(),
                        source,
                        agent_doc_hash::content_hash(target),
                    ),
                );
                return Ok(());
            }
            RetainedRouteProjection::ConvergedDrift => {
                anyhow::bail!(
                    "{source}: retained route write for {} converged to a newer editor projection; recovery=retry_crdt_merge",
                    file.display(),
                );
            }
            RetainedRouteProjection::Pending => {}
        }
        if Instant::now() >= deadline {
            anyhow::bail!(
                "{source}: retained route mutation for {} did not become editor-visible within {}ms; recovery=rerun_agent_doc_route (do not use owner-pane commit)",
                file.display(),
                RETAINED_ROUTE_WRITE_TIMEOUT.as_millis(),
            );
        }
        match agent_doc_controller_io::project_controller::await_delivery_convergence_for_file(
            file,
            RETAINED_ROUTE_WRITE_AWAIT_SLICE,
        ) {
            Ok(Some(_)) => {}
            Ok(None) | Err(_) => std::thread::sleep(Duration::from_millis(25)),
        }
    }
}

pub fn route_write_document(
    file: &Path,
    next_content: &str,
    previous_content: &str,
    reason: &str,
) -> Result<()> {
    if crate::invocation::force_disk_route_writes() {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, next_content)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{}_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                reason,
                file.display(),
                next_content.len(),
                agent_doc_hash::content_hash(next_content)
            ),
        );
        Ok(())
    } else {
        match agent_doc_write_converge_io::converge_document_or_disk(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            next_content,
            previous_content,
            reason,
        ) {
            Ok(()) => Ok(()),
            Err(error) if retained_route_write_error(&error) => {
                await_retained_route_write(file, next_content, reason)
            }
            Err(error) => Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn current(text: &str, delivery_converged: bool) -> agent_doc_crdt_relay_io::CurrentText {
        agent_doc_crdt_relay_io::CurrentText::Current {
            text: text.to_string(),
            live_editors: 1,
            delivery_converged,
            delivery_version: 1,
            semantics: None,
        }
    }

    #[test]
    fn retained_route_write_waits_for_exact_visible_target() {
        let target = "queue: go\n";
        assert_eq!(
            retained_route_projection(Some(&current(target, false)), target),
            RetainedRouteProjection::Pending,
        );
        assert_eq!(
            retained_route_projection(Some(&current(target, true)), target),
            RetainedRouteProjection::ConvergedTarget,
        );
        assert_eq!(
            retained_route_projection(Some(&current("queue: stop\n", true)), target),
            RetainedRouteProjection::ConvergedDrift,
        );
        assert_eq!(
            retained_route_projection(
                Some(&agent_doc_crdt_relay_io::CurrentText::EditorSyncPending),
                target,
            ),
            RetainedRouteProjection::Pending,
        );
    }
}
