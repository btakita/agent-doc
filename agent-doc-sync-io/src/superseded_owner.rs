//! Retire an agent-doc supervisor pane that no longer owns its document
//! (GH #243).
//!
//! One document must have one live supervisor pane. Two paths left a second
//! one running:
//!
//! - `agent-doc start --force` bypassed the existing owner's refusal and left
//!   that owner running, so its supervisor kept acting on the old pane and its
//!   harness kept answering there.
//! - A route then kept dispatching to the authoritative pane while the stale
//!   duplicate stayed alive; when the duplicate was the pane the operator could
//!   see, every write from it was refused ("pane execution authority rejected
//!   before mutation") and nothing told the operator why.
//!
//! The pane that wins is the controller's authoritative actor pane: it holds
//! the newest actor generation, the queue claims, and the route target. A
//! duplicate is retired only when its process tree *proves* it is agent-doc's
//! supervisor for this document (`#bare-foreign-session-guard`: ownership is
//! proven, never assumed) and the pane is idle, input-free, and not displayed
//! by a client. Protected route duplicates are reported and receive a targeted
//! request that their own idle watcher consumes at the first safe boundary;
//! protected force-start owners remain live and require an operator retry. A
//! bare harness, or a pane without ownership proof, is never touched.

use agent_doc_supervisor::session_owner::{
    SupersededOwnerAction, SupersededOwnerFacts, superseded_owner_action,
};
use anyhow::Result;
use std::path::Path;
use std::time::Duration;
use tmux_router::Tmux;

/// How long a retired supervisor gets to exit on SIGTERM before SIGKILL.
pub const SUPERSEDED_OWNER_EXIT_GRACE: Duration = Duration::from_secs(5);

/// Observe what `pane`'s process tree proves about owning `file`.
fn superseded_owner_facts(tmux: &Tmux, file: &Path, pane: &str) -> (bool, Option<String>) {
    let canonical = std::fs::canonicalize(file).unwrap_or_else(|_| file.to_path_buf());
    let Some(pane_pid) = agent_doc_tmux_io::pane_pid(tmux, pane).map(|pid| pid.to_string()) else {
        return (false, None);
    };
    let _observations = agent_doc_process_owner_io::begin_process_observation_scope();
    let owner_pid = agent_doc_process_owner_io::process_tree_agent_doc_owner_pid_for_file(
        &pane_pid,
        &canonical.to_string_lossy(),
    )
    .filter(|pid| {
        // Only a supervisor (`agent-doc start … <doc>`) proves ownership. A
        // transient `agent-doc write|compact|session … <doc>` in some other
        // pane also matches the owner predicate and must never be retired.
        agent_doc_process_owner_io::process_command(pid).is_some_and(|cmdline| {
            let args = cmdline
                .split_whitespace()
                .map(str::to_string)
                .collect::<Vec<_>>();
            agent_doc_controller::command_line::agent_doc_start_arg_index(&args).is_some()
        })
    });
    (
        agent_doc_process_owner_io::process_tree_runs_unmanaged_harness_session(&pane_pid),
        owner_pid,
    )
}

fn active_turn_for_pane(file: &Path, pane: &str) -> bool {
    agent_doc_project_root_io::project_root_or_file_parent(file)
        .ok()
        .is_some_and(|root| agent_doc_turn_status_io::turn_active_for_pane(&root, pane))
}

fn protected_input_reason(tmux: &Tmux, file: &Path, pane: &str) -> Option<String> {
    crate::sync::pane_repair::protected_registered_pane_state(tmux, file, pane)
        .map(|state| state.reason)
}

/// Decide, without acting, what retiring `pane` as `file`'s superseded owner
/// would do.
pub fn superseded_owner_decision(tmux: &Tmux, file: &Path, pane: &str) -> SupersededOwnerAction {
    let (runs_unmanaged_harness, owner_pid) = superseded_owner_facts(tmux, file, pane);
    superseded_owner_action(SupersededOwnerFacts {
        pane,
        runs_unmanaged_harness,
        agent_doc_owner_pid: owner_pid.as_deref(),
        active_turn: active_turn_for_pane(file, pane),
        protected_input_reason: protected_input_reason(tmux, file, pane).as_deref(),
        // `unwatched_pane` returns `Some` only when tmux positively proves the
        // pane is absent from every attached client's screen. Its `None` case
        // deliberately fails closed for both visible and unreadable placement.
        client_visible_or_unobservable: agent_doc_tmux_io::unwatched_pane(tmux, pane).is_none(),
    })
}

/// Retire `pane` as a superseded owner of `file` when its process tree proves
/// it is agent-doc's supervisor and live evidence proves destructive retirement
/// is safe. Otherwise return [`SupersededOwnerAction::Defer`] and report why.
///
/// SIGTERMs the supervisor (its handler records an intentional exit, so the
/// controller watchdog does not respawn it), escalates to SIGKILL after
/// `grace`, stops any descendant (the harness) that outlived it, and closes the
/// pane. Logs `<event>_retired` or `<event>_left reason=…`. Fails, naming the
/// pane, when the supervisor survives.
pub fn retire_superseded_owner_pane(
    tmux: &Tmux,
    file: &Path,
    pane: &str,
    authoritative_owner: &str,
    grace: Duration,
    event: &str,
) -> Result<SupersededOwnerAction> {
    let action = superseded_owner_decision(tmux, file, pane);
    let window = tmux
        .pane_window(pane)
        .unwrap_or_else(|_| "unknown".to_string());
    if let SupersededOwnerAction::Defer {
        supervisor_pid,
        protection,
        ..
    } = &action
    {
        let reason = protection.reason();
        let retirement_scheduled = event == "route_duplicate_owner"
            && supervisor_pid.parse::<u32>().ok().is_some_and(|pid| {
                match agent_doc_supervisor_io::duplicate_retirement::request_duplicate_retirement(
                    file,
                    pane,
                    pid,
                    authoritative_owner,
                    &window,
                    event,
                ) {
                    Ok(()) => true,
                    Err(error) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "{event}_defer_request_failed file={} pane={} window={} owner={} error={}",
                                file.display(),
                                pane,
                                window,
                                authoritative_owner,
                                format!("{error:#}").replace('\n', "\\n"),
                            ),
                        );
                        false
                    }
                }
            });
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{event}_deferred file={} pane={} window={} owner={} reason={} idle_retirement_scheduled={retirement_scheduled}",
                file.display(),
                pane,
                window,
                authoritative_owner,
                reason.replace(' ', "_")
            ),
        );
        eprintln!(
            "[agent-doc] deferred retirement of duplicate supervisor pane {pane} (window {window}) for {}; authoritative owner pane {authoritative_owner}; reason={reason}. {}",
            file.display(),
            if retirement_scheduled {
                "Its supervisor will close the pane at the first idle, input-free, unwatched boundary."
            } else {
                "The duplicate remains alive; retry after it reaches an idle, input-free, unwatched boundary."
            },
        );
        return Ok(action);
    }
    let SupersededOwnerAction::Retire {
        pane,
        supervisor_pid,
    } = &action
    else {
        let reason = match &action {
            SupersededOwnerAction::LeaveForeignHarness { .. } => "foreign_harness",
            SupersededOwnerAction::Defer { .. } => unreachable!("handled above"),
            _ => "unproven_owner",
        };
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "{event}_left file={} pane={} reason={reason}",
                file.display(),
                pane,
            ),
        );
        return Ok(action);
    };
    let own_pid = std::process::id().to_string();
    if supervisor_pid == &own_pid {
        anyhow::bail!("refusing to retire pane {pane}: its supervisor is this process ({own_pid})");
    }
    let descendants = process_descendants(supervisor_pid)
        .into_iter()
        .filter(|pid| pid != &own_pid)
        .collect::<Vec<_>>();
    if !terminate_pid(supervisor_pid, grace) {
        anyhow::bail!(
            "could not retire the superseded agent-doc supervisor for {}: pid {} in pane {} survived SIGTERM and SIGKILL; stop it (`tmux kill-pane -t {}`) and retry",
            file.display(),
            supervisor_pid,
            pane,
            pane,
        );
    }
    // The harness normally exits with its PTY; stop any straggler it left.
    let mut stragglers = Vec::new();
    for pid in &descendants {
        if pid_alive(pid) {
            stragglers.push(pid.clone());
            terminate_pid(pid, Duration::from_millis(1500));
        }
    }
    let pane_closed = agent_doc_tmux_io::kill_pane(tmux, pane).is_ok();
    if let Ok(pid) = supervisor_pid.parse::<u32>() {
        agent_doc_supervisor_io::duplicate_retirement::clear_duplicate_retirement_request(
            file, pid,
        );
    }
    tmux_router::invalidate_pane_snapshot();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "{event}_retired file={} pane={} supervisor_pid={} descendants=[{}] stragglers=[{}] pane_closed={}",
            file.display(),
            pane,
            supervisor_pid,
            descendants.join(","),
            stragglers.join(","),
            pane_closed,
        ),
    );
    eprintln!(
        "[agent-doc] closed superseded supervisor pane {} (window {}) for {}; authoritative owner pane {} (pid {})",
        pane,
        window,
        file.display(),
        authoritative_owner,
        supervisor_pid,
    );
    Ok(action)
}

/// Live panes other than `owner` whose process tree proves an agent-doc
/// supervisor for `file` (and that are not a bare harness session).
pub fn duplicate_owner_panes(tmux: &Tmux, file: &Path, owner: &str) -> Vec<String> {
    let Ok(listing) = agent_doc_tmux_io::list_panes_all(tmux, "#{pane_id}") else {
        return Vec::new();
    };
    // One `/proc` observation for the whole scan, not one walk per pane.
    let _observations = agent_doc_process_owner_io::begin_process_observation_scope();
    listing
        .lines()
        .map(str::trim)
        .filter(|pane| !pane.is_empty() && *pane != owner)
        .filter(|pane| {
            matches!(
                superseded_owner_decision(tmux, file, pane),
                SupersededOwnerAction::Retire { .. } | SupersededOwnerAction::Defer { .. }
            )
        })
        .map(str::to_string)
        .collect()
}

/// GH #243: before a route dispatches to `owner` (the document's authoritative
/// actor pane), retire every other live agent-doc supervisor pane for the same
/// document. The authoritative pane wins; a duplicate is a superseded owner
/// whose writes the controller already refuses. Returns the retired panes.
pub fn retire_duplicate_owner_panes(tmux: &Tmux, file: &Path, owner: &str) -> Vec<String> {
    let mut retired = Vec::new();
    for duplicate in duplicate_owner_panes(tmux, file, owner) {
        match retire_superseded_owner_pane(
            tmux,
            file,
            &duplicate,
            owner,
            SUPERSEDED_OWNER_EXIT_GRACE,
            "route_duplicate_owner",
        ) {
            Ok(SupersededOwnerAction::Retire { pane, .. }) => retired.push(pane),
            Ok(_) => {}
            Err(error) => agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "route_duplicate_owner_retire_failed file={} owner={} pane={} error={}",
                    file.display(),
                    owner,
                    duplicate,
                    format!("{error:#}").replace('\n', "\\n"),
                ),
            ),
        }
    }
    retired
}

/// Every descendant pid of `root` (not `root` itself), parents before children.
fn process_descendants(root: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut frontier = vec![root.to_string()];
    while let Some(pid) = frontier.pop() {
        for child in agent_doc_process_owner_io::child_pids(&pid) {
            if !out.contains(&child) && child != root {
                out.push(child.clone());
                frontier.push(child);
            }
        }
    }
    out
}

#[cfg(unix)]
fn pid_alive(pid: &str) -> bool {
    let Ok(pid) = pid.trim().parse::<libc::pid_t>() else {
        return false;
    };
    if pid <= 1 {
        return false;
    }
    // A zombie still answers kill(0); count it as exited.
    let zombie = std::fs::read_to_string(format!("/proc/{pid}/stat"))
        .ok()
        .and_then(|stat| {
            stat.rsplit_once(')')
                .map(|(_, rest)| rest.trim_start().starts_with('Z'))
        })
        .unwrap_or(false);
    !zombie && unsafe { libc::kill(pid, 0) } == 0
}

#[cfg(not(unix))]
fn pid_alive(_pid: &str) -> bool {
    false
}

/// SIGTERM `pid`, wait up to `grace`, then SIGKILL. True once it has exited.
#[cfg(unix)]
fn terminate_pid(pid: &str, grace: Duration) -> bool {
    let Ok(raw) = pid.trim().parse::<libc::pid_t>() else {
        return true;
    };
    if raw <= 1 {
        return false;
    }
    let wait_exit = |budget: Duration| {
        let deadline = std::time::Instant::now() + budget;
        while pid_alive(pid) {
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        true
    };
    unsafe { libc::kill(raw, libc::SIGTERM) };
    if wait_exit(grace) {
        return true;
    }
    unsafe { libc::kill(raw, libc::SIGKILL) };
    wait_exit(Duration::from_secs(2))
}

#[cfg(not(unix))]
fn terminate_pid(_pid: &str, _grace: Duration) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    /// Write an executable fake bare harness (`claude`). Its child keeps
    /// running after the parent dies.
    fn fake_binary(dir: &Path, name: &str) -> PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let path = dir.join(name);
        std::fs::write(&path, "#!/bin/sh\nsleep 1000 &\nwait\n").unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// A pane command whose process presents the command line `title` (as a
    /// real `agent-doc start <doc>` supervisor does) and keeps a child running
    /// that outlives it, like a harness under a supervisor.
    fn fake_process(title: &str) -> String {
        format!(
            "perl -e '$0 = q{{{title}}}; if (fork() == 0) {{ exec q{{sleep}}, q{{1000}} }} sleep 1000'"
        )
    }

    fn pane_running(tmux: &Tmux, session: &str, command: &str) -> String {
        let output = tmux
            .cmd()
            .args([
                "new-session",
                "-d",
                "-s",
                session,
                "-P",
                "-F",
                "#{pane_id}",
                command,
            ])
            .output()
            .unwrap();
        assert!(output.status.success(), "{output:?}");
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn wait_for_children(pid: u32) -> Vec<String> {
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let children = process_descendants(&pid.to_string());
            if !children.is_empty() || std::time::Instant::now() >= deadline {
                return children;
            }
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    struct Fixture {
        dir: tempfile::TempDir,
        doc: PathBuf,
        claude: PathBuf,
    }

    fn fixture() -> Fixture {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("doc-a.md");
        std::fs::write(&doc, "---\nagent_doc_session: gh243\n---\n").unwrap();
        let bin = dir.path().join("bin");
        std::fs::create_dir_all(&bin).unwrap();
        let claude = fake_binary(&bin, "claude");
        Fixture { dir, doc, claude }
    }

    fn ops_log(fixture: &Fixture) -> String {
        std::fs::read_to_string(fixture.dir.path().join(".agent-doc/logs/ops.log"))
            .unwrap_or_default()
    }

    /// GH #243: `start --force` retires the superseded agent-doc supervisor
    /// (and the harness under it) instead of leaving it running beside the new
    /// one, and never touches a bare harness agent-doc did not start.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn retire_superseded_owner_stops_a_proven_supervisor_and_spares_foreign_panes() {
        let fx = fixture();
        let tmux = agent_doc_tmux_io::isolated_test_tmux("gh243-force-retire");

        let owner_pane = pane_running(
            &tmux,
            "0",
            &fake_process(&format!("agent-doc start {}", fx.doc.display())),
        );
        // A transient `agent-doc <subcommand> <doc>` is not a supervisor.
        let transient_pane = pane_running(
            &tmux,
            "3",
            &fake_process(&format!("agent-doc write {}", fx.doc.display())),
        );
        let owner_pid = agent_doc_tmux_io::pane_pid(&tmux, &owner_pane).unwrap();
        let harness = wait_for_children(owner_pid);
        assert!(
            !harness.is_empty(),
            "fake supervisor never started its child"
        );

        let foreign_pane = pane_running(&tmux, "1", &fx.claude.display().to_string());
        let foreign_pid = agent_doc_tmux_io::pane_pid(&tmux, &foreign_pane).unwrap();
        wait_for_children(foreign_pid);

        let grace = Duration::from_secs(2);
        wait_for_children(agent_doc_tmux_io::pane_pid(&tmux, &transient_pane).unwrap());
        assert_eq!(
            retire_superseded_owner_pane(&tmux, &fx.doc, &transient_pane, "%owner", grace, "t",)
                .unwrap(),
            SupersededOwnerAction::LeaveUnproven {
                pane: transient_pane.clone()
            }
        );
        assert!(tmux.pane_alive(&transient_pane));
        // The operator's own `claude` is never ours to retire.
        assert_eq!(
            retire_superseded_owner_pane(&tmux, &fx.doc, &foreign_pane, "%owner", grace, "t",)
                .unwrap(),
            SupersededOwnerAction::LeaveForeignHarness {
                pane: foreign_pane.clone()
            }
        );
        assert!(tmux.pane_alive(&foreign_pane));
        assert!(pid_alive(&foreign_pid.to_string()));

        // A pane that does not run agent-doc for THIS document is unproven.
        let other_doc = fx.dir.path().join("doc-b.md");
        std::fs::write(&other_doc, "---\nagent_doc_session: other\n---\n").unwrap();
        assert_eq!(
            retire_superseded_owner_pane(&tmux, &other_doc, &owner_pane, "%owner", grace, "t",)
                .unwrap(),
            SupersededOwnerAction::LeaveUnproven {
                pane: owner_pane.clone()
            }
        );
        assert!(tmux.pane_alive(&owner_pane));

        assert_eq!(
            retire_superseded_owner_pane(&tmux, &fx.doc, &owner_pane, "%owner", grace, "t",)
                .unwrap(),
            SupersededOwnerAction::Retire {
                pane: owner_pane.clone(),
                supervisor_pid: owner_pid.to_string(),
            }
        );
        assert!(!pid_alive(&owner_pid.to_string()), "supervisor survived");
        for pid in &harness {
            assert!(
                !pid_alive(pid),
                "harness pid {pid} survived its retired supervisor"
            );
        }
        assert!(!tmux.pane_alive(&owner_pane), "retired pane was not closed");
        assert!(tmux.pane_alive(&foreign_pane), "foreign pane must survive");
        let ops = ops_log(&fx);
        assert!(
            ops.contains("t_retired")
                && ops.contains("t_left")
                && ops.contains("reason=foreign_harness"),
            "{ops}"
        );
    }

    /// GH #243 follow-up: the forced pane `%4` (authoritative, in a detached
    /// session) and the old route-owned pane `%2` (in the attached session)
    /// both ran a supervisor for one document. The authoritative pane wins;
    /// the duplicate exits, and a bare harness pane is left alone.
    #[test]
    #[ignore = "live tmux integration test; run `make tmux-ci`"]
    fn duplicate_owner_panes_are_retired_and_the_authoritative_pane_survives() {
        let fx = fixture();
        let tmux = agent_doc_tmux_io::isolated_test_tmux("gh243-duplicate-owner");
        let doc = fx.doc.display();
        let visible_duplicate = pane_running(
            &tmux,
            "0",
            &fake_process(&format!("agent-doc start --route-owned {doc}")),
        );
        let authoritative = pane_running(
            &tmux,
            "1",
            &fake_process(&format!("agent-doc start --force {doc}")),
        );
        let foreign = pane_running(&tmux, "2", &fx.claude.display().to_string());
        for pane in [&visible_duplicate, &authoritative, &foreign] {
            wait_for_children(agent_doc_tmux_io::pane_pid(&tmux, pane).unwrap());
        }

        assert_eq!(
            duplicate_owner_panes(&tmux, &fx.doc, &authoritative),
            vec![visible_duplicate.clone()]
        );
        assert_eq!(
            retire_duplicate_owner_panes(&tmux, &fx.doc, &authoritative),
            vec![visible_duplicate.clone()]
        );
        assert!(!tmux.pane_alive(&visible_duplicate));
        assert!(tmux.pane_alive(&authoritative));
        assert!(tmux.pane_alive(&foreign));
        assert!(duplicate_owner_panes(&tmux, &fx.doc, &authoritative).is_empty());
        assert!(
            ops_log(&fx).contains(&format!(
                "route_duplicate_owner_retired file={} pane={visible_duplicate}",
                fx.doc.display()
            )),
            "{}",
            ops_log(&fx)
        );
    }
}
