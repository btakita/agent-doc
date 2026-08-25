//! Durable tmux-server lifetime reconciliation.

use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use agent_doc_session_registry::tmux_server::{
    TmuxServerIdentity, TmuxServerIdentityAction, tmux_server_identity_action,
};
use anyhow::{Context, Result};
use tmux_router::{Registry, RegistryLock, Tmux};

const TMUX_SERVER_IDENTITY_KEY: &str = "tmux_server_identity_v1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct TmuxServerReconcileOutcome {
    pub server_replaced: bool,
    pub stale_rows_removed: usize,
}

/// Compare the live tmux server lifetime with the identity stored beside the
/// registry and clear rows from the prior server before they can be routed.
pub fn reconcile_tmux_server_identity_in(
    project_root: &Path,
    tmux: &Tmux,
) -> Result<TmuxServerReconcileOutcome> {
    let current = observe_tmux_server_identity(tmux)?;
    reconcile_observed_identity_in(project_root, current)
}

fn observe_tmux_server_identity(tmux: &Tmux) -> Result<TmuxServerIdentity> {
    let output = tmux
        .raw_cmd(&["display-message", "-p", "#{pid}\t#{start_time}"])
        .context("query tmux server identity")?;
    parse_tmux_server_identity(&output)
}

fn parse_tmux_server_identity(output: &str) -> Result<TmuxServerIdentity> {
    let mut fields = output.trim().split('\t');
    let pid = fields
        .next()
        .context("tmux server identity omitted pid")?
        .parse::<u32>()
        .context("parse tmux server pid")?;
    let start_time = fields
        .next()
        .context("tmux server identity omitted start time")?
        .parse::<u64>()
        .context("parse tmux server start time")?;
    if fields.next().is_some() {
        anyhow::bail!("tmux server identity returned unexpected fields: {output:?}");
    }
    Ok(TmuxServerIdentity { pid, start_time })
}

fn reconcile_observed_identity_in(
    project_root: &Path,
    current: TmuxServerIdentity,
) -> Result<TmuxServerReconcileOutcome> {
    let registry_path = crate::registry_path_in(project_root);
    let _lock = RegistryLock::acquire(&registry_path)?;
    let previous = load_identity(project_root)?;
    let action = tmux_server_identity_action(previous, current);
    let mut outcome = TmuxServerReconcileOutcome::default();

    if action == TmuxServerIdentityAction::Replace {
        let registry = crate::load_in(project_root)?;
        outcome.server_replaced = true;
        outcome.stale_rows_removed = registry.len();
        crate::save_in(project_root, &Registry::new())?;
    }
    store_identity(project_root, current)?;
    Ok(outcome)
}

fn load_identity(project_root: &Path) -> Result<Option<TmuxServerIdentity>> {
    let conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        project_root,
        Duration::from_secs(2),
    )?;
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(
        &conn,
        TMUX_SERVER_IDENTITY_KEY,
    )?
    .map(|payload| parse_tmux_server_identity(&payload))
    .transpose()
}

fn store_identity(project_root: &Path, identity: TmuxServerIdentity) -> Result<()> {
    let conn = agent_doc_sqlite::state_store::open_state_db_with_timeout(
        project_root,
        Duration::from_secs(2),
    )?;
    let updated_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX);
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        TMUX_SERVER_IDENTITY_KEY,
        &format!("{}\t{}", identity.pid, identity.start_time),
        updated_at_ms,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use tmux_router::RegistryEntry;

    fn entry(session_id: &str, pane: &str) -> RegistryEntry {
        RegistryEntry {
            pane: pane.to_string(),
            pid: 123,
            cwd: "/tmp".to_string(),
            started: "now".to_string(),
            session_id: session_id.to_string(),
            file: format!("{session_id}.md"),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        }
    }

    #[test]
    fn parser_accepts_tmux_display_message_identity() {
        assert_eq!(
            parse_tmux_server_identity("6535\t1787525374\n").unwrap(),
            TmuxServerIdentity {
                pid: 6535,
                start_time: 1_787_525_374,
            }
        );
    }

    #[test]
    fn replacement_clears_every_prior_server_row() {
        let project = tempfile::tempdir().unwrap();
        let server_a = TmuxServerIdentity {
            pid: 100,
            start_time: 1_000,
        };
        let server_b = TmuxServerIdentity {
            pid: 200,
            start_time: 2_000,
        };
        let mut registry = Registry::new();
        registry.insert("a.md".to_string(), entry("a", "%0"));
        registry.insert("b.md".to_string(), entry("b", "%1"));
        crate::save_in(project.path(), &registry).unwrap();

        let initialized = reconcile_observed_identity_in(project.path(), server_a).unwrap();
        assert_eq!(initialized.stale_rows_removed, 0);
        assert_eq!(crate::load_in(project.path()).unwrap().len(), 2);

        let replaced = reconcile_observed_identity_in(project.path(), server_b).unwrap();
        assert!(replaced.server_replaced);
        assert_eq!(replaced.stale_rows_removed, 2);
        assert!(crate::load_in(project.path()).unwrap().is_empty());
        assert_eq!(load_identity(project.path()).unwrap(), Some(server_b));
    }
}
