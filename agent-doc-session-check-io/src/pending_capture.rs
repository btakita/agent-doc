use std::collections::HashSet;
use std::path::Path;

pub fn unresolved_backlog_capture_targets(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
) -> Vec<String> {
    let current = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());

    state
        .required_backlog_targets
        .iter()
        .filter(|target| {
            let target_path = Path::new(&target.path);
            let normalized_target =
                std::fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());
            if normalized_target == current {
                return !state.had_pending_mutations;
            }

            let Ok(Some(content)) = std::fs::read_to_string(&normalized_target).map(Some) else {
                return true;
            };
            let Ok(components) = agent_doc_element::element::parse(&content) else {
                return true;
            };
            let component = target
                .component
                .as_deref()
                .and_then(|name| components.iter().find(|component| component.name == name))
                .or_else(|| {
                    components.iter().find(|component| {
                        agent_doc_element::element::is_backlog_component(&component.name)
                    })
                })
                .or_else(|| {
                    components.iter().find(|component| {
                        agent_doc_element::element::is_tracked_work_component(&component.name)
                    })
                });
            let current_hash = component
                .map(|component| agent_doc_hash::content_hash(component.content(&content)));
            match (&target.baseline_hash, current_hash) {
                (Some(expected), Some(current)) => current == *expected,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => true,
            }
        })
        .map(|target| target.path.clone())
        .collect()
}

pub fn promised_backlog_item_ids_from_response(
    response_text: &str,
    state: &agent_doc_cycle_state_io::CycleState,
) -> Vec<String> {
    agent_doc_workflow::pending_capture::promised_backlog_item_ids_from_response(
        response_text,
        state
            .required_backlog_targets
            .iter()
            .flat_map(|target| target.baseline_item_ids.iter()),
    )
}

pub fn promised_backlog_item_inventory_shortfall(
    state: &agent_doc_cycle_state_io::CycleState,
    response_text: &str,
) -> Option<(usize, usize)> {
    agent_doc_workflow::pending_capture::promised_backlog_item_inventory_shortfall(
        response_text,
        state
            .required_backlog_targets
            .iter()
            .flat_map(|target| target.baseline_item_ids.iter()),
        state.required_backlog_targets.len(),
        state.required_explicit_backlog_item_count,
    )
    .map(|shortfall| shortfall.as_tuple())
}

pub fn promised_plan_reference_paths(file: &Path, response_text: &str) -> Vec<String> {
    let mut promised = Vec::new();
    for trimmed in
        agent_doc_workflow::pending_capture::promised_plan_reference_candidate_lines(response_text)
    {
        let Some(path) = agent_doc_fs::referenced_markdown_path(file, &trimmed) else {
            continue;
        };
        if !path.exists() {
            continue;
        }
        let file_name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if !file_name.contains("plan") {
            continue;
        }
        let normalized = std::fs::canonicalize(&path)
            .unwrap_or(path)
            .display()
            .to_string();
        if !promised.iter().any(|existing| existing == &normalized) {
            promised.push(normalized);
        }
    }
    promised
}

pub fn promised_plan_reference_shortfall(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
    response_text: &str,
) -> Option<(usize, usize)> {
    let promised_count = promised_plan_reference_paths(file, response_text).len();
    agent_doc_workflow::pending_capture::promised_plan_reference_shortfall(
        state.required_plan_reference_count,
        promised_count,
    )
    .map(|shortfall| shortfall.as_tuple())
}

pub fn unresolved_promised_backlog_item_ids(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
    response_text: &str,
) -> Vec<String> {
    if state.required_backlog_targets.is_empty() {
        return Vec::new();
    }

    let promised_ids = promised_backlog_item_ids_from_response(response_text, state);
    if promised_ids.is_empty() {
        return Vec::new();
    }

    let current = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let mut current_target_ids = HashSet::new();
    for target in &state.required_backlog_targets {
        let target_path = Path::new(&target.path);
        let normalized_target =
            std::fs::canonicalize(target_path).unwrap_or_else(|_| target_path.to_path_buf());
        let target_file = if normalized_target == current {
            file
        } else {
            normalized_target.as_path()
        };
        let content = match crate::resolve_current_document_content(
            target_file,
            "pending_capture_required_backlog_target",
        ) {
            Ok(content) => content,
            Err(_) => continue,
        };
        let Ok(ids) = agent_doc_element_backlog::backlog::tracked_work_ids_for_target(
            &content,
            target.component.as_deref(),
        ) else {
            continue;
        };
        current_target_ids.extend(ids);
    }

    agent_doc_workflow::pending_capture::missing_promised_backlog_item_ids(
        promised_ids.iter().map(String::as_str),
        current_target_ids.iter().map(String::as_str),
    )
    .into_iter()
    .map(|id| format!("#{}", id))
    .collect()
}

/// `#capturegatedispatched`: whether the cycle's generic backlog-capture
/// requirement ([`agent_doc_cycle_state_io::CycleState::requires_untargeted_backlog_capture`])
/// still binds the closeout.
///
/// Preflight fixes the requirement from the heads that armed it. When the
/// coordinator then claims those heads and starts workers for them mid-cycle,
/// their capture belongs to the workers' cycles, so the requirement is released
/// once every head preflight recorded behind it
/// ([`agent_doc_cycle_state_io::CycleState::backlog_capture_heads`]) is
/// worker-owned in the current document. A requirement with no recorded heads
/// rests on operator text that is not a queue head and is never released this
/// way. Explicit backlog targets and the uncaptured-recommendation heuristic
/// are checked separately and are unaffected.
pub fn untargeted_backlog_capture_unmet(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
) -> bool {
    state.requires_untargeted_backlog_capture() && !backlog_capture_heads_worker_owned(file, state)
}

fn backlog_capture_heads_worker_owned(
    file: &Path,
    state: &agent_doc_cycle_state_io::CycleState,
) -> bool {
    if state.backlog_capture_heads.is_empty() {
        return false;
    }
    let Ok(content) = std::fs::read_to_string(file) else {
        return false;
    };
    let owned = agent_doc_queue_io::queue_claim::worker_owned_items_for_content(file, &content);
    let unowned = state
        .backlog_capture_heads
        .iter()
        .filter(|head| !owned.claims(head))
        .count();
    if unowned == 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "closeout_backlog_capture_released cycle={} heads={} reason=heads_worker_owned (#capturegatedispatched)",
                state.cycle_id,
                state.backlog_capture_heads.len()
            ),
        );
    }
    unowned == 0
}
