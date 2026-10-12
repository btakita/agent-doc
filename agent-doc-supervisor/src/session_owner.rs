#[derive(Debug, PartialEq, Eq)]
pub enum ExistingSessionPaneAction {
    Refuse(String),
}

#[derive(Debug, PartialEq, Eq)]
pub struct ExistingPaneConflictFacts<'a> {
    pub document: &'a str,
    pub current_pane: &'a str,
    pub conflicting_pane: &'a str,
    pub conflict_session: &'a str,
    pub conflict_window: &'a str,
    pub current_session: &'a str,
    pub current_window: &'a str,
}

/// Decide whether a launcher pane may claim a document session that may already
/// have a live owner.
///
/// A provable live owner wins over the registry entry because it is current
/// runtime evidence. Otherwise, a live registry pane different from the current
/// launcher pane blocks the start.
pub fn existing_session_pane_action(
    current_pane: &str,
    registry_pane: Option<&str>,
    registry_pane_alive: bool,
    live_owner: Option<&str>,
) -> Option<ExistingSessionPaneAction> {
    if let Some(owner) = live_owner
        && owner != current_pane
    {
        return Some(ExistingSessionPaneAction::Refuse(owner.to_string()));
    }

    let registry_pane = registry_pane?;
    if registry_pane == current_pane || !registry_pane_alive {
        return None;
    }
    Some(ExistingSessionPaneAction::Refuse(registry_pane.to_string()))
}

/// What happens to a live pane that no longer owns its document (GH #243):
/// the owner `agent-doc start --force` bypasses, or a duplicate supervisor pane
/// a route finds beside the document's authoritative actor pane.
///
/// One document must have one live supervisor pane. `--force` used to leave
/// the bypassed owner running, so one document had two live harness processes.
/// A pane is retired only when its process tree *proves* it is agent-doc's
/// supervisor for this document (`#bare-foreign-session-guard`: ownership is
/// proven, never assumed).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupersededOwnerAction {
    /// The pane runs `agent-doc` as the owner of this document: retire it.
    Retire {
        pane: String,
        supervisor_pid: String,
    },
    /// The pane runs a bare harness agent-doc did not start: leave it alone.
    LeaveForeignHarness { pane: String },
    /// Nothing in the pane proves agent-doc owns this document there (a stale
    /// registry row, a shell): leave it alone; `--force` only bypasses it.
    LeaveUnproven { pane: String },
}

/// Facts about the bypassed owner pane's process tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupersededOwnerFacts<'a> {
    pub pane: &'a str,
    /// The tree is a harness session agent-doc did not launch.
    pub runs_unmanaged_harness: bool,
    /// Pid of the tree's `agent-doc` process bound to this document, if any.
    pub agent_doc_owner_pid: Option<&'a str>,
}

pub fn superseded_owner_action(facts: SupersededOwnerFacts<'_>) -> SupersededOwnerAction {
    let pane = facts.pane.to_string();
    if facts.runs_unmanaged_harness {
        return SupersededOwnerAction::LeaveForeignHarness { pane };
    }
    match facts.agent_doc_owner_pid {
        Some(pid) if !pid.trim().is_empty() => SupersededOwnerAction::Retire {
            pane,
            supervisor_pid: pid.trim().to_string(),
        },
        _ => SupersededOwnerAction::LeaveUnproven { pane },
    }
}

pub fn format_existing_pane_conflict_error(facts: &ExistingPaneConflictFacts<'_>) -> String {
    terminal_error_lines([
        format!("refusing to start {}", facts.document),
        format!(
            "reason: pane {} already owns this document; current launcher is pane {}.",
            facts.conflicting_pane, facts.current_pane
        ),
        String::new(),
        "Existing owner:".to_string(),
        format!(
            "  pane={} session={} window={}",
            facts.conflicting_pane, facts.conflict_session, facts.conflict_window
        ),
        "Current launcher:".to_string(),
        format!(
            "  pane={} session={} window={}",
            facts.current_pane, facts.current_session, facts.current_window
        ),
        String::new(),
        "Inspect:".to_string(),
        "  tmux list-panes -a -F '#{pane_id} #{session_name}:#{window_name} #{pane_current_command}'"
            .to_string(),
        format!("  tmux capture-pane -pt {} | tail -n 80", facts.conflicting_pane),
        format!("  tmux capture-pane -pt {} | tail -n 80", facts.current_pane),
        String::new(),
        "Keep the existing owner:".to_string(),
        format!("  tmux kill-pane -t {}", facts.current_pane),
        String::new(),
        "Replace the existing owner:".to_string(),
        format!("  tmux kill-pane -t {}", facts.conflicting_pane),
        format!("  agent-doc start --force {}", facts.document),
    ])
}

fn terminal_error_lines(lines: impl IntoIterator<Item = String>) -> String {
    let mut rendered = lines.into_iter().collect::<Vec<_>>().join("\r\n");
    rendered.push('\r');
    rendered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn live_owner_other_than_current_refuses_start() {
        assert_eq!(
            existing_session_pane_action("%1", Some("%2"), false, Some("%3")),
            Some(ExistingSessionPaneAction::Refuse("%3".to_string()))
        );
    }

    #[test]
    fn current_live_owner_still_refuses_different_live_registry_pane() {
        assert_eq!(
            existing_session_pane_action("%1", Some("%2"), true, Some("%1")),
            Some(ExistingSessionPaneAction::Refuse("%2".to_string()))
        );
    }

    #[test]
    fn live_registry_pane_other_than_current_refuses_start() {
        assert_eq!(
            existing_session_pane_action("%1", Some("%2"), true, None),
            Some(ExistingSessionPaneAction::Refuse("%2".to_string()))
        );
    }

    #[test]
    fn force_start_retires_only_a_proven_agent_doc_owner() {
        assert_eq!(
            superseded_owner_action(SupersededOwnerFacts {
                pane: "%2",
                runs_unmanaged_harness: false,
                agent_doc_owner_pid: Some("41630"),
            }),
            SupersededOwnerAction::Retire {
                pane: "%2".to_string(),
                supervisor_pid: "41630".to_string(),
            }
        );
        // `#bare-foreign-session-guard`: an operator's own harness is never ours.
        assert_eq!(
            superseded_owner_action(SupersededOwnerFacts {
                pane: "%2",
                runs_unmanaged_harness: true,
                agent_doc_owner_pid: Some("41630"),
            }),
            SupersededOwnerAction::LeaveForeignHarness {
                pane: "%2".to_string()
            }
        );
        // No proof (stale registry row, idle shell): bypass, never kill.
        for pid in [None, Some(""), Some("  ")] {
            assert_eq!(
                superseded_owner_action(SupersededOwnerFacts {
                    pane: "%2",
                    runs_unmanaged_harness: false,
                    agent_doc_owner_pid: pid,
                }),
                SupersededOwnerAction::LeaveUnproven {
                    pane: "%2".to_string()
                }
            );
        }
    }

    #[test]
    fn dead_registry_pane_allows_start() {
        assert_eq!(
            existing_session_pane_action("%1", Some("%2"), false, None),
            None
        );
    }

    #[test]
    fn conflict_error_includes_manual_tmux_resolution_commands() {
        let rendered = format_existing_pane_conflict_error(&ExistingPaneConflictFacts {
            document: "/repo/tasks/doc.md",
            current_pane: "%42",
            conflicting_pane: "%41",
            conflict_session: "owner-session",
            conflict_window: "@7",
            current_session: "launcher-session",
            current_window: "@8",
        });

        assert!(rendered.contains("refusing to start /repo/tasks/doc.md"));
        assert!(
            rendered.contains(
                "reason: pane %41 already owns this document; current launcher is pane %42"
            )
        );
        assert!(rendered.contains("pane=%41 session=owner-session window=@7"));
        assert!(rendered.contains("pane=%42 session=launcher-session window=@8"));
        assert!(rendered.contains("tmux list-panes -a"));
        assert!(rendered.contains("tmux capture-pane -pt %41 | tail -n 80"));
        assert!(rendered.contains("tmux capture-pane -pt %42 | tail -n 80"));
        assert!(rendered.contains("tmux kill-pane -t %42"));
        assert!(rendered.contains("tmux kill-pane -t %41"));
        assert!(rendered.contains("agent-doc start --force /repo/tasks/doc.md"));
        assert!(
            rendered.contains("\r\nExisting owner:\r\n"),
            "multi-line terminal diagnostics should carry CRLF newlines: {rendered:?}"
        );
        assert!(
            rendered.ends_with('\r'),
            "final CLI newline should return to column 0 even when ONLCR is disabled"
        );
        assert!(
            rendered.lines().all(|line| line.len() <= 110),
            "diagnostic should avoid hard-to-read long terminal lines: {rendered}"
        );
    }
}
