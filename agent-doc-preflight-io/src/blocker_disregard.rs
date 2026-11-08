//! Preflight consumption of in-document blocker-disregard gestures
//! (`#blockerdisregard`).
//!
//! A `disregard` child line, a `[disregard]` token, or a `disregard blockers`
//! line is the operator's word. Preflight applies it before the diff, the same
//! way it answers `status?`, so the gesture never becomes a model prompt.

use std::path::Path;

use anyhow::Result;

/// Apply disregard gestures already in `file`. Best-effort at the call site:
/// a failure changes nothing and never blocks admission.
pub fn run_blocker_disregard_maintenance(file: &Path) -> Result<bool> {
    let source = "preflight_blocker_disregard";
    let content = match super::current_text_via_preflight_authority_retrying(file, source)? {
        Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => text,
        Some(agent_doc_crdt_relay_io::CurrentText::Detached) | None => {
            match std::fs::read_to_string(file) {
                Ok(text) => text,
                Err(_) => return Ok(false),
            }
        }
        Some(_) => return Ok(false),
    };
    let plan = agent_doc_queue::blocker_disregard::plan(
        &content,
        agent_doc_queue::blocker_disregard::DisregardTarget::Gestures,
    )?;
    if !plan.changed {
        return Ok(false);
    }
    let project_root = agent_doc_project_root_io::project_root_containing(file);
    super::persist_queue_maintenance_doc(
        file,
        &plan.content,
        &content,
        project_root.as_deref(),
        source,
    )?;
    for line in &plan.disregarded {
        eprintln!("[preflight] blocker disregard: {line} (#blockerdisregard)");
    }
    for (label, reason) in &plan.kept {
        eprintln!("[preflight] blocker disregard: kept {label}: {reason} (#blockerdisregard)");
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "blocker_disregard_applied file={} source=preflight",
            file.display()
        ),
    );
    Ok(true)
}
