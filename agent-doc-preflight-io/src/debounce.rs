use anyhow::Result;
use std::path::Path;

/// Poll cadence inherited from the document's configured debounce budget.
/// Debounce is no longer an editor-authority signal; Lazily current state is.
pub fn authority_settle_ms(file: &Path) -> u64 {
    std::fs::read_to_string(file)
        .ok()
        .and_then(|content| {
            agent_doc_frontmatter::parse(&content)
                .ok()
                .and_then(|(fm, _)| fm.debounce_ms)
        })
        .unwrap_or(2000)
}

fn lazily_current_ready(file: &Path, source: &str) -> Result<(bool, &'static str)> {
    use agent_doc_crdt_relay_io::CurrentText;

    Ok(match agent_doc_controller_io::project_controller::current_text_via_controller_model_for_doc(
        file, source,
    )? {
        None | Some(CurrentText::Detached) => (true, "detached"),
        Some(CurrentText::Current {
            delivery_converged: true,
            ..
        }) => (true, "lazily_current"),
        Some(CurrentText::Current { .. }) => (false, "delivery_pending"),
        Some(CurrentText::EditorAttachedMissingReplica) => (false, "missing_replica"),
        Some(CurrentText::EditorSyncPending) => (false, "current_pending"),
    })
}

/// Serialize a visible mutation behind Lazily's current-authority transition.
///
/// This deliberately does not infer operator activity from a filesystem typing
/// marker or disk mtime. The coherent current document is the authority, and the
/// eventual mutation remains guarded by its expected-current CAS.
pub fn wait_for_lazily_current_before_mutation(file: &Path) -> Result<()> {
    let settle_ms = authority_settle_ms(file);
    let max_wait = agent_doc_debounce::authority_settle_max_wait(settle_ms);
    let poll = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();
    loop {
        let (last_state, last_error) =
            match lazily_current_ready(file, "preflight_visible_mutation") {
                Ok((true, _)) => return Ok(()),
                Ok((false, state)) => (state, None),
                Err(error) => ("authority_unavailable", Some(error.to_string())),
            };
        if start.elapsed() >= max_wait {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "preflight_visible_mutation_deferred_lazily_current file={} state={} timeout_ms={} error={}",
                    file.display(),
                    last_state,
                    max_wait.as_millis(),
                    last_error.as_deref().unwrap_or("none")
                ),
            );
            anyhow::bail!(
                "preflight deferred for {}: Lazily current authority remained {} for {}ms; retry after the current transition settles{}",
                file.display(),
                last_state,
                max_wait.as_millis(),
                last_error
                    .as_deref()
                    .map(|error| format!(" ({error})"))
                    .unwrap_or_default()
            );
        }
        std::thread::sleep(poll);
    }
}

/// Observe a coherent Lazily current cut before preflight reads the document.
/// Mutation sites use [`wait_for_lazily_current_before_mutation`] and fail closed;
/// this read-only observation remains bounded and lets later CAS checks decide.
pub fn wait_for_lazily_current_observation(file: &Path) {
    let settle_ms = authority_settle_ms(file);
    let max_wait = agent_doc_debounce::authority_settle_max_wait(settle_ms);
    let poll = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    loop {
        match lazily_current_ready(file, "preflight_observation") {
            Ok((true, state)) => {
                tracing::debug!(
                    waited_ms = start.elapsed().as_millis() as u64,
                    authority_state = state,
                    file = %file.display(),
                    "preflight Lazily current observed"
                );
                return;
            }
            Ok((false, state)) if start.elapsed() < max_wait => {
                tracing::trace!(authority_state = state, "preflight Lazily current pending");
            }
            Err(error) if start.elapsed() < max_wait => {
                tracing::trace!(%error, "preflight Lazily current unavailable");
            }
            outcome => {
                tracing::warn!(
                    waited_ms = start.elapsed().as_millis() as u64,
                    outcome = ?outcome,
                    "preflight Lazily current observation timeout; later expected-current CAS remains authoritative"
                );
                return;
            }
        }
        std::thread::sleep(poll);
    }
}
