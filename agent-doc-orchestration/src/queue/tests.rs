    use super::*;

    fn ids(value: &[&str]) -> Vec<String> {
        value.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reference_directive_excluded_from_prompts() {
        // Optional-`do` Stage 1: `re [#id]` / `re #id` references a tracked id
        // without executing it — never a runnable prompt, preserved verbatim.
        let entries = parse("- re [#opt]\n- re #opt2\n- do [#run]\n- [#bare]\n").unwrap();
        let prompt_texts: Vec<&str> = prompts(&entries).iter().map(|p| p.text.as_str()).collect();
        assert!(
            !prompt_texts.iter().any(|t| t.starts_with("re ")),
            "re-references must not be runnable prompts: {prompt_texts:?}"
        );
        // `do [#id]` and bare `[#id]` still execute.
        assert!(prompt_texts.contains(&"do [#run]"));
        assert!(prompt_texts.contains(&"[#bare]"));
        // The references round-trip verbatim as Freeform.
        let rendered = render(&entries);
        assert!(rendered.contains("- re [#opt]"));
        assert!(rendered.contains("- re #opt2"));
    }

    #[test]
    fn strip_priority_markers_normalizes_pin_for_identity() {
        // #queue-consume-pushpin-normalization: a head differing only by a
        // cosmetic pin annotation must normalize to the same identity, so the
        // queue-consume head-equality check does not spuriously fail when the
        // snapshot holds the unpinned spelling and the document the pinned one.
        let bare = "do [#md-ast-document-model]. I like the tsift AST.";
        assert_eq!(strip_priority_markers(bare), bare);
        assert_eq!(strip_priority_markers(":pushpin: do [#x]"), "do [#x]");
        assert_eq!(strip_priority_markers("  :pushpin:   do [#x]"), "do [#x]");
        assert_eq!(strip_priority_markers(":round_pushpin: do [#x]"), "do [#x]");
        assert_eq!(strip_priority_markers("**prioritized** do [#x]"), "do [#x]");
        assert_eq!(strip_priority_markers("📌 do [#x]"), "do [#x]");
        // The exact session repro: pinned vs unpinned spellings are equal.
        assert_eq!(
            strip_priority_markers(":pushpin: do [#md-ast-document-model]"),
            strip_priority_markers("do [#md-ast-document-model]")
        );
        // A non-pin free-text head is untouched (no false stripping).
        assert_eq!(
            strip_priority_markers("Should tsift be a hard dependency?"),
            "Should tsift be a hard dependency?"
        );
    }

    #[test]
    fn annotate_agent_priority_promotions_marks_promoted_prompt() {
        let before = parse("- do [#low]\n- do [#high]\n").unwrap();
        let after = parse("- do [#high]\n- do [#low]\n").unwrap();
        let marked =
            annotate_agent_priority_promotions(&before, &after).expect("promotion should annotate");
        assert_eq!(
            render(&marked),
            "- :round_pushpin: do [#high]\n- do [#low]\n"
        );
    }

    #[test]
    fn annotate_operator_priority_reorders_marks_manually_moved_prompt() {
        let snapshot = parse("- do [#a]\n- do [#b]\n- do [#c]\n").unwrap();
        let current = parse("- do [#c]\n- do [#a]\n- do [#b]\n").unwrap();

        let marked = annotate_operator_priority_reorders(&snapshot, &current)
            .expect("manual promotion should annotate");

        assert_eq!(
            render(&marked),
            "- :pushpin: do [#c]\n- do [#a]\n- do [#b]\n"
        );
    }

    #[test]
    fn annotate_operator_priority_reorders_upgrades_agent_pin() {
        let snapshot = parse("- do [#a]\n- :round_pushpin: do [#b]\n").unwrap();
        let current = parse("- :round_pushpin: do [#b]\n- do [#a]\n").unwrap();

        let marked = annotate_operator_priority_reorders(&snapshot, &current)
            .expect("operator move should add operator pin");

        assert_eq!(
            render(&marked),
            "- :pushpin: :round_pushpin: do [#b]\n- do [#a]\n"
        );
    }

    #[test]
    fn annotate_operator_priority_reorders_ignores_new_and_later_prompts() {
        let snapshot = parse("- do [#a]\n- do [#b]\n").unwrap();
        let current = parse("- do [#new]\n- do [#b]\n- do [#a]\n").unwrap();

        assert!(annotate_operator_priority_reorders(&snapshot, &current).is_none());
    }

    #[test]
    fn annotate_manual_queue_additions_pins_operator_added_unpinned_line() {
        // #7r2s: the operator typed a brand-new `do [#manual]` line with no pin.
        // It is absent from the snapshot and was NOT appended by the backlog sync,
        // so it is auto-pinned with operator priority and stays at its slot.
        let snapshot = parse("- do [#a]\n").unwrap();
        let current = parse("- do [#manual]\n- do [#a]\n").unwrap();
        let synced: std::collections::HashSet<String> = std::collections::HashSet::new();

        let marked = annotate_manual_queue_additions(&snapshot, &current, &synced)
            .expect("a new operator-added line must be auto-pinned");
        assert_eq!(render(&marked), "- :pushpin: do [#manual]\n- do [#a]\n");
    }

    #[test]
    fn annotate_manual_queue_additions_skips_backlog_synced_and_pinned() {
        // A new line the binary appended from the backlog this cycle (#synced) is
        // NOT auto-pinned; an already-pinned new line is left as-is; an existing
        // (snapshot) line is untouched.
        let snapshot = parse("- do [#a]\n").unwrap();
        let current = parse("- do [#synced]\n- :round_pushpin: do [#pinned]\n- do [#a]\n").unwrap();
        let synced: std::collections::HashSet<String> =
            ["synced".to_string()].into_iter().collect();

        assert!(
            annotate_manual_queue_additions(&snapshot, &current, &synced).is_none(),
            "binary-synced and already-pinned new lines must not be auto-pinned"
        );
    }

    #[test]
    fn reference_directive_does_not_match_prose() {
        assert!(is_reference_directive("re [#opt]"));
        assert!(is_reference_directive("re #opt"));
        assert!(is_reference_directive("  re   [#opt] some note"));
        // Real words beginning with "re" are not references.
        assert!(!is_reference_directive("rebuild the index"));
        assert!(!is_reference_directive("re-run the tests"));
        assert!(!is_reference_directive("reference [#opt] in passing"));
        assert!(!is_reference_directive("re the meeting"));
        assert!(!is_reference_directive("do [#opt]"));
    }

    #[test]
    fn backlog_queue_sync_mode_parse() {
        use BacklogQueueSyncMode::*;
        assert_eq!(BacklogQueueSyncMode::parse(""), Some(Append));
        assert_eq!(BacklogQueueSyncMode::parse("append"), Some(Append));
        assert_eq!(BacklogQueueSyncMode::parse("sync"), Some(Sync));
        assert_eq!(BacklogQueueSyncMode::parse("prepend"), Some(Prepend));
        assert_eq!(BacklogQueueSyncMode::parse(" sync "), Some(Sync));
        assert_eq!(BacklogQueueSyncMode::parse("nope"), None);
    }

    #[test]
    fn sync_mode_fully_mirrors_backlog_order() {
        let entries = parse("- do [#old]\npreset spec\n").unwrap();
        let synced =
            sync_backlog_into_queue(&entries, &ids(&["a", "b"]), BacklogQueueSyncMode::Sync)
                .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\n");
    }

    #[test]
    fn sync_mode_idempotent_when_already_mirrored() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        assert!(
            sync_backlog_into_queue(&entries, &ids(&["a", "b"]), BacklogQueueSyncMode::Sync)
                .is_none(),
            "already-mirrored queue must not re-mutate"
        );
    }

    #[test]
    fn append_mode_adds_only_missing_ids_at_tail() {
        let entries = parse("- do [#a]\n").unwrap();
        let synced = sync_backlog_into_queue(
            &entries,
            &ids(&["a", "b", "c"]),
            BacklogQueueSyncMode::Append,
        )
        .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\n- do [#c]\n");
    }

    #[test]
    fn append_mode_skips_struck_completed_ids() {
        // A consumed/struck item must not be re-appended.
        let entries = parse("- ~do [#a]~\n").unwrap();
        assert!(
            sync_backlog_into_queue(&entries, &ids(&["a"]), BacklogQueueSyncMode::Append).is_none(),
            "struck id should count as present"
        );
    }

    #[test]
    fn prepend_mode_inserts_missing_ids_at_front_in_backlog_order() {
        let entries = parse("- do [#z]\n").unwrap();
        let synced = sync_backlog_into_queue(
            &entries,
            &ids(&["a", "b", "z"]),
            BacklogQueueSyncMode::Prepend,
        )
        .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\n- do [#z]\n");
    }

    #[test]
    fn sync_dedupes_and_normalizes_case() {
        let entries: Vec<QueueEntry> = Vec::new();
        let synced =
            sync_backlog_into_queue(&entries, &ids(&["A", "a", "B"]), BacklogQueueSyncMode::Sync)
                .expect("queue should change");
        assert_eq!(render(&synced), "- do [#a]\n- do [#b]\n");
    }

    #[test]
    fn sort_prompts_by_priority_orders_do_prompts() {
        let entries = parse("- do [#a]\n- do [#b]\n- do [#c]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 3u8);
        rank.insert("b".to_string(), 1u8);
        rank.insert("c".to_string(), 2u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("order should change");
        assert_eq!(render(&sorted), "- do [#b]\n- do [#c]\n- do [#a]\n");
    }

    #[test]
    fn sort_prompts_by_priority_keeps_non_prompts_in_place() {
        let entries = parse("preset spec\n- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("order should change");
        // preset stays at index 0; prompts reorder among themselves.
        assert_eq!(render(&sorted), "preset spec\n- do [#b]\n- do [#a]\n");
    }

    #[test]
    fn sort_prompts_by_priority_idempotent_when_ordered() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        rank.insert("b".to_string(), 2u8);
        assert!(sort_prompts_by_priority(&entries, &rank).is_none());
    }

    #[test]
    fn operator_pin_stays_in_place_while_unpinned_reorder() {
        // #queue-operator-pin-position-lock: a `__prioritized__` operator pin
        // stays at its authored slot; only the unpinned prompts reorder around it.
        let entries = parse("- do [#c]\n- __prioritized__ do [#b]\n- do [#a]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8); // best rank → first among unpinned
        rank.insert("b".to_string(), 9u8); // worst rank, but pinned → frozen in middle
        rank.insert("c".to_string(), 2u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("unpinned reorder");
        // Pin #b holds slot 1; unpinned #a (rank1) and #c (rank2) fill slots 0,2.
        assert_eq!(
            render(&sorted),
            "- do [#a]\n- __prioritized__ do [#b]\n- do [#c]\n"
        );
    }

    #[test]
    fn operator_pin_at_bottom_not_floated_to_top() {
        // The exact operator complaint: a pinned item at the bottom must NOT be
        // hoisted to the top by the priority attribute — it stays put.
        let entries = parse("- do [#a]\n- __prioritized__ do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        rank.insert("b".to_string(), 9u8);
        // #a is already in rank order at slot 0 and the pin is anchored at slot 1
        // → nothing moves.
        assert!(sort_prompts_by_priority(&entries, &rank).is_none());
    }

    #[test]
    fn operator_pins_hold_their_slots_unpinned_reorder_around() {
        // Multiple operator pins each hold their own slot; the unpinned prompts
        // reorder among the remaining slots in rank order.
        let entries =
            parse("- do [#a]\n- __prioritized__ [#z]\n- do [#b]\n- __prioritized__ [#y]\n")
                .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("unpinned reorder");
        // Pins #z, #y stay at slots 1,3; unpinned #b (rank1), #a (rank2) fill 0,2.
        assert_eq!(
            render(&sorted),
            "- do [#b]\n- __prioritized__ [#z]\n- do [#a]\n- __prioritized__ [#y]\n"
        );
    }

    #[test]
    fn prioritized_marker_release_reverts_to_rank_order() {
        // Deleting the marker drops the item back into rank-ordered position.
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 2u8);
        rank.insert("b".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("rank reorders");
        assert_eq!(render(&sorted), "- do [#b]\n- do [#a]\n");
    }

    #[test]
    fn operator_pin_position_locked_with_empty_rank_map() {
        // No backlog priority at all: the operator pin stays exactly where placed
        // (previously it floated to the top — that is the behavior being removed).
        let entries = parse("- do [#a]\n- __prioritized__ do [#b]\n").unwrap();
        let rank = std::collections::HashMap::new();
        assert!(sort_prompts_by_priority(&entries, &rank).is_none());
    }

    #[test]
    fn is_prioritized_detects_marker() {
        assert!(is_prioritized("__prioritized__ do [#x]"));
        assert!(is_prioritized("  __prioritized__ [#x]"));
        assert!(!is_prioritized("do [#x]"));
        assert!(!is_prioritized("not __prioritized__ here"));
    }

    #[test]
    fn operator_pin_anchored_while_agent_pin_floats_among_movable() {
        // #queue-agent-vs-operator-pin-tier + #queue-operator-pin-position-lock:
        // the operator pin (__) is anchored at its slot; among the remaining
        // (movable) slots the agent pin (_) still floats above unpinned.
        let entries = parse(concat!(
            "- do [#a]\n",
            "- _prioritized_ do [#b]\n",
            "- __prioritized__ do [#c]\n",
        ))
        .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8); // best rank, but unpinned
        rank.insert("b".to_string(), 5u8);
        rank.insert("c".to_string(), 9u8); // operator-pinned → stays at slot 2
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("agent pin floats");
        // Operator pin #c stays at the bottom (slot 2); agent pin #b floats above
        // unpinned #a in the two movable slots.
        assert_eq!(
            render(&sorted),
            "- _prioritized_ do [#b]\n- do [#a]\n- __prioritized__ do [#c]\n"
        );
    }

    #[test]
    fn is_agent_prioritized_distinguishes_single_from_double_underscore() {
        assert!(is_agent_prioritized("_prioritized_ do [#x]"));
        assert!(!is_agent_prioritized("__prioritized__ do [#x]")); // operator pin, not agent
        assert!(!is_prioritized("_prioritized_ do [#x]")); // agent pin is not operator pin
        assert!(!is_agent_prioritized("do [#x]"));
    }

    #[test]
    fn pin_markers_accept_both_asterisk_and_underscore_emphasis() {
        // Operator = strong emphasis (** / __); agent = italic emphasis (* / _).
        assert!(is_prioritized("**prioritized** do [#x]"));
        assert!(is_prioritized("__prioritized__ do [#x]"));
        assert!(is_agent_prioritized("*prioritized* do [#x]"));
        assert!(is_agent_prioritized("_prioritized_ do [#x]"));
        // Strong emphasis is operator, never agent — for both spellings.
        assert!(!is_agent_prioritized("**prioritized** do [#x]"));
        assert!(!is_agent_prioritized("__prioritized__ do [#x]"));
    }

    #[test]
    fn pin_shortcode_aliases_resolve_to_tiers() {
        // Operator aliases: :pin:, :pushpin:, 📌, **pin**, __pin__.
        for m in [
            "**prioritized**",
            "__prioritized__",
            "**pin**",
            "__pin__",
            ":pin:",
            ":pushpin:",
            "📌",
        ] {
            assert!(is_prioritized(&format!("{m} do [#x]")), "operator: {m}");
            assert!(
                !is_agent_prioritized(&format!("{m} do [#x]")),
                "not agent: {m}"
            );
        }
        // Agent aliases: _pin_, :round_pushpin:, 📍, *pin*, *prioritized*.
        for m in [
            "*prioritized*",
            "_prioritized_",
            "*pin*",
            "_pin_",
            ":round_pushpin:",
            "📍",
        ] {
            assert!(is_agent_prioritized(&format!("{m} do [#x]")), "agent: {m}");
            assert!(
                !is_prioritized(&format!("{m} do [#x]")),
                "not operator: {m}"
            );
        }
        // Tier ordering with emoji shortcodes: the operator pin (:pushpin:) is
        // position-locked at its slot; the agent pin (:round_pushpin:) floats
        // above the unpinned prompt among the remaining slots.
        let entries = parse(concat!(
            "- do [#a]\n",
            "- :round_pushpin: do [#b]\n",
            "- :pushpin: do [#c]\n",
        ))
        .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("tiers reorder");
        assert_eq!(
            render(&sorted),
            "- :round_pushpin: do [#b]\n- do [#a]\n- :pushpin: do [#c]\n"
        );
    }

    #[test]
    fn dag_orders_dependency_before_dependent() {
        // #queue-auto-dag-priority: `after=#a` forces #a before #b even though #b
        // has the better priority rank.
        let entries = parse("- do [#b]\n- do [#a]\n").unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("b".to_string(), 1u8); // best rank
        rank.insert("a".to_string(), 9u8);
        let mut deps = std::collections::HashMap::new();
        deps.insert("b".to_string(), vec!["a".to_string()]); // b after a
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps).expect("dep reorders");
        assert_eq!(render(&sorted), "- do [#a]\n- do [#b]\n");
    }

    #[test]
    fn dag_blocker_outranks_pin() {
        // A pinned item that depends on an unpinned item cannot float above it.
        let entries = parse("- :pushpin: do [#b]\n- do [#a]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let mut deps = std::collections::HashMap::new();
        deps.insert("b".to_string(), vec!["a".to_string()]); // pinned b after a
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps).expect("blocker wins");
        assert_eq!(render(&sorted), "- do [#a]\n- :pushpin: do [#b]\n");
    }

    #[test]
    fn dag_operator_pin_position_locked_with_movable_edges() {
        // #queue-operator-pin-position-lock-dag: with `after=#id` edges among the
        // movable prompts, the operator pin must stay anchored at its document
        // slot — not float to the front by tier — while the movable prompts
        // reorder around it to satisfy the dependency.
        let entries = parse("- do [#y] after=#x\n- :pushpin: do [#p]\n- do [#x]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let deps = std::collections::HashMap::new();
        let sorted =
            sort_prompts_by_dag(&entries, &rank, &deps).expect("movable edge reorders around pin");
        assert_eq!(
            render(&sorted),
            "- do [#x]\n- :pushpin: do [#p]\n- do [#y] after=#x\n"
        );
    }

    #[test]
    fn dag_operator_pin_no_spurious_reorder_with_edges() {
        // The pin is already at its anchored slot and the movable order already
        // satisfies the edge → the DAG sort must not spuriously float the pin
        // forward (the pre-fix bug returned `#p, #x, #y`). It returns None.
        let entries = parse("- do [#x]\n- :pushpin: do [#p]\n- do [#y] after=#x\n").unwrap();
        let rank = std::collections::HashMap::new();
        let deps = std::collections::HashMap::new();
        assert!(
            sort_prompts_by_dag(&entries, &rank, &deps).is_none(),
            "operator pin at its slot with a satisfied edge must not be reordered"
        );
    }

    #[test]
    fn dag_returns_none_without_edges() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let deps = std::collections::HashMap::new();
        assert!(sort_prompts_by_dag(&entries, &rank, &deps).is_none());
    }

    #[test]
    fn dag_inline_after_token_on_queue_prompt() {
        // `after=#a` declared inline on the queue prompt text (not via backlog).
        let entries = parse("- do [#b] after=#a\n- do [#a]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let deps = std::collections::HashMap::new();
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps).expect("inline dep reorders");
        assert_eq!(render(&sorted), "- do [#a]\n- do [#b] after=#a\n");
    }

    #[test]
    fn dag_cycle_does_not_drop_prompts() {
        // a after b, b after a → cycle; both still emitted (priority order).
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        let rank = std::collections::HashMap::new();
        let mut deps = std::collections::HashMap::new();
        deps.insert("a".to_string(), vec!["b".to_string()]);
        deps.insert("b".to_string(), vec!["a".to_string()]);
        let sorted = sort_prompts_by_dag(&entries, &rank, &deps);
        // Either reordered or None (already-ordered), but never fewer prompts.
        if let Some(s) = sorted {
            assert_eq!(prompts(&s).len(), 2);
        }
    }

    #[test]
    fn completed_item_renders_double_tilde_strikethrough() {
        // #queue-strike-on-complete: a completed single-line item renders as
        // markdown strikethrough (`~~x~~`), and round-trips through the parser.
        let entries = vec![QueueEntry::Completed(QueuePrompt {
            text: "do [#x]".to_string(),
            multiline: false,
        })];
        let rendered = render(&entries);
        assert_eq!(rendered, "- ~~do [#x]~~\n");
        // Legacy single-tilde residue still parses back as Completed.
        let reparsed = parse("- ~~do [#x]~~\n").unwrap();
        assert!(matches!(&reparsed[0], QueueEntry::Completed(p) if p.text == "do [#x]"));
        let legacy = parse("- ~do [#x]~\n").unwrap();
        assert!(matches!(&legacy[0], QueueEntry::Completed(p) if p.text == "do [#x]"));
    }

    #[test]
    fn item_after_deps_parses_tokens() {
        assert_eq!(
            agent_doc_core::pending::item_after_deps("do the thing after=#a"),
            vec!["a".to_string()]
        );
        assert_eq!(
            agent_doc_core::pending::item_after_deps("x after=#a,#b more"),
            vec!["a".to_string(), "b".to_string()]
        );
        // word-boundary guard: `hereafter=` must not match.
        assert!(agent_doc_core::pending::item_after_deps("hereafter=#a").is_empty());
    }

    #[test]
    fn pin_tiers_with_asterisk_emphasis() {
        // Operator pin (**strong**) is position-locked at slot 2; agent pin
        // (*italic*) floats above the unpinned prompt in the movable slots.
        let entries = parse(concat!(
            "- do [#a]\n",
            "- *prioritized* do [#b]\n",
            "- **prioritized** do [#c]\n",
        ))
        .unwrap();
        let mut rank = std::collections::HashMap::new();
        rank.insert("a".to_string(), 1u8);
        let sorted = sort_prompts_by_priority(&entries, &rank).expect("tiers reorder");
        assert_eq!(
            render(&sorted),
            "- *prioritized* do [#b]\n- do [#a]\n- **prioritized** do [#c]\n"
        );
    }

    #[test]
    fn dedup_live_prompts_collapses_duplicate_live_prompt() {
        // #adoc-queue-ipc-drift: a merge-duplicated live head must collapse to one,
        // while Completed residue / Preset entries are preserved.
        let entries = parse(concat!(
            "preset #spec-test-build-install-commit-push\n",
            "- ~do [#adoc-sqlite-seam]~\n",
            "- do [#adoc-orch-shim-cleanup]\n",
            "- do [#adoc-orch-shim-cleanup]\n",
        ))
        .unwrap();
        let deduped = dedup_live_prompts(&entries).expect("duplicate should be collapsed");
        assert_eq!(
            prompts(&deduped).len(),
            1,
            "duplicate live prompt collapses to one: {deduped:?}"
        );
        assert_eq!(
            deduped
                .iter()
                .filter(|e| matches!(e, QueueEntry::Completed(_)))
                .count(),
            1
        );
        assert!(deduped.iter().any(|e| matches!(e, QueueEntry::Preset(_))));
    }

    #[test]
    fn dedup_live_prompts_preserves_free_text_duplicates() {
        let entries = parse(concat!("- do deploy\n", "- do deploy\n")).unwrap();
        let deduped = dedup_live_prompts(&entries);
        assert!(
            deduped.is_none(),
            "free-text duplicate prompts should stay as user intent"
        );
        assert_eq!(
            prompts(&entries).len(),
            2,
            "free-text duplicate prompts are intentionally preserved: {entries:?}"
        );
    }

    #[test]
    fn dedup_live_prompts_noop_without_duplicates() {
        let entries = parse("- do [#a]\n- do [#b]\n").unwrap();
        assert!(
            dedup_live_prompts(&entries).is_none(),
            "no duplicates → no mutation"
        );
    }

    #[test]
    fn parse_single_line_items() {
        let body = "- do #fix1\n- do #fix2\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            })
        );
        assert_eq!(
            entries[1],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            })
        );
        assert_eq!(
            entries[2],
            QueueEntry::Prompt(QueuePrompt {
                text: "run tests".to_string(),
                multiline: false,
            })
        );
    }

    #[test]
    fn parse_multiline_tilde_prompt() {
        let body = "~~~prompt\nReview the changes in src/.\nCheck for edge cases.\n~~~\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "Review the changes in src/.\nCheck for edge cases.".to_string(),
                multiline: true,
            })
        );
    }

    #[test]
    fn parse_multiline_dash_prompt() {
        let body = "---\nReview the changes.\nThen run cargo test.\n---\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "Review the changes.\nThen run cargo test.".to_string(),
                multiline: true,
            })
        );
    }

    #[test]
    fn parse_mixed_syntax() {
        let body = "- do #fix1\n---\nReview changes.\n---\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert!(matches!(&entries[0], QueueEntry::Prompt(p) if !p.multiline));
        assert!(matches!(&entries[1], QueueEntry::Prompt(p) if p.multiline));
        assert!(matches!(&entries[2], QueueEntry::Prompt(p) if !p.multiline));
    }

    #[test]
    fn parse_normalizes_stray_leading_backtick_on_item() {
        // #queue-line-leading-backtick-drop: a queue item mistyped with a stray
        // leading backtick (`` `- text ``, the operator's code-span tick landing
        // before the bullet) must parse as a real prompt — not be silently kept
        // as inert Freeform and skipped — and re-render canonically as `- text`.
        let body = "`- There is significant blocking with the sync pipeline.\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "There is significant blocking with the sync pipeline.".to_string(),
                multiline: false,
            })
        );
        assert!(matches!(&entries[1], QueueEntry::Prompt(p) if p.text == "run tests"));
        // Re-render strips the stray backtick (self-heal to canonical `- `).
        let rendered = render(&entries);
        assert!(
            rendered.contains("- There is significant blocking"),
            "{rendered}"
        );
        assert!(!rendered.contains("`-"), "{rendered}");
    }

    #[test]
    fn parse_completed_single_line_item() {
        // Legacy single-tilde input parses as Completed; render canonicalizes to
        // double-tilde markdown strikethrough (#queue-strike-on-complete).
        let body = "- ~do #fix1~\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert!(matches!(&entries[0], QueueEntry::Completed(p) if p.text == "do #fix1"));
        assert!(matches!(&entries[1], QueueEntry::Prompt(p) if p.text == "do #fix2"));
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(render(&entries), "- ~~do #fix1~~\n- do #fix2\n");
    }

    #[test]
    fn stop_fence_after_completed_residue_is_live_head() {
        let body = "- ~do #fix1~\n--- stop\n- do #fix2\n";
        let entries = parse(body).unwrap();

        assert!(has_stop_fence_at_head(&entries));
    }

    #[test]
    fn time_gate_after_completed_residue_is_live_head() {
        let body = "- ~do #fix1~\n--- start at 17:00 ET\n- do #fix2\n";
        let entries = parse(body).unwrap();

        assert_eq!(time_gate_at_head(&entries), Some("17:00 ET"));
    }

    #[test]
    fn parse_preset_directive() {
        let body = "preset spec-test-build-install-commit-push\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::Preset("spec-test-build-install-commit-push".to_string())
        );
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(render(&entries), body);
    }

    #[test]
    fn parse_dispatch_directive() {
        let body = "dispatch #spec-test-build-install-commit-push\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::Dispatch("#spec-test-build-install-commit-push".to_string())
        );
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(render(&entries), body);
    }

    #[test]
    fn parse_bare_slash_command_as_prompt() {
        let body = "\n  /clear  \n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert!(matches!(&entries[0], QueueEntry::Prompt(p) if p.text == "/clear"));
        assert_eq!(first_prompt(&entries).unwrap().text, "/clear");
        assert_eq!(render(&entries), "/clear\n");
    }

    #[test]
    fn parse_bare_non_slash_line_remains_freeform() {
        let body = "clear the context\n";
        let entries = parse(body).unwrap();
        assert_eq!(
            entries,
            vec![QueueEntry::Freeform("clear the context".to_string())]
        );
        assert!(first_prompt(&entries).is_none());
        assert_eq!(render(&entries), body);
    }

    #[test]
    fn parse_empty_queue() {
        let body = "";
        let entries = parse(body).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_blank_lines_only() {
        let body = "\n\n\n";
        let entries = parse(body).unwrap();
        assert!(entries.is_empty());
    }

    #[test]
    fn parse_start_fence_bare() {
        let body = "--- start\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], QueueEntry::StartFence(None));
    }

    #[test]
    fn parse_start_fence_with_time() {
        let body = "--- start at 17:00 ET\n- run nightly tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_start_fence_without_at() {
        let body = "--- start 17:00 ET\n- run tests\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_tilde_start_fence() {
        let body = "~~~start\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0], QueueEntry::StartFence(None));
    }

    #[test]
    fn parse_stop_fence() {
        let body = "- do #fix1\n--- stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1], QueueEntry::StopFence);
    }

    #[test]
    fn parse_tilde_stop_fence() {
        let body = "- do #fix1\n~~~stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[1], QueueEntry::StopFence);
    }

    #[test]
    fn parse_multiple_time_gates() {
        let body =
            "- do #fix1\n--- start 17:00 ET\n- run nightly\n--- start 18:00 ET\n- coverage\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 5);
        assert_eq!(
            entries[1],
            QueueEntry::StartFence(Some("17:00 ET".to_string()))
        );
        assert_eq!(
            entries[3],
            QueueEntry::StartFence(Some("18:00 ET".to_string()))
        );
    }

    #[test]
    fn parse_auto_attribute() {
        let mut attrs = std::collections::HashMap::new();
        attrs.insert("auto".to_string(), String::new());
        assert!(has_auto_attr(&attrs));

        let empty = std::collections::HashMap::new();
        assert!(!has_auto_attr(&empty));
    }

    #[test]
    fn parse_preserves_unexpected_content_as_freeform() {
        // Previously this bailed; now unrecognized lines are preserved as
        // non-actionable Freeform so a polluted queue cannot brick the
        // consume/resume/dispatch guards that parse it.
        let body = "random text that is not a list item or fence\n";
        let entries = parse(body).expect("unexpected content is tolerated as Freeform");
        assert_eq!(entries.len(), 1);
        assert!(matches!(entries[0], QueueEntry::Freeform(_)));
        assert!(prompts(&entries).is_empty());
    }

    #[test]
    fn unclosed_prompt_fence_opener_preserved_as_freeform() {
        // Previously bailed; now an unclosed fence opener is preserved as a
        // Freeform separator so a polluted queue cannot brick the parse.
        let body = "~~~prompt\nSome content without closing fence\n";
        let entries = parse(body).expect("unclosed fence is tolerated");
        assert!(prompts(&entries).is_empty());
        assert!(entries.iter().any(|e| matches!(e, QueueEntry::Freeform(_))));
    }

    #[test]
    fn unclosed_dash_fence_opener_preserved_as_freeform() {
        let body = "---\nContent without closing dashes\n";
        let entries = parse(body).expect("unclosed --- fence is tolerated");
        assert!(prompts(&entries).is_empty());
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, QueueEntry::Freeform(s) if s.trim() == "---"))
        );
    }

    #[test]
    fn render_single_line() {
        let entries = vec![QueueEntry::Prompt(QueuePrompt {
            text: "do #fix1".to_string(),
            multiline: false,
        })];
        assert_eq!(render(&entries), "- do #fix1\n");
    }

    #[test]
    fn render_multiline() {
        let entries = vec![QueueEntry::Prompt(QueuePrompt {
            text: "Review changes.\nRun tests.".to_string(),
            multiline: true,
        })];
        assert_eq!(render(&entries), "---\nReview changes.\nRun tests.\n---\n");
    }

    #[test]
    fn render_start_fence() {
        let entries = vec![QueueEntry::StartFence(None)];
        assert_eq!(render(&entries), "--- start\n");
    }

    #[test]
    fn render_start_fence_with_time() {
        let entries = vec![QueueEntry::StartFence(Some("17:00 ET".to_string()))];
        assert_eq!(render(&entries), "--- start at 17:00 ET\n");
    }

    #[test]
    fn render_stop_fence() {
        let entries = vec![QueueEntry::StopFence];
        assert_eq!(render(&entries), "--- stop\n");
    }

    #[test]
    fn render_mixed() {
        let entries = vec![
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            }),
            QueueEntry::StopFence,
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            }),
        ];
        assert_eq!(render(&entries), "- do #fix1\n--- stop\n- do #fix2\n");
    }

    #[test]
    fn mark_first_prompt_completed_preserves_later_prompts() {
        let entries = vec![
            QueueEntry::Preset("spec".to_string()),
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            }),
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            }),
        ];
        let result = mark_first_prompt_completed(&entries);
        assert_eq!(render(&result), "preset spec\n- ~~do #fix1~~\n- do #fix2\n");
        assert_eq!(prompts(&result).len(), 1);
    }

    #[test]
    fn mark_first_prompt_completed_preserves_dispatch_directive() {
        let entries = vec![
            QueueEntry::Dispatch("#spec".to_string()),
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            }),
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix2".to_string(),
                multiline: false,
            }),
        ];
        let result = mark_first_prompt_completed(&entries);
        assert_eq!(
            render(&result),
            "dispatch #spec\n- ~~do #fix1~~\n- do #fix2\n"
        );
        assert_eq!(prompts(&result).len(), 1);
    }

    #[test]
    fn roundtrip_preserves_structure() {
        let body = "- do #fix1\n--- start at 17:00 ET\n- run nightly\n--- stop\n- do #fix2\n";
        let entries = parse(body).unwrap();
        let rendered = render(&entries);
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(entries, reparsed);
    }

    #[test]
    fn prompts_filters_to_prompt_entries() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
            QueueEntry::StopFence,
            QueueEntry::Prompt(QueuePrompt {
                text: "task2".to_string(),
                multiline: false,
            }),
        ];
        let p = prompts(&entries);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].text, "task1");
        assert_eq!(p[1].text, "task2");
    }

    #[test]
    fn first_prompt_skips_control_fences() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
        ];
        assert_eq!(first_prompt(&entries).unwrap().text, "task1");
    }

    #[test]
    fn first_prompt_none_when_empty() {
        let entries: Vec<QueueEntry> = vec![];
        assert!(first_prompt(&entries).is_none());
    }

    #[test]
    fn remove_first_prompt_preserves_control_fences() {
        let entries = vec![
            QueueEntry::StartFence(None),
            QueueEntry::Prompt(QueuePrompt {
                text: "task1".to_string(),
                multiline: false,
            }),
            QueueEntry::Prompt(QueuePrompt {
                text: "task2".to_string(),
                multiline: false,
            }),
        ];
        let result = remove_first_prompt(&entries);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], QueueEntry::StartFence(None));
        assert!(matches!(&result[1], QueueEntry::Prompt(p) if p.text == "task2"));
    }

    #[test]
    fn empty_multiline_fence_is_skipped() {
        let body = "---\n\n---\n- do #fix1\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(
            entries[0],
            QueueEntry::Prompt(QueuePrompt {
                text: "do #fix1".to_string(),
                multiline: false,
            })
        );
    }

    #[test]
    fn blank_lines_between_items_ignored() {
        let body = "- do #fix1\n\n\n- do #fix2\n";
        let entries = parse(body).unwrap();
        assert_eq!(entries.len(), 2);
    }

    // --- Activation resolution tests ---

    fn make_prompt(text: &str) -> QueueEntry {
        QueueEntry::Prompt(QueuePrompt {
            text: text.to_string(),
            multiline: false,
        })
    }

    // `#completed-queue-residue-regression` / `#queue-auto-no-continue`: a new
    // item inserted/reordered ahead of the still-present in-flight head is a
    // re-prioritization, not an in-place head edit, so it must NOT register as
    // `item_modified` (which would halt + strand the whole queue as residue).
    #[test]
    fn head_prompt_modified_false_when_new_item_inserted_ahead_of_present_head() {
        let snapshot = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        let file = vec![
            make_prompt("do [#ccc]"),
            make_prompt("do [#bbb]"),
            make_prompt("do [#ddd]"),
        ];
        assert!(!detect_head_prompt_modified(&snapshot, &file));
    }

    #[test]
    fn head_prompt_modified_false_on_reorder_promoting_existing_item() {
        // Operator promoted #ddd above #bbb; #bbb is still present → reprioritize.
        let snapshot = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        let file = vec![make_prompt("do [#ddd]"), make_prompt("do [#bbb]")];
        assert!(!detect_head_prompt_modified(&snapshot, &file));
    }

    #[test]
    fn head_prompt_modified_true_when_head_text_edited_in_place() {
        // The snapshot head text is gone from the queue (edited in place) → halt.
        let snapshot = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        let file = vec![
            make_prompt("do [#bbb] with extra operator notes"),
            make_prompt("do [#ddd]"),
        ];
        assert!(detect_head_prompt_modified(&snapshot, &file));
    }

    #[test]
    fn head_prompt_modified_false_when_head_unchanged() {
        let snapshot = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        let file = vec![make_prompt("do [#bbb]"), make_prompt("do [#ddd]")];
        assert!(!detect_head_prompt_modified(&snapshot, &file));
    }

    #[test]
    fn activation_auto_with_prompts() {
        let entries = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let act = resolve_activation(&entries, true, false, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::Auto));
        assert!(!act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 2);
    }

    #[test]
    fn activation_auto_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, true, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
    }

    #[test]
    fn activation_start_fence_bare() {
        let entries = vec![QueueEntry::StartFence(None), make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::StartFence));
        assert!(act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 1);
        assert!(matches!(&act.entries_after[0], QueueEntry::Prompt(p) if p.text == "do #fix1"));
    }

    #[test]
    fn activation_start_fence_bare_no_prompts_after() {
        let entries = vec![QueueEntry::StartFence(None)];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
        assert!(act.consumed_start_fence);
        assert!(act.entries_after.is_empty());
    }

    #[test]
    fn activation_start_fence_with_time_defers() {
        let entries = vec![
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
            make_prompt("run nightly"),
        ];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.deferred);
        assert_eq!(act.start_at, Some("17:00 ET".to_string()));
        assert!(!act.consumed_start_fence);
        assert_eq!(act.entries_after.len(), 2);
    }

    #[test]
    fn activation_exchange_trigger() {
        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, true, false);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::ExchangeRequest));
    }

    #[test]
    fn activation_exchange_trigger_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, false, true, false);
        assert!(!act.active);
    }

    #[test]
    fn activation_persisted_active() {
        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, true);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::Persisted));
    }

    #[test]
    fn activation_persisted_empty_queue() {
        let entries: Vec<QueueEntry> = vec![];
        let act = resolve_activation(&entries, false, false, true);
        assert!(!act.active);
    }

    #[test]
    fn activation_canonical_queue_start_drives_persisted_active() {
        // End-to-end (#queue-state-unify): `queue: start` in frontmatter folds
        // onto `queue_active`, which feeds `resolve_activation` as
        // `persisted_active`, activating the queue with the Persisted trigger
        // (auto-loop-continuation eligible).
        let (fm, _) = agent_doc_core::frontmatter::parse(
            "---\nagent_doc_format: template\nqueue: start\n---\n\n",
        )
        .unwrap();
        let persisted_active = fm.queue_active.unwrap_or(false);
        assert!(persisted_active, "queue: start must set queue_active");

        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, persisted_active);
        assert!(act.active);
        assert_eq!(act.trigger, Some(QueueTrigger::Persisted));
    }

    #[test]
    fn marker_control_detects_start_go_stop() {
        use agent_doc_core::frontmatter::QueueControl;
        let mut attrs = std::collections::HashMap::new();
        assert_eq!(marker_control(&attrs), None);
        attrs.insert("go".to_string(), String::new());
        assert_eq!(marker_control(&attrs), Some(QueueControl::Start));
        attrs.clear();
        attrs.insert("start".to_string(), String::new());
        assert_eq!(marker_control(&attrs), Some(QueueControl::Start));
        attrs.clear();
        attrs.insert("stop".to_string(), String::new());
        assert_eq!(marker_control(&attrs), Some(QueueControl::Stop));
        // stop wins over start/go if both are present.
        attrs.insert("go".to_string(), String::new());
        assert_eq!(marker_control(&attrs), Some(QueueControl::Stop));
    }

    #[test]
    fn strip_control_from_tag_removes_control_tokens() {
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue preset=\"#p\" go -->"),
            "<!-- agent:queue preset=\"#p\" -->"
        );
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue preset=\"#p\"=true go=true -->"),
            "<!-- agent:queue preset=\"#p\" -->"
        );
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue start -->"),
            "<!-- agent:queue -->"
        );
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue stop -->"),
            "<!-- agent:queue -->"
        );
        // No control token → unchanged.
        assert_eq!(
            strip_control_from_tag("<!-- agent:queue auto -->"),
            "<!-- agent:queue auto -->"
        );
    }

    #[test]
    fn activation_canonical_queue_stop_deactivates() {
        let (fm, _) =
            agent_doc_core::frontmatter::parse("---\nqueue: stop\nqueue_active: true\n---\n\n")
                .unwrap();
        // Canonical `queue: stop` wins over a stale `queue_active: true`.
        assert_eq!(fm.queue_active, Some(false));
        let entries = vec![make_prompt("do #fix1")];
        let act = resolve_activation(&entries, false, false, fm.queue_active.unwrap_or(false));
        assert!(!act.active);
    }

    #[test]
    fn activation_auto_takes_precedence_over_exchange() {
        let entries = vec![make_prompt("task")];
        let act = resolve_activation(&entries, true, true, false);
        assert_eq!(act.trigger, Some(QueueTrigger::Auto));
    }

    #[test]
    fn activation_start_fence_takes_precedence_over_exchange() {
        let entries = vec![QueueEntry::StartFence(None), make_prompt("task")];
        let act = resolve_activation(&entries, false, true, false);
        assert_eq!(act.trigger, Some(QueueTrigger::StartFence));
        assert!(act.consumed_start_fence);
    }

    #[test]
    fn activation_none_when_no_triggers() {
        let entries = vec![make_prompt("task")];
        let act = resolve_activation(&entries, false, false, false);
        assert!(!act.active);
        assert!(act.trigger.is_none());
    }

    #[test]
    fn strip_auto_from_tag_removes_auto() {
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue auto -->"),
            "<!-- agent:queue -->"
        );
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue auto patch=append -->"),
            "<!-- agent:queue patch=append -->"
        );
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue auto=true priority=true -->"),
            "<!-- agent:queue priority -->"
        );
    }

    #[test]
    fn strip_auto_from_tag_noop_without_auto() {
        assert_eq!(
            strip_auto_from_tag("<!-- agent:queue -->"),
            "<!-- agent:queue -->"
        );
    }

    #[test]
    fn normalize_queue_tag_attrs_repairs_boolean_true_regression() {
        assert_eq!(
            normalize_queue_tag_attrs(
                "<!-- agent:queue priority=true preset=\"#spec-test-build-install-commit-push\"=true go=true -->"
            ),
            "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->"
        );
    }

    // --- Phase 3: halt detection tests ---

    #[test]
    fn detect_head_modified_same_prompt() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_different_prompt() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1 EDITED"), make_prompt("do #fix2")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_prompt_removed() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix2")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_both_empty() {
        let snap: Vec<QueueEntry> = vec![];
        let file: Vec<QueueEntry> = vec![];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_snap_empty_file_has() {
        let snap: Vec<QueueEntry> = vec![];
        let file = vec![make_prompt("new item")];
        assert!(detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_ignores_later_changes() {
        let snap = vec![make_prompt("do #fix1"), make_prompt("do #fix2")];
        let file = vec![make_prompt("do #fix1"), make_prompt("do #fix2 EDITED")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn detect_head_modified_skips_control_fences() {
        let snap = vec![QueueEntry::StopFence, make_prompt("task")];
        let file = vec![QueueEntry::StopFence, make_prompt("task")];
        assert!(!detect_head_prompt_modified(&snap, &file));
    }

    #[test]
    fn stop_fence_at_head_detected() {
        let entries = vec![QueueEntry::StopFence, make_prompt("task")];
        assert!(has_stop_fence_at_head(&entries));
    }

    #[test]
    fn stop_fence_not_at_head() {
        let entries = vec![make_prompt("task"), QueueEntry::StopFence];
        assert!(!has_stop_fence_at_head(&entries));
    }

    #[test]
    fn stop_fence_empty_entries() {
        let entries: Vec<QueueEntry> = vec![];
        assert!(!has_stop_fence_at_head(&entries));
    }

    #[test]
    fn time_gate_at_head_detected() {
        let entries = vec![
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
            make_prompt("task"),
        ];
        assert_eq!(time_gate_at_head(&entries), Some("17:00 ET"));
    }

    #[test]
    fn time_gate_at_head_bare_start_not_time_gate() {
        let entries = vec![QueueEntry::StartFence(None), make_prompt("task")];
        assert_eq!(time_gate_at_head(&entries), None);
    }

    #[test]
    fn time_gate_at_head_prompt_first() {
        let entries = vec![
            make_prompt("task"),
            QueueEntry::StartFence(Some("17:00 ET".to_string())),
        ];
        assert_eq!(time_gate_at_head(&entries), None);
    }

    #[test]
    fn parse_preserves_unrecognized_freetext_as_freeform_instead_of_failing() {
        // A queue polluted with prose (the contamination class) must not fail
        // every consume/resume guard that parses the queue.
        let body = "JB `Run Agent Doc` error:\n- do [#existing]\nThe response should contain the prompt.\n";
        let entries = parse(body).expect("polluted queue must parse, not bail");
        // The free-text lines are preserved as Freeform.
        assert_eq!(
            entries
                .iter()
                .filter(|e| matches!(e, QueueEntry::Freeform(_)))
                .count(),
            2
        );
        // The real prompt item is still recognized and actionable.
        let prompts = prompts(&entries);
        assert_eq!(prompts.len(), 1);
        assert_eq!(prompts[0].text, "do [#existing]");
    }

    #[test]
    fn freeform_round_trips_through_render() {
        let body = "stray prose line\n- do [#real]\n";
        let rendered = render(&parse(body).unwrap());
        assert!(rendered.contains("stray prose line"));
        assert!(rendered.contains("- do [#real]"));
        // Re-parsing the rendered output is stable.
        let reparsed = parse(&rendered).unwrap();
        assert_eq!(prompts(&reparsed).len(), 1);
        assert_eq!(
            reparsed
                .iter()
                .filter(|e| matches!(e, QueueEntry::Freeform(_)))
                .count(),
            1
        );
    }

    #[test]
    fn freeform_is_not_an_actionable_prompt() {
        let entries = vec![
            QueueEntry::Freeform("just a note".to_string()),
            QueueEntry::Freeform("another note".to_string()),
        ];
        assert!(prompts(&entries).is_empty());
        assert!(first_prompt(&entries).is_none());
    }

    #[test]
    fn unclosed_bare_fence_does_not_swallow_real_items_beneath_it() {
        // The exact live-corruption shape: a stray `---` separator with no
        // matching close, followed by the real preset + queue items. The `---`
        // must be preserved as Freeform and the items must still parse as
        // actionable prompts (not swallowed into an unclosed fence).
        let body = "---\npreset #spec-test\n- do [#first]\n- do [#second]\n";
        let entries = parse(body).expect("unbalanced fences must not fail the parse");
        let prompts = prompts(&entries);
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].text, "do [#first]");
        assert_eq!(prompts[1].text, "do [#second]");
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, QueueEntry::Preset(p) if p == "#spec-test"))
        );
        assert!(
            entries
                .iter()
                .any(|e| matches!(e, QueueEntry::Freeform(s) if s.trim() == "---")),
            "the stray --- separator is preserved as Freeform"
        );
    }

    #[test]
    fn unclosed_prompt_fence_is_preserved_as_freeform_separator() {
        // A stray ``` opener with no closer is preserved, and the `- do` item
        // beneath it still parses.
        let body = "```\n- do [#real]\n";
        let entries = parse(body).expect("unclosed prompt fence must not fail the parse");
        assert_eq!(prompts(&entries).len(), 1);
        assert_eq!(prompts(&entries)[0].text, "do [#real]");
    }
