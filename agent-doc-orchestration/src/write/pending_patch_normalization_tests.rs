    use super::normalize_backlog_patch_response;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn doc_with_backlog(root: &TempDir, backlog_body: &str) -> (PathBuf, String) {
        let doc = root.path().join("doc.md");
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:backlog -->\n{backlog_body}<!-- /agent:backlog -->\n"
        );
        fs::write(&doc, &content).unwrap();
        (doc, content)
    }

    fn doc_with_todo(root: &TempDir, todo_body: &str) -> (PathBuf, String) {
        let doc = root.path().join("todo.md");
        let content = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange -->\n❯ Please reply\n<!-- /agent:exchange -->\n\n<!-- agent:todo patch=replace -->\n{todo_body}<!-- /agent:todo -->\n"
        );
        fs::write(&doc, &content).unwrap();
        (doc, content)
    }

    #[test]
    fn normalize_pending_patch_repairs_lone_bare_placeholder() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#] repair placeholder\n",
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("lone bare placeholder should be normalized");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("repair placeholder"));
        assert!(rewritten.contains("- [ ] [#"));
        assert!(
            !rewritten.contains("- [ ] [#] repair placeholder"),
            "bare placeholder must not persist: {}",
            rewritten
        );
    }

    #[test]
    fn normalize_pending_patch_rejects_stacked_leading_id_prefixes() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#] [#ship1] release checklist\n",
        )];

        let err =
            match normalize_backlog_patch_response(&doc, &content, patches, String::new(), false) {
                Ok(_) => panic!("stacked leading id prefixes should be rejected"),
                Err(err) => err,
            };
        let msg = err.to_string();
        assert!(
            msg.contains("pending/backlog patch"),
            "unexpected error: {}",
            msg
        );
        assert!(
            msg.contains("duplicate leading custom id prefix"),
            "unexpected error: {}",
            msg
        );
    }

    #[test]
    fn normalize_pending_patch_allows_existing_alias_tag_items() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!("### Active\n", "- [ ] [#yckq] [#ss01] ShipStation fix\n");
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#new1] add phone confirmation item\n",
                "- [ ] [#yckq] [#ss01] ShipStation fix\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("existing alias-tag items should not block normalization");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("[#new1] add phone confirmation item"));
        assert!(rewritten.contains("[#yckq] [#ss01] ShipStation fix"));
    }

    #[test]
    fn normalize_pending_patch_preserves_interleaved_headers() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!(
            "### Active\n",
            "- [ ] [#keep1] existing item\n",
            "\n",
            "### Later\n",
            "- [ ] [#keep2] later item\n"
        );
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#keep1] existing item\n",
                "- [ ] [#new1] new top item\n",
                "\n",
                "### Later\n",
                "- [ ] [#keep2] later item\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("header-preserving patch should normalize");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(
            rewritten
                .contains("### Active\n- [ ] [#keep1] existing item\n- [ ] [#new1] new top item\n")
        );
        assert!(rewritten.contains("\n\n### Later\n- [ ] [#keep2] later item\n"));
    }

    #[test]
    fn normalize_pending_patch_merges_partial_structured_prefix() {
        let tmp = TempDir::new().unwrap();
        let backlog = concat!(
            "### Active\n",
            "- [ ] [#keep1] existing item\n",
            "\n",
            "### Later\n",
            "- [ ] [#keep2] later item\n"
        );
        let (doc, content) = doc_with_backlog(&tmp, backlog);
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            concat!(
                "### Active\n",
                "- [ ] [#keep1] existing item\n",
                "- [ ] [#new1] new top item\n"
            ),
        )];

        normalize_backlog_patch_response(&doc, &content, patches, String::new(), false)
            .expect("prefix-only structured patch should merge with later sections");

        let rewritten = fs::read_to_string(&doc).unwrap();
        assert!(rewritten.contains("[#new1] new top item"));
        assert!(rewritten.contains("### Later\n- [ ] [#keep2] later item\n"));
    }

    #[test]
    fn write_flags_allow_replace_bypasses_enforcement() {
        let tmp = TempDir::new().unwrap();
        let (doc, content) = doc_with_backlog(&tmp, "- [ ] [#aaaa] existing\n");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#zzzz] new\n",
        )];
        normalize_backlog_patch_response(&doc, &content, patches.clone(), String::new(), true)
            .expect("allow_replace=true should bypass enforcement");
        super::enforce_no_replace_pending(&patches, true)
            .expect("allow=true should bypass enforcement");
    }

    #[test]
    fn write_flags_default_rejects_replace_pending() {
        let tmp = TempDir::new().unwrap();
        let (_doc, _content) = doc_with_backlog(&tmp, "- [ ] [#aaaa] existing\n");
        let patches = vec![crate::template::PatchBlock::new(
            "backlog",
            "- [ ] [#zzzz] new\n",
        )];
        super::enforce_no_replace_pending(&patches, false)
            .expect_err("allow=false should reject backlog replacement");
    }

    #[test]
    fn destructive_todo_patch_is_rejected_when_it_drops_checklist_items() {
        let tmp = TempDir::new().unwrap();
        let (_doc, content) = doc_with_todo(
            &tmp,
            concat!(
                "### Phase 1\n\n",
                "- [x] Select benchmark\n",
                "- [x] Write methodology\n\n",
                "### Phase 2\n\n",
                "- [ ] Expand git signal extraction\n",
                "- [ ] Re-score sessions\n",
            ),
        );
        let patches = vec![crate::template::PatchBlock::new(
            "todo",
            concat!(
                "### Phase 1\n\n",
                "- [x] Select benchmark\n",
                "- [x] Write methodology\n",
            ),
        )];

        let err = super::enforce_no_destructive_todo_patch(&content, &patches)
            .expect_err("subset todo patch should fail closed");
        let msg = err.to_string();
        assert!(
            msg.contains("patch:todo would reduce total checklist item count from 4 to 2"),
            "unexpected error: {}",
            msg
        );
    }

    #[test]
    fn todo_patch_with_same_checklist_count_is_allowed() {
        let tmp = TempDir::new().unwrap();
        let (_doc, content) = doc_with_todo(
            &tmp,
            concat!(
                "### Phase 1\n\n",
                "- [ ] Original item 1\n",
                "- [ ] Original item 2\n",
            ),
        );
        let patches = vec![crate::template::PatchBlock::new(
            "todo",
            concat!(
                "### Phase 1\n\n",
                "- [x] Updated item 1\n",
                "- [ ] Updated item 2\n",
            ),
        )];

        super::enforce_no_destructive_todo_patch(&content, &patches)
            .expect("same-size todo rewrite should remain allowed");
    }
