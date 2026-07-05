use std::path::Path;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SweepOwner {
    pane: String,
    source: String,
}

enum ActorSweepOwner {
    Active(SweepOwner),
    Inactive,
    Unknown,
}

fn actor_sweep_owner(audit_file: &Path, root: &Path, doc_path: &Path) -> ActorSweepOwner {
    match agent_doc_controller_io::project_controller::authoritative_actor_binding(root, doc_path) {
        Ok(Some(record)) if record.state.as_str() != "closed" => {
            ActorSweepOwner::Active(SweepOwner {
                pane: record.pane_id,
                source: format!("actor:{}", record.state.as_str()),
            })
        }
        Ok(Some(_)) => ActorSweepOwner::Inactive,
        Ok(None) => ActorSweepOwner::Unknown,
        Err(err) => {
            eprintln!(
                "[preflight] sweep: owner warning for {}: {}",
                doc_path.display(),
                err
            );
            agent_doc_ops_log_io::log_op(
                audit_file,
                &format!(
                    "foreign_owned_sweep_owner_warning file={} error={}",
                    doc_path.display(),
                    err.to_string().replace('\n', " ")
                ),
            );
            ActorSweepOwner::Unknown
        }
    }
}

fn registry_sweep_owner(
    root: &Path,
    registry: &tmux_router::Registry,
    doc_path: &Path,
) -> Option<SweepOwner> {
    let key = tmux_router::registry::canonical_registry_key_in(root, &doc_path.to_string_lossy());
    registry.get(&key).and_then(|entry| {
        (!entry.pane.trim().is_empty()).then(|| SweepOwner {
            pane: entry.pane.clone(),
            source: "sessions.json".to_string(),
        })
    })
}

pub fn sweep_owner_for_doc(
    audit_file: &Path,
    root: &Path,
    registry: &tmux_router::Registry,
    doc_path: &Path,
) -> Option<SweepOwner> {
    match actor_sweep_owner(audit_file, root, doc_path) {
        ActorSweepOwner::Active(owner) => Some(owner),
        ActorSweepOwner::Inactive | ActorSweepOwner::Unknown => {
            registry_sweep_owner(root, registry, doc_path)
        }
    }
}

pub fn current_sweep_owner(
    audit_file: &Path,
    root: &Path,
    registry: &tmux_router::Registry,
    current_doc: &Path,
) -> Option<SweepOwner> {
    sweep_owner_for_doc(audit_file, root, registry, current_doc)
}

pub fn log_and_skip_foreign_owned_sweep_if_needed(
    audit_file: &Path,
    doc_path: &Path,
    current_owner: Option<&SweepOwner>,
    sibling_owner: Option<&SweepOwner>,
) -> bool {
    let (Some(current_owner), Some(sibling_owner)) = (current_owner, sibling_owner) else {
        return false;
    };
    if !agent_doc_workflow::preflight_policy::should_skip_foreign_owned_sweep(
        Some(current_owner.pane.as_str()),
        Some(sibling_owner.pane.as_str()),
    ) {
        return false;
    };

    eprintln!(
        "[preflight] sweep: skipping {} (foreign-owned by pane {}; current owner pane {})",
        doc_path.display(),
        sibling_owner.pane,
        current_owner.pane
    );
    agent_doc_ops_log_io::log_op(
        audit_file,
        &format!(
            "foreign_owned_sweep_skip file={} owner_pane={} owner_source={} current_pane={} current_source={}",
            doc_path.display(),
            sibling_owner.pane,
            sibling_owner.source,
            current_owner.pane,
            current_owner.source
        ),
    );
    true
}
