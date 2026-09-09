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
    let Some(current) = observe_tmux_server_identity(tmux)? else {
        // Absence is not a replacement receipt. Preserve the prior identity so
        // the first observation after bootstrap can still invalidate old rows.
        return Ok(TmuxServerReconcileOutcome::default());
    };
    reconcile_observed_identity_in(project_root, current)
}

fn observe_tmux_server_identity(tmux: &Tmux) -> Result<Option<TmuxServerIdentity>> {
    // Actorless bootstrap observation: no managed process graph exists yet.
    // Keep process status instead of depending on raw_cmd's version-specific
    // treatment of nonzero exits and empty stdout.
    let output = tmux
        .cmd()
        .args(["display-message", "-p", "#{pid}\t#{start_time}"])
        .output()
        .context("query tmux server identity")?;
    let stdout = String::from_utf8_lossy(&output.stdout);
    if stdout.trim().is_empty() && (output.status.success() || !tmux.running()) {
        return Ok(None);
    }
    if !output.status.success() {
        anyhow::bail!(
            "query tmux server identity failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    parse_tmux_server_identity(&stdout).map(Some)
}

fn parse_tmux_server_identity(output: &str) -> Result<TmuxServerIdentity> {
    let mut fields = output.trim().split('\t');
    let pid = fields
        .next()
        .filter(|field| !field.is_empty())
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
    fn absent_server_preserves_prior_identity_and_registry() {
        let project = tempfile::tempdir().unwrap();
        let prior = TmuxServerIdentity {
            pid: 100,
            start_time: 1_000,
        };
        store_identity(project.path(), prior).unwrap();
        let mut registry = Registry::new();
        registry.insert("old.md".into(), entry("old", "%0"));
        crate::save_in(project.path(), &registry).unwrap();
        let tmux = Tmux::default_server_with_binary("tmux").with_server_socket(Some(format!(
            "agent-doc-absent-{}",
            project.path().file_name().unwrap().to_string_lossy()
        )));

        assert_eq!(
            reconcile_tmux_server_identity_in(project.path(), &tmux).unwrap(),
            TmuxServerReconcileOutcome::default()
        );
        assert_eq!(load_identity(project.path()).unwrap(), Some(prior));
        assert_eq!(crate::load_in(project.path()).unwrap().len(), 1);

        let replacement = TmuxServerIdentity {
            pid: 200,
            start_time: 2_000,
        };
        let outcome = reconcile_observed_identity_in(project.path(), replacement).unwrap();
        assert!(outcome.server_replaced);
        assert_eq!(outcome.stale_rows_removed, 1);
    }

    #[test]
    fn missing_executable_is_not_server_absence() {
        let project = tempfile::tempdir().unwrap();
        let tmux = Tmux::default_server_with_binary(project.path().join("missing-tmux"));
        assert!(
            observe_tmux_server_identity(&tmux)
                .unwrap_err()
                .to_string()
                .contains("query tmux server identity")
        );
    }

    #[test]
    fn malformed_identity_remains_an_error() {
        for input in ["", "garbage", "12\tbad", "12\t34\textra"] {
            assert!(parse_tmux_server_identity(input).is_err(), "{input:?}");
        }
        assert!(
            parse_tmux_server_identity("")
                .unwrap_err()
                .to_string()
                .contains("omitted pid")
        );
    }

    #[cfg(unix)]
    #[test]
    fn observation_distinguishes_empty_output_from_live_query_failure() {
        use std::os::unix::fs::PermissionsExt;
        let project = tempfile::tempdir().unwrap();
        let binary = project.path().join("tmux");
        let tmux = Tmux::default_server_with_binary(&binary);
        for (script, absent) in [
            ("#!/bin/sh\nexit 0\n", true),
            ("#!/bin/sh\nexit 1\n", true),
            (
                "#!/bin/sh\ncase \"$1\" in has-session) exit 0;; *) echo denied >&2; exit 1;; esac\n",
                false,
            ),
            ("#!/bin/sh\nprintf 'not-an-identity\\n'\n", false),
        ] {
            std::fs::write(&binary, script).unwrap();
            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
            let observation = observe_tmux_server_identity(&tmux);
            if absent {
                assert_eq!(observation.unwrap(), None);
            } else {
                assert!(observation.is_err());
            }
        }
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
