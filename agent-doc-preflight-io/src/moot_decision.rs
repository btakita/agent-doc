//! Preflight and strict-closeout pass that removes moot reported-closeout
//! operator decisions (`#mootdecision`).
//!
//! Best-effort: an unreadable claim or lineage ledger prunes nothing and does
//! not fail the caller. A document with no `Reported` claim loses every
//! reported-closeout decision that no longer needs an answer.

use std::path::Path;

use anyhow::Result;

/// Preflight pass. Reads through the same authority as the other maintenance
/// passes and writes only when a decision was removed.
pub fn run_moot_decision_maintenance(file: &Path) -> Result<()> {
    run_moot_decision_maintenance_with_options(file, false, None)
}

/// Force-disk counterpart for an explicitly force-disk strict closeout.
pub fn run_moot_decision_maintenance_force_disk(
    file: &Path,
    write_effects: &dyn super::PreflightMaintenanceWriteEffects,
) -> Result<()> {
    run_moot_decision_maintenance_with_options(file, true, Some(write_effects))
}

fn run_moot_decision_maintenance_with_options(
    file: &Path,
    force_disk: bool,
    write_effects: Option<&dyn super::PreflightMaintenanceWriteEffects>,
) -> Result<()> {
    let source = "preflight_moot_decision";
    let Some(content) = read_authoritative(file, force_disk)? else {
        return Ok(());
    };
    let live =
        match agent_doc_queue_io::queue_claim::live_reported_closeout_operator_request_ids(file) {
            Ok(live) => live,
            Err(err) => {
                eprintln!(
                    "[preflight] moot decision: not applied (claim state unreadable): {err:#}"
                );
                return Ok(());
            }
        };
    let Some(target) =
        agent_doc_queue::moot_decision::prune_moot_reported_closeout_decisions(&content, &live)
    else {
        return Ok(());
    };
    let persisted = if force_disk {
        let effects = write_effects.ok_or_else(|| {
            anyhow::anyhow!("force-disk moot-decision maintenance requires write effects")
        })?;
        super::persist_pending_maintenance_doc(file, &content, &target, source, true, effects)?;
        target
    } else {
        let root = agent_doc_project_root_io::project_root_containing(file);
        super::persist_queue_maintenance_doc(file, &target, &content, root.as_deref(), source)?
    };
    if let Ok(Some(snapshot)) = agent_doc_snapshot_io::load_document_baseline(file)
        && let Some(new_snapshot) = carry_prune_into_snapshot(&snapshot, &content, &persisted)
        && let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
            file,
            &new_snapshot,
            agent_doc_ops_log_io::log_op,
        )
    {
        eprintln!("[preflight] moot decision: snapshot sync warning: {err}");
    }
    eprintln!("[preflight] moot decision: pruned moot reported-closeout decisions (#mootdecision)");
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "moot_decision_pruned file={} (#mootdecision)",
            file.display()
        ),
    );
    Ok(())
}

fn read_authoritative(file: &Path, force_disk: bool) -> Result<Option<String>> {
    if force_disk {
        return Ok(std::fs::read_to_string(file).ok());
    }
    Ok(
        match super::current_text_via_preflight_authority_retrying(file, "preflight_moot_decision")?
        {
            Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => Some(text),
            Some(agent_doc_crdt_relay_io::CurrentText::Detached) | None => {
                std::fs::read_to_string(file).ok()
            }
            Some(_) => None,
        },
    )
}

/// Keep the recovery baseline aligned with a prune when the pruned region
/// still matched the baseline. A snapshot that already differs keeps that
/// difference, except for a todo, queue, or exchange body that was unchanged
/// before the prune.
fn carry_prune_into_snapshot(snapshot: &str, before: &str, after: &str) -> Option<String> {
    if snapshot == before {
        return (snapshot != after).then(|| after.to_string());
    }
    let mut current = snapshot.to_string();
    let mut changed = false;
    if let Some(carried) =
        agent_doc_element::todo::carry_todo_body_into_snapshot(&current, before, after)
    {
        current = carried;
        changed = true;
    }
    for name in ["queue", "exchange"] {
        if let Some(carried) = carry_component_body(&current, before, after, name) {
            current = carried;
            changed = true;
        }
    }
    changed.then_some(current)
}

fn carry_component_body(snapshot: &str, before: &str, after: &str, name: &str) -> Option<String> {
    let before_components = agent_doc_element::element::parse(before).ok()?;
    let after_components = agent_doc_element::element::parse(after).ok()?;
    let before_bodies = component_bodies(&before_components, before, name);
    let after_bodies = component_bodies(&after_components, after, name);
    if before_bodies.len() != after_bodies.len() {
        return None;
    }
    let mut current = snapshot.to_string();
    let mut changed = false;
    for index in 0..before_bodies.len() {
        if before_bodies[index] == after_bodies[index] {
            continue;
        }
        let snap_components = agent_doc_element::element::parse(&current).ok()?;
        let snap = snap_components
            .iter()
            .filter(|component| component.name == name)
            .nth(index)?;
        if snap.content(&current) != before_bodies[index] {
            continue;
        }
        current = snap.replace_content(&current, &after_bodies[index]);
        changed = true;
    }
    changed.then_some(current)
}

fn component_bodies(
    components: &[agent_doc_element::element::Component],
    document: &str,
    name: &str,
) -> Vec<String> {
    components
        .iter()
        .filter(|component| component.name == name)
        .map(|component| component.content(document).to_string())
        .collect()
}
