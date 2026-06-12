    use super::*;

    fn candidate(pane: &str, idle: u64, active: bool, agentdoc_stash: bool) -> StashTtlCandidate {
        StashTtlCandidate {
            pane_id: pane.to_string(),
            idle_secs: idle,
            is_active_pane: active,
            is_agent_doc_stash_pane: agentdoc_stash,
        }
    }

    #[test]
    fn disabled_ttl_never_prunes() {
        // ttl_secs == 0 (unset/disabled) keeps the feature inert.
        assert!(!stash_ttl_prune_candidate(99_999, 0, false, true));
        let targets = stash_ttl_prune_targets(&[candidate("%1", 99_999, false, true)], 0);
        assert!(targets.is_empty(), "disabled TTL must reap nothing");
    }

    #[test]
    fn active_pane_is_never_reaped() {
        assert!(!stash_ttl_prune_candidate(10_000, 300, true, true));
    }

    #[test]
    fn only_agent_doc_stash_panes_are_eligible() {
        assert!(!stash_ttl_prune_candidate(10_000, 300, false, false));
    }

    #[test]
    fn idle_must_strictly_exceed_ttl() {
        assert!(!stash_ttl_prune_candidate(300, 300, false, true));
        assert!(stash_ttl_prune_candidate(301, 300, false, true));
    }

    #[test]
    fn targets_filters_only_eligible_panes() {
        let candidates = vec![
            candidate("%idle-old", 1_000, false, true),   // eligible
            candidate("%active", 1_000, true, true),      // active → skip
            candidate("%fresh", 100, false, true),        // under TTL → skip
            candidate("%not-stash", 1_000, false, false), // not agent-doc stash → skip
        ];
        let targets = stash_ttl_prune_targets(&candidates, 300);
        assert_eq!(targets, vec!["%idle-old".to_string()]);
    }
