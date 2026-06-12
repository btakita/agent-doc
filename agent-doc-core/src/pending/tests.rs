    use super::*;

    const DOC_ID: &str = "test-doc";

    fn ids() -> HashSet<String> {
        HashSet::new()
    }

    #[test]
    fn active_item_ids_returns_open_items_in_order() {
        let body = concat!(
            "- [ ] [#first] one\n",
            "- [/] [#gated] blocked\n",
            "- [x] [#done] finished\n",
            "- [ ] [#second] two\n",
        );
        assert_eq!(active_item_ids(body), vec!["first", "second"]);
    }

    #[test]
    fn active_item_ids_empty_for_empty_body() {
        assert!(active_item_ids("").is_empty());
    }

    #[test]
    fn active_enqueue_item_ids_returns_marked_open_items() {
        let body = concat!(
            "- [ ] [#inbox] :inbox_tray: one\n",
            "- [/] [#gated] :inbox_tray: blocked\n",
            "- [x] [#done] **enqueue** finished\n",
            "- [ ] [#bold] **enqueue** two\n",
            "- [ ] [#slash] /enqueue three\n",
            "- [ ] [#plain] enqueue should not be a marker\n",
            "- [ ] untracked :inbox_tray: no id\n",
        );
        assert_eq!(
            active_enqueue_item_ids(body),
            vec!["inbox", "bold", "slash"]
        );
    }

    #[test]
    fn item_priority_rank_parses_token() {
        assert_eq!(item_priority_rank("priority=1 do the thing"), 1);
        assert_eq!(item_priority_rank("text priority=5 more"), 5);
        assert_eq!(item_priority_rank("no token here"), PRIORITY_RANK_UNSET);
        assert_eq!(
            item_priority_rank("priority=0 out of range"),
            PRIORITY_RANK_UNSET
        );
        assert_eq!(
            item_priority_rank("priority=12 out of range"),
            PRIORITY_RANK_UNSET
        );
    }

    #[test]
    fn sort_by_priority_orders_ascending_stable() {
        let body = concat!(
            "- [ ] [#a] priority=3 third\n",
            "- [ ] [#b] no priority\n",
            "- [ ] [#c] priority=1 first\n",
            "- [ ] [#d] priority=1 also first\n",
        );
        let sorted = sort_by_priority(body).expect("order should change");
        // priority=1 items first (stable: c before d), then priority=3, then unset.
        let ids: Vec<&str> = sorted
            .lines()
            .filter_map(|l| l.split("[#").nth(1))
            .filter_map(|s| s.split(']').next())
            .collect();
        assert_eq!(ids, vec!["c", "d", "a", "b"]);
    }

    #[test]
    fn sort_by_priority_idempotent_when_ordered() {
        let body = "- [ ] [#a] priority=1 x\n- [ ] [#b] priority=2 y\n";
        assert!(sort_by_priority(body).is_none());
    }

    #[test]
    fn active_item_priorities_pairs_open_ids_with_rank() {
        let body = concat!(
            "- [ ] [#a] priority=2 one\n",
            "- [/] [#g] priority=1 gated\n",
            "- [ ] [#b] two\n",
        );
        assert_eq!(
            active_item_priorities(body),
            vec![
                ("a".to_string(), 2u8),
                ("b".to_string(), PRIORITY_RANK_UNSET)
            ]
        );
    }

    #[test]
    fn op_take_active_items_by_ids_removes_open_and_gated_matches_only() {
        let body = concat!(
            "intro\n",
            "- [ ] [#open] remove open\n",
            "  child line\n",
            "- [/] [#gated] remove gated\n",
            "- [x] [#done] keep explicit done\n",
            "- [ ] [#keep] keep open\n",
        );
        let ids: HashSet<String> = ["#open".to_string(), "gated".to_string(), "done".to_string()]
            .into_iter()
            .collect();

        let (new_body, removed) = op_take_active_items_by_ids(body, &ids);

        assert_eq!(
            removed
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["open", "gated"]
        );
        assert!(!new_body.contains("[#open]"));
        assert!(!new_body.contains("child line"));
        assert!(!new_body.contains("[#gated]"));
        assert!(new_body.contains("[#done] keep explicit done"));
        assert!(new_body.contains("[#keep] keep open"));
        assert!(new_body.starts_with("intro\n"));
    }

    #[test]
    fn parse_empty_body() {
        let (p, items, post) = parse_items("");
        assert_eq!(p, "");
        assert!(items.is_empty());
        assert_eq!(post, "");
    }

    #[test]
    fn parse_fully_migrated() {
        let body = "- [ ] [#a3f2] first\n- [x] [#b1c4] second\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, PendingListMarker::Bullet);
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].text, "first");
        assert_eq!(items[1].id, "b1c4");
        assert_eq!(items[1].state, PendingState::Done);
    }

    #[test]
    fn detects_malformed_tracked_checklist_lines() {
        let body = concat!(
            "_- [ ] [#pcops] damaged prefix\n",
            "- [ ] [#keep1] valid item\n",
            "plain note mentioning [#note1]\n",
        );

        let malformed = detect_malformed_item_lines(body);

        assert_eq!(malformed.len(), 1);
        assert_eq!(malformed[0].id, "pcops");
        assert_eq!(malformed[0].line, 1);
        assert!(malformed[0].text.contains("damaged prefix"));
    }

    #[test]
    fn parse_hyphenated_id() {
        let body = "- [ ] [#tmuxcrash-abcd] child task\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "tmuxcrash-abcd");
        assert_eq!(items[0].text, "child task");
    }

    #[test]
    fn parse_ordered_items() {
        let body = "1. [ ] [#a3f2] first\n2. [x] [#b1c4] second\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, PendingListMarker::Ordered(1));
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[1].marker, PendingListMarker::Ordered(2));
        assert_eq!(items[1].state, PendingState::Done);
    }

    #[test]
    fn strips_redundant_self_id_tag_from_text() {
        // #pending-redundant-self-id-strip: a self-id repeated after a tag is dropped.
        let body = "- [ ] [#stale-x] [recommended] [#stale-x] Evaluate the retire path.\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "stale-x");
        assert_eq!(items[0].text, "[recommended] Evaluate the retire path.");
    }

    #[test]
    fn preserves_cross_reference_ids_in_text() {
        // Only the item's OWN id is stripped; references to other ids stay.
        let body = "- [ ] [#a] depends on [#b] and [#c] downstream.\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].id, "a");
        assert_eq!(items[0].text, "depends on [#b] and [#c] downstream.");
    }

    #[test]
    fn parse_gated_state() {
        let body = "- [/] [#eg0w] CommitLock — gate: v0.32.5\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].id, "eg0w");
        assert_eq!(items[0].text, "CommitLock — gate: v0.32.5");
    }

    #[test]
    fn parse_all_three_states() {
        let body = "- [ ] [#a3f2] open\n- [/] [#b1c4] gated\n- [x] [#c9e0] done\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[1].state, PendingState::Gated);
        assert_eq!(items[2].state, PendingState::Done);
    }

    #[test]
    fn parse_legacy_tilde_checkbox_preserves_hash_id() {
        let body = "- [~] [#q6js] in-progress legacy item\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].id, "q6js");
        assert_eq!(items[0].text, "in-progress legacy item");
    }

    #[test]
    fn parse_checkbox_only_no_id() {
        let body = "- [ ] just text\n- [x] done item\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "");
        assert_eq!(items[0].text, "just text");
        assert_eq!(items[1].state, PendingState::Done);
    }

    #[test]
    fn parse_legacy_no_checkbox() {
        let body = "- legacy one\n- legacy two\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].text, "legacy one");
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].id, "");
    }

    #[test]
    fn parse_mixed() {
        let body = "- [ ] [#a3f2] migrated\n- [ ] partial\n- legacy\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[1].id, "");
        assert_eq!(items[1].text, "partial");
        assert_eq!(items[2].text, "legacy");
    }

    #[test]
    fn parse_nested_lines_attach_to_parent_item() {
        let body = concat!(
            "- [ ] [#a3f2] parent task\n",
            "  - dependency one\n",
            "  - dependency two\n",
            "- [ ] [#b1c4] sibling task\n"
        );
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].id, "a3f2");
        assert_eq!(items[0].text, "parent task");
        assert_eq!(
            items[0].continuation,
            "  - dependency one\n  - dependency two\n"
        );
        assert_eq!(items[1].id, "b1c4");
        assert!(items[1].continuation.is_empty());
    }

    #[test]
    fn parse_nested_lines_attach_to_ordered_parent_item() {
        let body = concat!(
            "1. [ ] [#a3f2] parent task\n",
            "   1. dependency one\n",
            "   2. dependency two\n",
            "2. [ ] [#b1c4] sibling task\n"
        );
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 2);
        assert_eq!(items[0].marker, PendingListMarker::Ordered(1));
        assert_eq!(
            items[0].continuation,
            "   1. dependency one\n   2. dependency two\n"
        );
        assert_eq!(items[1].marker, PendingListMarker::Ordered(2));
    }

    #[test]
    fn render_roundtrip_canonical() {
        let body = "- [ ] [#a3f2] first\n- [x] [#b1c4] second\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn render_roundtrip_all_three_states() {
        let body = "- [ ] [#a3f2] open\n- [/] [#b1c4] gated — gate: v0.32.5\n- [x] [#c9e0] done\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn render_roundtrip_ordered_list() {
        let body = "1. [ ] [#a3f2] open\n2. [/] [#b1c4] gated\n3. [x] [#c9e0] done\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn render_emits_slash_for_gated() {
        let item = PendingItem {
            marker: PendingListMarker::Bullet,
            id: "eg0w".to_string(),
            state: PendingState::Gated,
            gate_type: None,
            text: "CommitLock".to_string(),
            continuation: String::new(),
        };
        assert_eq!(item.render(), "- [/] [#eg0w] CommitLock");
    }

    #[test]
    fn backfill_adds_hashes() {
        let body = "- legacy one\n- legacy two\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 2);
        assert!(!items[0].id.is_empty());
        assert!(!items[1].id.is_empty());
        assert_ne!(items[0].id, items[1].id);
        assert!(new_body.contains("- [ ] [#"));
    }

    #[test]
    fn backfill_idempotent() {
        let body = "- [ ] [#a3f2] first\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed, "fully-migrated body should not change");
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_normalizes_checkbox_only() {
        let body = "- [ ] no id here\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("[#"));
    }

    #[test]
    fn backfill_drops_content_less_empty_bullet() {
        // #icebox-empty-item-phantom-id: a stray empty `- [ ]` must NOT be
        // assigned a phantom hash id; it carries no description and is dropped.
        let body = "- [ ]\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(
            !new_body.contains("[#"),
            "empty bullet must not get a phantom id: {new_body:?}"
        );
        let (_, items, _) = parse_items(&new_body);
        assert!(
            items.is_empty(),
            "empty bullet should be dropped: {items:?}"
        );
    }

    #[test]
    fn backfill_drops_id_only_empty_item_self_heal() {
        // Already-cemented phantom (`- [ ] [#1k5y]` with no description) is
        // removed on the next backfill so the bug self-heals.
        let body = "- [ ] [#existing] real item\n- [ ] [#1k5y]\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("[#existing] real item"));
        assert!(
            !new_body.contains("[#1k5y]"),
            "phantom id-only item must be dropped: {new_body:?}"
        );
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 1);
    }

    #[test]
    fn backfill_keeps_empty_text_with_continuation() {
        // Guard against over-dropping: an item with empty header text but a real
        // indented continuation still carries content and must be preserved.
        let body = "- [ ] [#p1]\n  - detail line\n";
        let (new_body, _changed) = backfill(body, DOC_ID, &ids());
        assert!(
            new_body.contains("[#p1]"),
            "item with continuation must survive: {new_body:?}"
        );
        assert!(new_body.contains("detail line"));
    }

    #[test]
    fn backfill_never_inserts_gated() {
        // Legacy items with no checkbox must default to Open `[ ]`,
        // never Gated `[/]`. Gated state is always operator-explicit.
        let body = "- legacy item awaiting v0.32.5\n";
        let (new_body, _) = backfill(body, DOC_ID, &ids());
        assert!(new_body.contains("- [ ] "));
        assert!(!new_body.contains("- [/] "));
    }

    #[test]
    fn backfill_preserves_existing_gated() {
        let body = "- [/] [#eg0w] CommitLock — gate: v0.32.5\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed);
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_preserves_id_behind_legacy_tilde_checkbox() {
        let body = "- [~] [#q6js] pcpc5e3 08b cut-over proof\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert_eq!(new_body, "- [ ] [#q6js] pcpc5e3 08b cut-over proof\n");
        assert_eq!(new_body.matches("[#").count(), 1);

        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[0].id, "q6js");
        assert_eq!(items[0].text, "pcpc5e3 08b cut-over proof");
    }

    #[test]
    fn backfill_preserves_interleaved_headers_and_blank_lines() {
        let body = concat!(
            "### Active\n",
            "- legacy one\n",
            "\n",
            "### Later\n",
            "- [ ] [#keep1] keep section\n"
        );
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("### Active"));
        assert!(new_body.contains("\n\n### Later\n"));
        assert!(new_body.contains("[#keep1] keep section"));
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines[0], "### Active");
        assert!(lines[1].starts_with("- [ ] [#"));
        assert_eq!(lines[3], "### Later");
    }

    #[test]
    fn backfill_assigns_nested_subtask_ids_prefixed_by_parent() {
        let body = concat!(
            "- parent task\n",
            "  - child dependency\n",
            "  - child subtask\n",
            "- sibling task\n"
        );
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert_eq!(new_body.matches("[#").count(), 4, "got: {new_body}");
        assert!(new_body.contains("  - [ ] [#"));
        let lines: Vec<&str> = new_body.lines().collect();
        let parent_line = lines[0];
        let parent_id = parent_line
            .split("[#")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("parent id");
        assert!(
            lines[1].contains(&format!("[#{}-", parent_id)),
            "expected first child id prefixed by parent id, got: {}",
            lines[1]
        );
        assert!(
            lines[2].contains(&format!("[#{}-", parent_id)),
            "expected second child id prefixed by parent id, got: {}",
            lines[2]
        );
    }

    #[test]
    fn backfill_preserves_existing_nested_subtask_ids() {
        let body = concat!(
            "- [ ] [#tmuxcrash] parent task\n",
            "  - [ ] [#tmuxcrash-abcd] child dependency\n"
        );
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed);
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_reassigns_duplicate_existing_nested_subtask_ids() {
        let body = concat!(
            "- [ ] [#tmuxcrash] parent task\n",
            "  - [ ] [#tmuxcrash-abcd] child dependency\n",
            "  - [ ] [#tmuxcrash-abcd] child subtask\n"
        );
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines.len(), 3, "got: {new_body}");
        assert!(lines[1].contains("[#tmuxcrash-abcd]"));
        let second_child_id = lines[2]
            .split("[#")
            .nth(1)
            .and_then(|rest| rest.split(']').next())
            .expect("second child id");
        assert_ne!(second_child_id, "tmuxcrash-abcd");
        assert!(second_child_id.starts_with("tmuxcrash-"));
    }

    #[test]
    fn backfill_preserves_ordered_parent_items() {
        let body = concat!("1. legacy one\n", "2. [ ] [#keep1] keep section\n");
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.starts_with("1. [ ] [#"));
        assert!(new_body.contains("2. [ ] [#keep1] keep section"));
    }

    #[test]
    fn reap_skips_gated() {
        let body = "- [/] [#eg0w] gated\n- [x] [#c9e0] done\n";
        let (new_body, removed) = reap(body).unwrap();
        assert_eq!(removed, vec!["c9e0"]);
        assert!(new_body.contains("[#eg0w]"));
        assert!(!new_body.contains("[#c9e0]"));
    }

    #[test]
    fn reap_removes_checked() {
        let body = "- [ ] [#a3f2] keep\n- [x] [#b1c4] drop\n- [ ] [#c5d6] keep2\n";
        let (new_body, removed) = reap(body).unwrap();
        assert_eq!(removed, vec!["b1c4"]);
        assert!(new_body.contains("a3f2"));
        assert!(!new_body.contains("b1c4"));
        assert!(new_body.contains("c5d6"));
    }

    #[test]
    fn reap_removes_flush_left_spill_with_completed_parent() {
        let body = concat!(
            "- [x] [#b1c4] drop\n",
            "Commands:\n",
            "  cargo test -p agent-doc pending::\n",
            "Diff:\n",
            "@@ -1 +1 @@\n",
            "- [ ] [#c5d6] keep\n"
        );
        let (new_body, removed) = reap(body).unwrap();
        assert_eq!(removed, vec!["b1c4"]);
        assert!(!new_body.contains("[#b1c4]"));
        assert!(!new_body.contains("Commands:"));
        assert!(!new_body.contains("@@ -1 +1 @@"));
        assert!(new_body.contains("- [ ] [#c5d6] keep"));
    }

    #[test]
    fn reap_preserves_following_heading_text() {
        let body = concat!(
            "- [x] [#b1c4] drop\n",
            "\n",
            "## Next Group\n",
            "- [ ] [#c5d6] keep\n"
        );
        let (new_body, removed) = reap(body).unwrap();
        assert_eq!(removed, vec!["b1c4"]);
        assert!(!new_body.contains("[#b1c4]"));
        assert!(new_body.contains("## Next Group"));
        assert!(new_body.contains("- [ ] [#c5d6] keep"));
    }

    #[test]
    fn reap_noop_when_none_checked() {
        let body = "- [ ] [#a3f2] keep\n";
        let (new_body, removed) = reap(body).unwrap();
        assert!(removed.is_empty());
        assert_eq!(new_body, body);
    }

    #[test]
    fn reap_errors_when_completed_item_is_missing_id() {
        let body = "- [x] legacy done without id\n- [ ] [#keep1] keep\n";
        let err = reap(body).unwrap_err();
        assert!(
            err.to_string()
                .contains("pending reap requires ids for completed items"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn reap_with_items_archives_malformed_flush_left_spill_with_parent() {
        let body = concat!(
            "- [x] [#b1c4] drop\n",
            "Commands:\n",
            "  cargo test -p agent-doc pending::\n"
        );
        let (_new_body, removed) = reap_with_items(body).unwrap();
        assert_eq!(removed.len(), 1);
        assert_eq!(removed[0].id, "b1c4");
        assert_eq!(
            removed[0].continuation,
            "Commands:\n  cargo test -p agent-doc pending::\n"
        );
    }

    #[test]
    fn detect_reorder_same_set_different_order() {
        let snap = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        let cur = "- [ ] [#c3d4] two\n- [ ] [#a1b2] one\n";
        let result = detect_reorder(snap, cur);
        assert_eq!(result, Some(vec!["c3d4".to_string(), "a1b2".to_string()]));
    }

    #[test]
    fn detect_reorder_none_when_sets_differ() {
        let snap = "- [ ] [#a1b2] one\n";
        let cur = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        assert_eq!(detect_reorder(snap, cur), None);
    }

    #[test]
    fn detect_reorder_none_when_same_order() {
        let snap = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        assert_eq!(detect_reorder(snap, snap), None);
    }

    #[test]
    fn op_add_inserts_new_item_with_hash() {
        let body = "";
        let (new_body, id) = op_add(body, "first task", DOC_ID, false).unwrap();
        assert!(new_body.contains("- [ ] [#"));
        assert!(new_body.contains("first task"));
        assert!(!id.is_empty());
    }

    #[test]
    fn op_add_prepends_before_existing_items() {
        let body = "- [ ] [#a1b2] existing task\n- [ ] [#c3d4] later task\n";
        let (new_body, _id) = op_add(body, "new first task", DOC_ID, false).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(
            lines[0].contains("new first task"),
            "expected new item first, got: {}",
            new_body
        );
        assert!(
            lines[1].contains("existing task"),
            "expected previous first item second, got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_at_after_inserts_immediately_after_anchor() {
        // #ah0s: --pending-add-after lands directly below the anchor.
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] third\n";
        let (new_body, _id) =
            op_add_at(body, "second", DOC_ID, false, AddPosition::After("a1b2")).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(lines[0].contains("first"), "{new_body}");
        assert!(lines[1].contains("second"), "{new_body}");
        assert!(lines[2].contains("third"), "{new_body}");
    }

    #[test]
    fn op_add_at_before_inserts_immediately_before_anchor() {
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] third\n";
        let (new_body, _id) =
            op_add_at(body, "zeroth", DOC_ID, false, AddPosition::Before("a1b2")).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(lines[0].contains("zeroth"), "{new_body}");
        assert!(lines[1].contains("first"), "{new_body}");
    }

    #[test]
    fn op_add_at_last_appends_at_tail() {
        // #ah0s: --pending-add-back lands at the tail without disturbing the head.
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] second\n";
        let (new_body, _id) =
            op_add_at(body, "tail item", DOC_ID, false, AddPosition::Last).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(lines[0].contains("first"), "{new_body}");
        assert!(lines[2].contains("tail item"), "{new_body}");
    }

    #[test]
    fn op_add_at_after_chains_build_deterministic_order() {
        // #ah0s: chaining after A then after B builds A→B→C with no reorder pass.
        let body = "- [ ] [#a1b2] A\n";
        let (b1, _) =
            op_add_at(body, "id=bbbb B", DOC_ID, false, AddPosition::After("a1b2")).unwrap();
        let (b2, _) = op_add_at(&b1, "C", DOC_ID, false, AddPosition::After("bbbb")).unwrap();
        let lines: Vec<&str> = b2.lines().collect();
        assert!(lines[0].contains(" A"), "{b2}");
        assert!(lines[1].contains(" B"), "{b2}");
        assert!(lines[2].contains(" C"), "{b2}");
    }

    #[test]
    fn op_add_at_unknown_anchor_errors() {
        let body = "- [ ] [#a1b2] first\n";
        let err = op_add_at(body, "x", DOC_ID, false, AddPosition::After("nope")).unwrap_err();
        assert!(err.to_string().contains("anchor id not found"), "{err}");
    }

    #[test]
    fn op_add_preserves_section_headers() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#a1b2] existing task\n",
            "### Later\n",
            "- [ ] [#c3d4] later task\n"
        );
        let (new_body, _id) = op_add(body, "new first task", DOC_ID, false).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines[0], "### Active");
        assert!(lines[1].contains("new first task"), "got: {}", new_body);
        assert!(lines[2].contains("existing task"), "got: {}", new_body);
        assert_eq!(lines[3], "### Later");
        assert!(lines[4].contains("later task"), "got: {}", new_body);
    }

    #[test]
    fn op_append_items_preserves_order_after_existing_items() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#a1b2] existing task\n",
            "\n",
            "### Notes\n",
            "Keep this note.\n",
        );
        let appended = vec![
            PendingItem {
                marker: PendingListMarker::Bullet,
                id: "b2c3".to_string(),
                state: PendingState::Open,
                gate_type: None,
                text: "first appended task".to_string(),
                continuation: String::new(),
            },
            PendingItem {
                marker: PendingListMarker::Bullet,
                id: "d4e5".to_string(),
                state: PendingState::Gated,
                gate_type: Some("release".to_string()),
                text: "second appended task".to_string(),
                continuation: String::new(),
            },
        ];

        let new_body = op_append_items(body, &appended);

        assert_eq!(
            new_body,
            concat!(
                "### Active\n",
                "- [ ] [#a1b2] existing task\n",
                "- [ ] [#b2c3] first appended task\n",
                "- [/release] [#d4e5] second appended task\n",
                "\n",
                "### Notes\n",
                "Keep this note.\n",
            )
        );
    }

    #[test]
    fn op_add_renumbers_ordered_lists() {
        let body = "1. [ ] [#a1b2] existing task\n2. [ ] [#c3d4] later task\n";
        let (new_body, _id) = op_add(body, "new first task", DOC_ID, false).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert!(lines[0].starts_with("1. "), "got: {}", new_body);
        assert!(lines[0].contains("new first task"), "got: {}", new_body);
        assert!(lines[1].starts_with("2. "), "got: {}", new_body);
        assert!(lines[1].contains("existing task"), "got: {}", new_body);
        assert!(lines[2].starts_with("3. "), "got: {}", new_body);
        assert!(lines[2].contains("later task"), "got: {}", new_body);
    }

    #[test]
    fn op_add_accepts_custom_id_prefix() {
        let (new_body, id) = op_add("", "id=ship1 release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_custom_id_prefix_with_hash_marker() {
        let (new_body, id) = op_add("", "id=#ship1 release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_bracketed_custom_id_prefix() {
        let (new_body, id) = op_add("", "[#ship1] release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "ship1");
        assert!(new_body.contains("- [ ] [#ship1] release checklist"));
    }

    #[test]
    fn op_add_accepts_long_bracketed_custom_id_prefix() {
        let (new_body, id) = op_add("", "[#sdig2matrix] release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "sdig2matrix");
        assert!(new_body.contains("- [ ] [#sdig2matrix] release checklist"));
    }

    #[test]
    fn op_add_accepts_hyphenated_custom_id_prefix() {
        let (new_body, id) =
            op_add("", "id=tmuxcrash-abcd release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "tmuxcrash-abcd");
        assert!(new_body.contains("- [ ] [#tmuxcrash-abcd] release checklist"));
    }

    #[test]
    fn op_add_accepts_hyphenated_bracketed_custom_id_prefix() {
        let (new_body, id) =
            op_add("", "[#tmuxcrash-abcd] release checklist", DOC_ID, false).unwrap();
        assert_eq!(id, "tmuxcrash-abcd");
        assert!(new_body.contains("- [ ] [#tmuxcrash-abcd] release checklist"));
    }

    #[test]
    fn op_add_rejects_bare_bracket_placeholder_prefix() {
        let err = op_add("", "[#] release checklist", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("bare `[#]` placeholder"));
    }

    #[test]
    fn op_add_rejects_empty_explicit_custom_id_prefix() {
        let err = op_add("", "id=  release checklist", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("empty custom id"));
    }

    #[test]
    fn op_add_rejects_duplicate_custom_id_prefix() {
        let body = "- [ ] [#ship1] existing task\n";
        let err = op_add(body, "id=ship1 new task", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("custom id already exists"));
    }

    #[test]
    fn op_add_rejects_missing_text_after_custom_id_prefix() {
        let err = op_add("", "id=ship1", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("custom id prefix must be followed by item text"));
    }

    #[test]
    fn op_add_rejects_missing_text_after_bracketed_custom_id_prefix() {
        let err = op_add("", "[#ship1]", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("bracketed custom id prefix must be followed by item text")
        );
    }

    #[test]
    fn op_add_rejects_stacked_bracketed_custom_id_prefixes() {
        let err = op_add("", "[#ship1] [#ship2] release checklist", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn op_add_rejects_stacked_mixed_custom_id_prefixes() {
        let err = op_add("", "id=ship1 [#ship2] release checklist", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            err
        );
    }

    #[test]
    fn op_add_rejects_empty() {
        assert!(op_add("", "   ", DOC_ID, false).is_err());
    }

    #[test]
    fn op_add_rejects_state_marker_prefix() {
        for marker in &["[ ] task", "[/] task", "[x] task", "[X] task"] {
            let err = op_add("", marker, DOC_ID, false).unwrap_err();
            let msg = format!("{}", err);
            assert!(
                msg.contains("state marker"),
                "expected state marker error for '{}', got: {}",
                marker,
                msg
            );
        }
    }

    #[test]
    fn op_add_rejects_duplicate_text() {
        let (body, _id1) = op_add("", "Wire Sift into corky", DOC_ID, false).unwrap();
        let err = op_add(&body, "Wire Sift into corky", DOC_ID, false).unwrap_err();
        let msg = format!("{}", err);
        assert!(
            msg.contains("duplicate"),
            "expected duplicate error, got: {}",
            msg
        );
    }

    #[test]
    fn parse_bare_hash_placeholder_strips_marker() {
        let item = parse_item_line("- [ ] [#] Wire Sift into corky").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.text, "Wire Sift into corky");
    }

    #[test]
    fn backfill_strips_bare_hash_placeholder() {
        let body = "- [ ] [#] task with placeholder\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        assert!(new_body.contains("[#"), "should have a hash id");
        // The bare [#] should be consumed — only one [# in the output
        let hash_count = new_body.matches("[#").count();
        assert_eq!(hash_count, 1, "expected exactly one [# in: {}", new_body);
        assert!(new_body.contains("task with placeholder"));
        assert!(
            !new_body.contains("[#] task"),
            "bare [#] should not survive in text"
        );
    }

    #[test]
    fn parse_bare_hash_placeholder_no_checkbox() {
        // `- [#] text` — no checkbox, bare placeholder
        let item = parse_item_line("- [#] no checkbox").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.state, PendingState::Open);
        assert_eq!(item.text, "no checkbox");
    }

    #[test]
    fn parse_bare_hash_placeholder_gated() {
        // `- [/] [#] gated task` — gated with bare placeholder
        let item = parse_item_line("- [/] [#] gated task").unwrap();
        assert_eq!(item.id, "");
        assert_eq!(item.state, PendingState::Gated);
        assert_eq!(item.text, "gated task");
    }

    #[test]
    fn backfill_strips_multiple_bare_placeholders() {
        // Multiple items each with bare [#] — all should get real IDs
        let body = "- [ ] [#] first task\n- [ ] [#] second task\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(changed);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items.len(), 2);
        assert!(!items[0].id.is_empty(), "first should have id");
        assert!(!items[1].id.is_empty(), "second should have id");
        assert_ne!(items[0].id, items[1].id, "ids should be unique");
        // No residual [#] in text
        assert!(
            !items[0].text.contains("[#]"),
            "first text has residual [#]: {}",
            items[0].text
        );
        assert!(
            !items[1].text.contains("[#]"),
            "second text has residual [#]: {}",
            items[1].text
        );
    }

    #[test]
    fn backfill_preserves_long_custom_id() {
        let body = "- [ ] [#sdig2matrix] Fixture evidence matrix\n";
        let (new_body, changed) = backfill(body, DOC_ID, &ids());
        assert!(!changed);
        assert_eq!(new_body, body);
    }

    #[test]
    fn backfill_idempotent_after_placeholder_strip() {
        // After stripping [#] and assigning ID, second backfill should be a no-op
        let body = "- [ ] [#] task\n";
        let (first_pass, _) = backfill(body, DOC_ID, &ids());
        let (second_pass, changed) = backfill(&first_pass, DOC_ID, &ids());
        assert!(
            !changed,
            "second backfill should be no-op, got: {}",
            second_pass
        );
        assert_eq!(first_pass, second_pass);
    }

    #[test]
    fn op_add_dedup_case_sensitive() {
        // Different casing should NOT be considered duplicate
        let (body, _) = op_add("", "Wire Sift", DOC_ID, false).unwrap();
        let result = op_add(&body, "wire sift", DOC_ID, false);
        assert!(result.is_ok(), "different case should not be duplicate");
    }

    #[test]
    fn op_add_dedup_across_states() {
        // Item exists as gated — adding same text as open should still dedup
        let (body, _) = op_add("", "deploy to prod", DOC_ID, true).unwrap();
        let err = op_add(&body, "deploy to prod", DOC_ID, false).unwrap_err();
        assert!(format!("{}", err).contains("duplicate"));
    }

    #[test]
    fn op_add_gated_produces_gated_item() {
        let (new_body, id) = op_add("", "gated task", DOC_ID, true).unwrap();
        assert!(new_body.contains("[/]"), "expected [/] in: {}", new_body);
        assert!(new_body.contains(&format!("[#{}]", id)));
        assert!(new_body.contains("gated task"));
    }

    #[test]
    fn op_add_returns_assigned_id() {
        let (body, id1) = op_add("", "task one", DOC_ID, false).unwrap();
        assert!(!id1.is_empty());
        assert!(body.contains(&format!("[#{}]", id1)));
        let (body2, id2) = op_add(&body, "task two", DOC_ID, false).unwrap();
        assert_ne!(id1, id2);
        assert!(body2.contains(&format!("[#{}]", id2)));
    }

    #[test]
    fn op_add_extracts_inline_tag_as_id() {
        let (new_body, id) = op_add(
            "",
            "2026-05-11 [#nopatchbackopencode] OpenCode agent-doc turn completed",
            DOC_ID,
            false,
        )
        .unwrap();
        assert_eq!(id, "nopatchbackopencode");
        assert!(
            new_body.contains(
                "- [ ] [#nopatchbackopencode] 2026-05-11 OpenCode agent-doc turn completed"
            ),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_inline_tag_strips_from_text() {
        let (new_body, id) = op_add("", "fix [#mybug] the thing", DOC_ID, false).unwrap();
        assert_eq!(id, "mybug");
        assert!(
            new_body.contains("- [ ] [#mybug] fix the thing"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_inline_tag_at_end() {
        let (new_body, id) = op_add("", "deploy the thing [#deploy1]", DOC_ID, false).unwrap();
        assert_eq!(id, "deploy1");
        assert!(
            new_body.contains("- [ ] [#deploy1] deploy the thing"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_inline_tag_dedup_uses_cleaned_text() {
        let (body, _) = op_add("", "fix [#mybug] the thing", DOC_ID, false).unwrap();
        let err = op_add(&body, "fix [#mybug] the thing", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("duplicate"),
            "expected duplicate error, got: {}",
            err
        );
    }

    #[test]
    fn op_add_inline_tag_rejects_existing_id() {
        let body = "- [ ] [#mybug] existing task\n";
        let err = op_add(body, "fix [#mybug] new task", DOC_ID, false).unwrap_err();
        assert!(
            format!("{}", err).contains("inline tag id already exists"),
            "expected inline tag id already exists, got: {}",
            err
        );
    }

    #[test]
    fn op_add_inline_tag_ignores_invalid_tag() {
        let (new_body, id) =
            op_add("", "see [#not-a-valid tag] because spaces", DOC_ID, false).unwrap();
        assert_ne!(id, "not-a-valid");
        assert!(
            new_body.contains("see [#not-a-valid tag] because spaces"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_add_leading_prefix_takes_precedence_over_inline() {
        let (new_body, id) =
            op_add("", "[#myid] text with [#other] inline", DOC_ID, false).unwrap();
        assert_eq!(id, "myid");
        assert!(
            new_body.contains("- [ ] [#myid] text with [#other] inline"),
            "got: {}",
            new_body
        );
    }

    #[test]
    fn op_done_marks_checked() {
        let body = "- [ ] [#a1b2] task\n";
        let new_body = op_done(body, "a1b2").unwrap();
        assert!(new_body.contains("[x]"));
    }

    #[test]
    fn op_done_accepts_hash_prefixed_id() {
        let body = "- [ ] [#a1b2] task\n";
        let new_body = op_done(body, "#a1b2").unwrap();
        assert!(new_body.contains("- [x] [#a1b2] task"));
    }

    #[test]
    fn op_done_unknown_id_errors() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_done(body, "zzzz").is_err());
    }

    #[test]
    fn op_edit_preserves_hash() {
        let body = "- [ ] [#a1b2] original\n";
        let new_body = op_edit(body, "a1b2", "updated").unwrap();
        assert!(new_body.contains("[#a1b2]"));
        assert!(new_body.contains("updated"));
        assert!(!new_body.contains("original"));
    }

    #[test]
    fn op_edit_multiline_replaces_existing_continuation() {
        let body = concat!(
            "- [ ] [#tmuxcrash] parent task\n",
            "  - [ ] [#tmuxcrash-old1] stale child\n",
            "  - [ ] [#tmuxcrash-old2] stale child two\n",
            "- [ ] [#keep1] sibling task\n"
        );
        let new_body = op_edit(
            body,
            "tmuxcrash",
            "parent task revised\n  - fresh child\n  - fresh child two",
        )
        .unwrap();
        assert!(new_body.contains("[#tmuxcrash] parent task revised"));
        assert!(new_body.contains("  - fresh child\n"));
        assert!(new_body.contains("  - fresh child two"));
        assert!(!new_body.contains("stale child"));
        assert!(!new_body.contains("stale child two"));
        assert!(new_body.contains("  - fresh child two\n- [ ] [#keep1] sibling task"));
    }

    #[test]
    fn op_edit_rejects_unindented_multiline_follow_up() {
        let body = "- [ ] [#a1b2] original\n";
        let err = op_edit(body, "a1b2", "updated\nsecond parent").unwrap_err();
        assert!(
            format!("{}", err)
                .contains("multiline text may only contain indented continuation lines"),
            "got: {err}"
        );
    }

    #[test]
    fn op_clear_empties_items() {
        let body = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two\n";
        let new_body = op_clear(body).unwrap();
        assert!(!new_body.contains("[#"));
    }

    #[test]
    fn op_clear_preserves_headers_and_spacing() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#a1b2] one\n",
            "\n",
            "### Later\n",
            "- [ ] [#c3d4] two\n"
        );
        let new_body = op_clear(body).unwrap();
        assert_eq!(new_body, "### Active\n\n### Later\n");
    }

    #[test]
    fn op_reorder_reorders_by_id() {
        let body = "- [ ] [#a1b2] first\n- [ ] [#c3d4] second\n- [ ] [#e5f6] third\n";
        let new_body = op_reorder(body, &["e5f6".to_string(), "a1b2".to_string()]).unwrap();
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].id, "e5f6");
        assert_eq!(items[1].id, "a1b2");
        assert_eq!(items[2].id, "c3d4");
    }

    #[test]
    fn op_reorder_keeps_headers_in_place() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#a1b2] first\n",
            "### Later\n",
            "- [ ] [#c3d4] second\n",
            "- [ ] [#e5f6] third\n"
        );
        let new_body = op_reorder(body, &["e5f6".to_string(), "a1b2".to_string()]).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines[0], "### Active");
        assert!(lines[1].contains("[#e5f6] third"), "got: {}", new_body);
        assert_eq!(lines[2], "### Later");
        assert!(lines[3].contains("[#a1b2] first"), "got: {}", new_body);
        assert!(lines[4].contains("[#c3d4] second"), "got: {}", new_body);
    }

    #[test]
    fn op_reorder_moves_nested_subtasks_with_parent_item() {
        let body = concat!(
            "- [ ] [#a1b2] first\n",
            "  - child a\n",
            "- [ ] [#c3d4] second\n",
            "  - child b\n"
        );
        let new_body = op_reorder(body, &["c3d4".to_string()]).unwrap();
        assert_eq!(
            new_body,
            concat!(
                "- [ ] [#c3d4] second\n",
                "  - child b\n",
                "- [ ] [#a1b2] first\n",
                "  - child a\n"
            )
        );
    }

    #[test]
    fn op_reorder_renumbers_ordered_lists() {
        let body = "1. [ ] [#a1b2] first\n2. [ ] [#c3d4] second\n3. [ ] [#e5f6] third\n";
        let new_body = op_reorder(body, &["e5f6".to_string(), "a1b2".to_string()]).unwrap();
        let lines: Vec<&str> = new_body.lines().collect();
        assert_eq!(lines[0], "1. [ ] [#e5f6] third");
        assert_eq!(lines[1], "2. [ ] [#a1b2] first");
        assert_eq!(lines[2], "3. [ ] [#c3d4] second");
    }

    #[test]
    fn op_reorder_unknown_id_errors() {
        let body = "- [ ] [#a1b2] one\n";
        assert!(op_reorder(body, &["zzzz".to_string()]).is_err());
    }

    // ---- Phase 2: state matrix + gate/ungate ----

    #[test]
    fn validate_transition_full_matrix() {
        use PendingOp::*;
        use PendingState::*;
        use TransitionResult::*;

        // Open
        assert_eq!(validate_transition(Open, Gate).unwrap(), Transition(Gated));
        assert!(validate_transition(Open, Ungate).is_err());
        assert_eq!(
            validate_transition(Open, MarkDone).unwrap(),
            Transition(Done)
        );

        // Gated
        assert_eq!(validate_transition(Gated, Gate).unwrap(), NoOp);
        assert_eq!(
            validate_transition(Gated, Ungate).unwrap(),
            Transition(Open)
        );
        assert_eq!(
            validate_transition(Gated, MarkDone).unwrap(),
            Transition(Done)
        );

        // Done
        assert!(validate_transition(Done, Gate).is_err());
        assert!(validate_transition(Done, Ungate).is_err());
        assert_eq!(validate_transition(Done, MarkDone).unwrap(), NoOp);
    }

    #[test]
    fn op_gate_open_to_gated() {
        let body = "- [ ] [#a1b2] task\n";
        let new_body = op_gate(body, "a1b2").unwrap();
        assert!(new_body.contains("- [/] [#a1b2]"));
    }

    #[test]
    fn op_gate_gated_is_noop() {
        let body = "- [/] [#a1b2] task\n";
        let new_body = op_gate(body, "a1b2").unwrap();
        // No-op: body unchanged byte-for-byte.
        assert_eq!(new_body, body);
    }

    #[test]
    fn op_gate_done_errors() {
        let body = "- [x] [#a1b2] task\n";
        let err = op_gate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot gate Done item"), "got: {}", msg);
    }

    #[test]
    fn op_gate_unknown_id_errors() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_gate(body, "zzzz").is_err());
    }

    #[test]
    fn op_ungate_gated_to_open() {
        let body = "- [/] [#a1b2] task\n";
        let new_body = op_ungate(body, "a1b2").unwrap();
        assert!(new_body.contains("- [ ] [#a1b2]"));
    }

    #[test]
    fn op_ungate_open_errors() {
        let body = "- [ ] [#a1b2] task\n";
        let err = op_ungate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot ungate Open"), "got: {}", msg);
    }

    #[test]
    fn op_ungate_done_errors() {
        let body = "- [x] [#a1b2] task\n";
        let err = op_ungate(body, "a1b2").unwrap_err();
        let msg = format!("{}", err);
        assert!(msg.contains("cannot ungate Done"), "got: {}", msg);
    }

    #[test]
    fn op_ungate_unknown_id_errors() {
        let body = "- [/] [#a1b2] task\n";
        assert!(op_ungate(body, "zzzz").is_err());
    }

    #[test]
    fn op_gate_preserves_other_items_and_text() {
        let body = "- [ ] [#a1b2] one\n- [ ] [#c3d4] two — gate: v0.32.6\n- [x] [#e5f6] three\n";
        let new_body = op_gate(body, "c3d4").unwrap();
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].state, PendingState::Open);
        assert_eq!(items[1].state, PendingState::Gated);
        assert_eq!(items[1].text, "two — gate: v0.32.6");
        assert_eq!(items[2].state, PendingState::Done);
    }

    #[test]
    fn generate_hash_deterministic_and_short() {
        let h = generate_hash("text", "doc", 0);
        assert_eq!(h.len(), 4);
        assert_eq!(h, generate_hash("text", "doc", 0));
        assert_ne!(h, generate_hash("text", "doc", 1));
    }

    #[test]
    fn generate_hash_n_width4_matches_generate_hash() {
        // Width-4 output must be bit-identical to the pre-#14z4 formula so
        // existing docs don't churn their IDs on re-backfill.
        let cases = [
            ("text", "doc", 0u64),
            ("refactor preflight", "abc123", 7),
            ("", "", 42),
            ("long text with spaces", "doc_id_long", 99),
        ];
        for (t, d, c) in cases {
            assert_eq!(generate_hash(t, d, c), generate_hash_n(t, d, c, 4));
        }
    }

    #[test]
    fn generate_hash_n_widths_have_correct_length() {
        for w in 4..=8 {
            let h = generate_hash_n("text", "doc", 0, w);
            assert_eq!(h.len(), w, "width {} produced len {}", w, h.len());
        }
        // Out-of-range widths clamp to [4, 8].
        assert_eq!(generate_hash_n("x", "y", 0, 1).len(), 4);
        assert_eq!(generate_hash_n("x", "y", 0, 20).len(), 8);
    }

    #[test]
    fn generate_hash_n_wider_extends_shorter() {
        // A wider hash must start with the shorter hash as a prefix so
        // visible widening is explainable to humans.
        let h4 = generate_hash_n("text", "doc", 0, 4);
        let h5 = generate_hash_n("text", "doc", 0, 5);
        let h8 = generate_hash_n("text", "doc", 0, 8);
        assert!(h5.starts_with(&h4), "h5={} h4={}", h5, h4);
        assert!(h8.starts_with(&h4), "h8={} h4={}", h8, h4);
        assert!(h8.starts_with(&h5), "h8={} h5={}", h8, h5);
    }

    #[test]
    fn assign_unique_hash_extends_on_collision() {
        // Pre-populate `taken` with the width-4 hash of "item". The next
        // assignment for the same text must either reuse the width-4 slot
        // with a different counter OR widen. Either way the result must
        // differ from the pre-populated value and be valid.
        let h4 = generate_hash_n("item", "doc", 0, 4);
        let mut taken = HashSet::new();
        taken.insert(h4.clone());
        let id = assign_unique_hash("item", "doc", &taken);
        assert_ne!(id, h4);
        assert!((4..=8).contains(&id.len()));
    }

    #[test]
    fn assign_unique_hash_widens_when_counter_exhausted_at_width4() {
        // Pre-populate `taken` with EVERY width-4 hash the retry loop would
        // try at counters 0..=3. Assignment must widen to 5 chars.
        let mut taken = HashSet::new();
        for c in 0..=3u64 {
            taken.insert(generate_hash_n("x", "d", c, 4));
        }
        let id = assign_unique_hash("x", "d", &taken);
        assert!(!taken.contains(&id));
        // Either width-4 (an untried counter) or width-5+. Accept both —
        // the important invariant is uniqueness, not forced widening.
        assert!((4..=8).contains(&id.len()));
    }

    #[test]
    fn backfill_assigns_collision_free_ids_under_pressure() {
        // Stress test: backfill 50 items. All must get unique 4..=8-char ids.
        let mut body = String::new();
        for i in 0..50 {
            body.push_str(&format!("- item {}\n", i));
        }
        let (out, changed) = backfill(&body, "doc", &HashSet::new());
        assert!(changed);
        let (_, items, _) = parse_items(&out);
        assert_eq!(items.len(), 50);
        let ids: HashSet<String> = items.iter().map(|i| i.id.clone()).collect();
        assert_eq!(ids.len(), 50, "ids must be unique");
        for id in &ids {
            assert!(
                (4..=8).contains(&id.len()),
                "id {} has width {}",
                id,
                id.len()
            );
        }
    }

    // ---- Typed gates ----

    #[test]
    fn parse_typed_gate_release() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
        assert_eq!(items[0].text, "Release v0.32.4");
    }

    #[test]
    fn parse_typed_gate_deploy() {
        let body = "- [/deploy] [#c3d4] Push CDN config\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, Some("deploy".to_string()));
    }

    #[test]
    fn parse_untyped_gate_has_no_gate_type() {
        let body = "- [/] [#a1b2] waiting\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, None);
    }

    #[test]
    fn parse_open_has_no_gate_type() {
        let body = "- [ ] [#a1b2] task\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, None);
    }

    #[test]
    fn render_typed_gate() {
        let item = PendingItem {
            marker: PendingListMarker::Bullet,
            id: "a1b2".to_string(),
            state: PendingState::Gated,
            gate_type: Some("release".to_string()),
            text: "Release v0.32.4".to_string(),
            continuation: String::new(),
        };
        assert_eq!(item.render(), "- [/release] [#a1b2] Release v0.32.4");
    }

    #[test]
    fn render_roundtrip_typed_gate() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n- [/deploy] [#c3d4] Push\n- [/] [#e5f6] Generic\n";
        let (p, items, post) = parse_items(body);
        let out = render_items(&p, &items, &post);
        assert_eq!(out, body);
    }

    #[test]
    fn op_resolve_gate_flips_matching() {
        let body = "- [/release] [#a1b2] Release v0.32.4\n- [/deploy] [#c3d4] Deploy\n- [/] [#e5f6] Generic gate\n";
        let (new_body, resolved) = op_resolve_gate(body, "release");
        assert_eq!(resolved, vec!["a1b2"]);
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].state, PendingState::Done); // was [/release]
        assert_eq!(items[0].gate_type, None); // cleared
        assert_eq!(items[1].state, PendingState::Gated); // [/deploy] untouched
        assert_eq!(items[1].gate_type, Some("deploy".to_string()));
        assert_eq!(items[2].state, PendingState::Gated); // [/] untouched
        assert_eq!(items[2].gate_type, None);
    }

    #[test]
    fn op_resolve_gate_no_match() {
        let body = "- [/release] [#a1b2] Release\n- [/] [#c3d4] Generic\n";
        let (new_body, resolved) = op_resolve_gate(body, "deploy");
        assert!(resolved.is_empty());
        assert_eq!(new_body, body);
    }

    #[test]
    fn op_resolve_gate_ignores_untyped() {
        let body = "- [/] [#a1b2] Generic gate\n";
        let (_, resolved) = op_resolve_gate(body, "release");
        assert!(resolved.is_empty());
    }

    #[test]
    fn op_resolve_gate_multiple_same_type() {
        let body = "- [/release] [#a1b2] First\n- [/release] [#c3d4] Second\n";
        let (_, resolved) = op_resolve_gate(body, "release");
        assert_eq!(resolved, vec!["a1b2", "c3d4"]);
    }

    #[test]
    fn op_set_gate_type_on_gated() {
        let body = "- [/] [#a1b2] Release v0.32.4\n";
        let new_body = op_set_gate_type(body, "a1b2", "release").unwrap();
        assert!(new_body.contains("[/release]"));
        let (_, items, _) = parse_items(&new_body);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
    }

    #[test]
    fn op_set_gate_type_errors_on_open() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_set_gate_type(body, "a1b2", "release").is_err());
    }

    #[test]
    fn op_set_gate_verify_round_trips_predicate() {
        let body = "- [/] [#saev] early-ack live verify\n";
        let new_body = op_set_gate_verify(
            body,
            "saev",
            "verify=ops_log:early_ack_pending;disproof=false ack-timeout",
            1749526200,
        )
        .unwrap();
        let (_, items, _) = parse_items(&new_body);
        // Still gated, untyped checkbox preserved.
        assert_eq!(items[0].state, PendingState::Gated);
        assert_eq!(items[0].gate_type, None);
        let pred = crate::gate_verify::parse_gate_predicate(&items[0].text).unwrap();
        assert_eq!(pred.verify.as_deref(), Some("early_ack_pending"));
        assert_eq!(pred.disproof.as_deref(), Some("false ack-timeout"));
        assert_eq!(pred.set_at, Some(1749526200));
    }

    #[test]
    fn op_set_gate_verify_errors_on_open() {
        let body = "- [ ] [#a1b2] task\n";
        assert!(op_set_gate_verify(body, "a1b2", "verify=ops_log:m", 1).is_err());
    }

    #[test]
    fn op_set_gate_verify_errors_on_empty_spec() {
        let body = "- [/] [#a1b2] task\n";
        assert!(op_set_gate_verify(body, "a1b2", "noop=1", 1).is_err());
    }

    #[test]
    fn op_set_gate_type_errors_on_done() {
        let body = "- [x] [#a1b2] task\n";
        assert!(op_set_gate_type(body, "a1b2", "release").is_err());
    }

    #[test]
    fn op_set_gate_type_replaces_existing() {
        let body = "- [/release] [#a1b2] task\n";
        let new_body = op_set_gate_type(body, "a1b2", "deploy").unwrap();
        assert!(new_body.contains("[/deploy]"));
        assert!(!new_body.contains("[/release]"));
    }

    #[test]
    fn parse_typed_gate_case_insensitive() {
        let body = "- [/Release] [#a1b2] task\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, Some("release".to_string()));
    }

    #[test]
    fn parse_typed_gate_with_hyphens_underscores() {
        let body =
            "- [/code-review] [#a1b2] Review PR\n- [/pre_release] [#c3d4] Pre-release check\n";
        let (_, items, _) = parse_items(body);
        assert_eq!(items[0].gate_type, Some("code-review".to_string()));
        assert_eq!(items[1].gate_type, Some("pre_release".to_string()));
    }

    #[test]
    fn detect_shadow_open_items_classifies_duplicate_and_shadow_only_ids() {
        let doc = concat!(
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "- [/] [#gate1] Gated live item\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked copy\n",
            "- [ ] [#live1] Keep in backlog\n",
            "- [ ] [#lost1] Drifted out of backlog\n",
            "- [x] [#done1] Already done\n",
            "-->\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert_eq!(
            report
                .duplicated_in_live_backlog
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["live1"]
        );
        assert_eq!(
            report
                .shadow_only
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            vec!["lost1"]
        );
    }

    #[test]
    fn detect_shadow_open_items_ignores_indented_nested_ids() {
        let doc = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Parent task\n",
            "  - [ ] [#nested1] Nested checklist item\n",
            "<!-- /agent:backlog -->\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert!(report.duplicated_in_live_backlog.is_empty());
        assert!(report.shadow_only.is_empty());
    }

    #[test]
    fn extract_items_by_id_preserves_nested_subtasks() {
        let body = concat!(
            "### Active\n",
            "- [ ] [#move1] Parent task\n",
            "  - child dependency\n",
            "- [ ] [#keep1] Keep task\n"
        );
        let (remaining, moved, matched) =
            extract_items_by_id(body, &["move1".to_string()]).unwrap();
        assert_eq!(matched, vec!["move1".to_string()]);
        assert!(remaining.contains("### Active\n"));
        assert!(!remaining.contains("[#move1]"));
        assert!(remaining.contains("[#keep1] Keep task"));
        assert_eq!(
            moved,
            concat!("- [ ] [#move1] Parent task\n", "  - child dependency\n")
        );
    }

    #[test]
    fn extract_items_by_id_preserves_ordered_list_style() {
        let body = concat!(
            "1. [ ] [#move1] Parent task\n",
            "2. [ ] [#keep1] Keep task\n",
            "3. [ ] [#keep2] Keep task two\n"
        );
        let (remaining, moved, matched) =
            extract_items_by_id(body, &["move1".to_string()]).unwrap();
        assert_eq!(matched, vec!["move1".to_string()]);
        assert_eq!(moved, "1. [ ] [#move1] Parent task\n");
        assert_eq!(
            remaining,
            concat!(
                "1. [ ] [#keep1] Keep task\n",
                "2. [ ] [#keep2] Keep task two\n"
            )
        );
    }

    #[test]
    fn detect_shadow_open_items_ignores_icebox_and_code_blocks() {
        let doc = concat!(
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#live1] Keep in backlog\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#cold1] Intentionally parked\n",
            "<!-- /agent:icebox -->\n\n",
            "```md\n",
            "- [ ] [#code1] Example only\n",
            "```\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert!(report.duplicated_in_live_backlog.is_empty());
        assert!(report.shadow_only.is_empty());
    }

    #[test]
    fn detect_shadow_open_items_ignores_exchange_transcript_items() {
        let doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ What are #next-steps to implement the planned ipc features?\n\n",
            "### Re: What are #next-steps to implement the planned ipc features?\n\n",
            "1. [#ipc1] finalize the lazily-serde contract so message shapes are stable.\n\n",
            "### Session Summary\n\n",
            "*Compacted.*\n\n",
            "Icebox:\n",
            "- [ ] [#cold1] Intentionally parked\n",
            "- [ ] [#cold2] Still parked\n\n",
            "❯ #code-review\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#ipc2] Live backlog item\n",
            "- [ ] [#cold1] Intentionally parked\n",
            "- [ ] [#cold2] Still parked\n",
            "<!-- /agent:icebox -->\n"
        );

        let report = detect_shadow_open_items(doc).unwrap();
        assert!(report.duplicated_in_live_backlog.is_empty());
        assert!(report.shadow_only.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_catches_missing_item() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Still here\n",
            "- [ ] [#gone1] Was open in baseline\n",
            "- [ ] [#gone2] Also open in baseline\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Still here\n",
            "<!-- /agent:backlog -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        let ids: Vec<&str> = report.dropped.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["gone1", "gone2"]);
    }

    #[test]
    fn detect_dropped_from_history_allows_done_in_live() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "- [x] [#item1] Now done\n",
            "<!-- /agent:backlog -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_allows_done_ids() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!("<!-- agent:backlog -->\n", "<!-- /agent:backlog -->\n");
        let mut done = HashSet::new();
        done.insert("item1".to_string());
        let report = detect_dropped_from_history(current, baseline, &done).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_allows_completed_archive_id() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:done -->\n",
            "- 2026-05-10 [#item1] Was open\n",
            "<!-- /agent:done -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_rejects_removed_completed_archive_alias() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:pending-done -->\n",
            "- 2026-05-10 [#item1] Was open\n",
            "<!-- /agent:pending-done -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert_eq!(report.dropped.len(), 1);
        assert_eq!(report.dropped[0].id, "item1");
    }

    #[test]
    fn detect_dropped_from_history_allows_icebox() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#item1] Archived\n",
            "<!-- /agent:icebox -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_allows_shadow() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- parked\n",
            "- [ ] [#item1] Drifted to shadow\n",
            "-->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_ignores_baseline_done_items() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [x] [#done1] Already done in baseline\n",
            "- [/] [#gate1] Gated in baseline\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!("<!-- agent:backlog -->\n", "<!-- /agent:backlog -->\n");
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        let ids: Vec<&str> = report.dropped.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["gate1"]);
    }

    #[test]
    fn detect_dropped_from_history_no_baseline_backlog() {
        let baseline = "# Just a document\nNo backlog here.\n";
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] New item\n",
            "<!-- /agent:backlog -->\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        assert!(report.dropped.is_empty());
    }

    #[test]
    fn detect_dropped_from_history_ignores_code_blocks_in_current() {
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#item1] Was open\n",
            "<!-- /agent:backlog -->\n"
        );
        let current = concat!(
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n\n",
            "```md\n",
            "- [ ] [#item1] In code block only\n",
            "```\n"
        );
        let report = detect_dropped_from_history(current, baseline, &HashSet::new()).unwrap();
        let ids: Vec<&str> = report.dropped.iter().map(|i| i.id.as_str()).collect();
        assert_eq!(ids, vec!["item1"]);
    }
