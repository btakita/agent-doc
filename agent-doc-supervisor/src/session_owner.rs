#[derive(Debug, PartialEq, Eq)]
pub enum ExistingSessionPaneAction {
    Refuse(String),
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
}
