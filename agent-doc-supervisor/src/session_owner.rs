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

pub fn format_existing_pane_conflict_error(facts: &ExistingPaneConflictFacts<'_>) -> String {
    format!(
        "refusing to start {} in pane {} because pane {} is already bound to this document.\n\
\n\
Existing owner:\n\
  pane={} session={} window={}\n\
\n\
Current launcher pane:\n\
  pane={} session={} window={}\n\
\n\
Inspect the conflicting panes first:\n\
  tmux list-panes -a -F '#{{session_name}} #{{window_name}} #{{pane_id}} #{{pane_current_command}} #{{pane_current_path}}'\n\
  tmux capture-pane -pt {} | tail -n 80\n\
  tmux capture-pane -pt {} | tail -n 80\n\
\n\
If you want to keep the existing owner, kill this launcher pane yourself and rerun from the owner pane:\n\
  tmux kill-pane -t {}\n\
\n\
If you want to replace the existing owner, kill it yourself first and then rerun `agent-doc start` from pane {}:\n\
  tmux kill-pane -t {}",
        facts.document,
        facts.current_pane,
        facts.conflicting_pane,
        facts.conflicting_pane,
        facts.conflict_session,
        facts.conflict_window,
        facts.current_pane,
        facts.current_session,
        facts.current_window,
        facts.conflicting_pane,
        facts.current_pane,
        facts.current_pane,
        facts.current_pane,
        facts.conflicting_pane
    )
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

        assert!(
            rendered.contains(
                "refusing to start /repo/tasks/doc.md in pane %42 because pane %41 is already bound to this document"
            )
        );
        assert!(rendered.contains("pane=%41 session=owner-session window=@7"));
        assert!(rendered.contains("pane=%42 session=launcher-session window=@8"));
        assert!(rendered.contains("tmux list-panes -a"));
        assert!(rendered.contains("tmux capture-pane -pt %41 | tail -n 80"));
        assert!(rendered.contains("tmux capture-pane -pt %42 | tail -n 80"));
        assert!(rendered.contains("tmux kill-pane -t %42"));
        assert!(rendered.contains("tmux kill-pane -t %41"));
    }
}
