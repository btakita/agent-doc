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
/// proven, never assumed) and live safety evidence proves it has no active
/// turn, protected input, or attached client displaying it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SupersededOwnerAction {
    /// The pane runs `agent-doc` as the owner of this document: retire it.
    Retire {
        pane: String,
        supervisor_pid: String,
    },
    /// The pane is a proven duplicate, but retiring it now could destroy
    /// operator work. Leave it alive until a later safe-boundary observation.
    Defer {
        pane: String,
        supervisor_pid: String,
        protection: SupersededOwnerProtection,
    },
    /// The pane runs a bare harness agent-doc did not start: leave it alone.
    LeaveForeignHarness { pane: String },
    /// Nothing in the pane proves agent-doc owns this document there (a stale
    /// registry row, a shell): leave it alone; `--force` only bypasses it.
    LeaveUnproven { pane: String },
}

/// Safety evidence that prevents immediate duplicate-owner retirement.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupersededOwnerProtection {
    pub active_turn: bool,
    pub protected_input_reason: Option<String>,
    /// `true` also covers an unreadable screen placement: destructive actions
    /// fail closed when tmux cannot prove that no client displays the pane.
    pub client_visible_or_unobservable: bool,
}

impl SupersededOwnerProtection {
    pub fn is_protected(&self) -> bool {
        self.active_turn
            || self.protected_input_reason.is_some()
            || self.client_visible_or_unobservable
    }

    pub fn reason(&self) -> String {
        let mut reasons = Vec::new();
        if self.active_turn {
            reasons.push("active_turn".to_string());
        }
        if let Some(reason) = &self.protected_input_reason {
            reasons.push(format!("protected_input:{reason}"));
        }
        if self.client_visible_or_unobservable {
            reasons.push("client_visible_or_unobservable".to_string());
        }
        reasons.join(",")
    }
}

/// Decision made by a duplicate supervisor when it observes a durable
/// retirement request in its idle watcher.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeferredDuplicateRetirementAction {
    /// The named authoritative owner changed or disappeared; the request is
    /// stale and must not retire this supervisor.
    CancelAuthorityChanged,
    /// The request still applies, but a destructive protection remains live.
    Wait(SupersededOwnerProtection),
    /// The authority still matches and every protection is clear.
    Retire,
}

pub fn deferred_duplicate_retirement_action(
    authority_matches: bool,
    protection: SupersededOwnerProtection,
) -> DeferredDuplicateRetirementAction {
    if !authority_matches {
        DeferredDuplicateRetirementAction::CancelAuthorityChanged
    } else if protection.is_protected() {
        DeferredDuplicateRetirementAction::Wait(protection)
    } else {
        DeferredDuplicateRetirementAction::Retire
    }
}

/// Facts about the bypassed owner pane's process tree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SupersededOwnerFacts<'a> {
    pub pane: &'a str,
    /// The tree is a harness session agent-doc did not launch.
    pub runs_unmanaged_harness: bool,
    /// Pid of the tree's `agent-doc` process bound to this document, if any.
    pub agent_doc_owner_pid: Option<&'a str>,
    pub active_turn: bool,
    pub protected_input_reason: Option<&'a str>,
    pub client_visible_or_unobservable: bool,
}

pub fn superseded_owner_action(facts: SupersededOwnerFacts<'_>) -> SupersededOwnerAction {
    let pane = facts.pane.to_string();
    if facts.runs_unmanaged_harness {
        return SupersededOwnerAction::LeaveForeignHarness { pane };
    }
    match facts.agent_doc_owner_pid {
        Some(pid) if !pid.trim().is_empty() => {
            let supervisor_pid = pid.trim().to_string();
            let protection = SupersededOwnerProtection {
                active_turn: facts.active_turn,
                protected_input_reason: facts.protected_input_reason.map(str::to_string),
                client_visible_or_unobservable: facts.client_visible_or_unobservable,
            };
            if protection.is_protected() {
                SupersededOwnerAction::Defer {
                    pane,
                    supervisor_pid,
                    protection,
                }
            } else {
                SupersededOwnerAction::Retire {
                    pane,
                    supervisor_pid,
                }
            }
        }
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
                active_turn: false,
                protected_input_reason: None,
                client_visible_or_unobservable: false,
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
                active_turn: true,
                protected_input_reason: Some("typed prompt input"),
                client_visible_or_unobservable: true,
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
                    active_turn: true,
                    protected_input_reason: Some("typed prompt input"),
                    client_visible_or_unobservable: true,
                }),
                SupersededOwnerAction::LeaveUnproven {
                    pane: "%2".to_string()
                }
            );
        }
    }

    #[test]
    fn proven_owner_defers_for_every_destructive_protection() {
        for (active_turn, protected_input_reason, client_visible_or_unobservable, expected) in [
            (true, None, false, "active_turn"),
            (
                false,
                Some("typed prompt input"),
                false,
                "protected_input:typed prompt input",
            ),
            (false, None, true, "client_visible_or_unobservable"),
        ] {
            let action = superseded_owner_action(SupersededOwnerFacts {
                pane: "%2",
                runs_unmanaged_harness: false,
                agent_doc_owner_pid: Some("41630"),
                active_turn,
                protected_input_reason,
                client_visible_or_unobservable,
            });
            let SupersededOwnerAction::Defer { protection, .. } = action else {
                panic!("protected owner must defer: {action:?}");
            };
            assert_eq!(protection.reason(), expected);
        }
    }

    #[test]
    fn deferred_retirement_revalidates_authority_and_every_protection() {
        let safe = SupersededOwnerProtection {
            active_turn: false,
            protected_input_reason: None,
            client_visible_or_unobservable: false,
        };
        assert_eq!(
            deferred_duplicate_retirement_action(false, safe.clone()),
            DeferredDuplicateRetirementAction::CancelAuthorityChanged,
        );
        assert_eq!(
            deferred_duplicate_retirement_action(true, safe),
            DeferredDuplicateRetirementAction::Retire,
        );

        for protection in [
            SupersededOwnerProtection {
                active_turn: true,
                protected_input_reason: None,
                client_visible_or_unobservable: false,
            },
            SupersededOwnerProtection {
                active_turn: false,
                protected_input_reason: Some("typed prompt input".to_string()),
                client_visible_or_unobservable: false,
            },
            SupersededOwnerProtection {
                active_turn: false,
                protected_input_reason: None,
                client_visible_or_unobservable: true,
            },
        ] {
            assert_eq!(
                deferred_duplicate_retirement_action(true, protection.clone()),
                DeferredDuplicateRetirementAction::Wait(protection),
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
