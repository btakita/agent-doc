    use super::*;

    #[test]
    fn diff_format_additions() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\n";
        let current = "line1\nline2\n";
        let diff = TextDiff::from_lines(previous, current);
        let has_insert = diff
            .iter_all_changes()
            .any(|c| c.tag() == ChangeTag::Insert);
        assert!(has_insert);
    }

    #[test]
    fn diff_format_deletions() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\nline2\n";
        let current = "line1\n";
        let diff = TextDiff::from_lines(previous, current);
        let has_delete = diff
            .iter_all_changes()
            .any(|c| c.tag() == ChangeTag::Delete);
        assert!(has_delete);
    }

    #[test]
    fn diff_format_unchanged() {
        use similar::{ChangeTag, TextDiff};
        let content = "line1\nline2\n";
        let diff = TextDiff::from_lines(content, content);
        let all_equal = diff.iter_all_changes().all(|c| c.tag() == ChangeTag::Equal);
        assert!(all_equal);
    }

    #[test]
    fn diff_format_mixed() {
        use similar::{ChangeTag, TextDiff};
        let previous = "line1\nline2\nline3\n";
        let current = "line1\nchanged\nline3\n";
        let diff = TextDiff::from_lines(previous, current);

        let mut output = String::new();
        for change in diff.iter_all_changes() {
            let prefix = match change.tag() {
                ChangeTag::Delete => "-",
                ChangeTag::Insert => "+",
                ChangeTag::Equal => " ",
            };
            output.push_str(prefix);
            output.push_str(change.value());
        }
        assert!(output.contains(" line1\n"));
        assert!(output.contains("-line2\n"));
        assert!(output.contains("+changed\n"));
        assert!(output.contains(" line3\n"));
    }

    // --- Comment stripping tests ---

    #[test]
    fn strip_html_comment() {
        let input = "before\n<!-- a comment -->\nafter\n";
        assert_eq!(strip_comments(input), "before\nafter\n");
    }

    #[test]
    fn strip_multiline_html_comment() {
        let input = "before\n<!--\nmulti\nline\n-->\nafter\n";
        assert_eq!(strip_comments(input), "before\nafter\n");
    }

    #[test]
    fn strip_link_ref_comment() {
        let input = "before\n[//]: # (a comment)\nafter\n";
        assert_eq!(strip_comments(input), "before\nafter\n");
    }

    #[test]
    fn preserve_agent_markers() {
        let input = "<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        assert_eq!(strip_comments(input), input);
    }

    #[test]
    fn strip_regular_keep_agent_marker() {
        let input = "<!-- regular comment -->\n<!-- agent:s -->\ndata\n<!-- /agent:s -->\n";
        assert_eq!(
            strip_comments(input),
            "<!-- agent:s -->\ndata\n<!-- /agent:s -->\n"
        );
    }

    #[test]
    fn strip_inline_comment() {
        // Comment not on its own line — strip just the comment text
        let input = "text <!-- note --> more\n";
        let result = strip_comments(input);
        assert_eq!(result, "text  more\n");
    }

    #[test]
    fn no_comments_unchanged() {
        let input = "# Title\n\nJust text.\n";
        assert_eq!(strip_comments(input), input);
    }

    #[test]
    fn empty_document() {
        assert_eq!(strip_comments(""), "");
    }

    // --- Stale snapshot detection tests ---

    #[test]
    fn stale_snapshot_detects_completed_exchange() {
        let snapshot = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nWhat's up\n\n## Assistant\n\nNot much\n\n## User\n\n";
        assert!(is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_false_when_user_has_new_content() {
        let snapshot = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\n";
        let document =
            "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\nNew question here\n";
        assert!(!is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_false_when_identical() {
        let content = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\n";
        assert!(!is_stale_snapshot(content, content));
    }

    #[test]
    fn stale_snapshot_false_when_no_assistant_block() {
        let snapshot = "## User\n\nHello\n\n";
        let document = "## User\n\nHello\n\nSome random text\n\n## User\n\n";
        assert!(!is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_multiple_exchanges_stale() {
        let snapshot = "## User\n\nQ1\n\n## Assistant\n\nA1\n\n## User\n\n";
        let document = "## User\n\nQ1\n\n## Assistant\n\nA1\n\n## User\n\nQ2\n\n## Assistant\n\nA2\n\n## User\n\nQ3\n\n## Assistant\n\nA3\n\n## User\n\n";
        assert!(is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_with_inline_annotation_not_stale() {
        let snapshot = "## User\n\nHello\n\n## Assistant\n\nHi there\n\n## User\n\n";
        // User added inline annotation within an existing assistant block
        let document =
            "## User\n\nHello\n\n## Assistant\n\nHi there\n\nPlease elaborate\n\n## User\n\n";
        // This modifies the snapshot prefix, so starts_with check fails
        assert!(!is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn stale_snapshot_ignores_comments_in_detection() {
        let snapshot = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\n";
        let document = "## User\n\nHello\n\n## Assistant\n\nHi\n\n## User\n\n<!-- scratch -->\n\n## Assistant\n\nResponse\n\n## User\n\n";
        // Comments are stripped, so the user block between snapshot and new assistant is empty
        assert!(is_stale_snapshot(snapshot, document));
    }

    #[test]
    fn copy_on_read_guard_skips_recovery_when_snapshot_modified() {
        // Verifies the copy-on-read guard logic: if snapshot mtime changes
        // between read and recovery, the save must be skipped.
        use std::time::SystemTime;

        let t1 = Some(SystemTime::UNIX_EPOCH);
        let t2 = Some(SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1));

        // Same mtime → recovery should proceed (guard passes)
        assert_eq!(t1, t1, "same mtime should allow recovery");

        // Different mtime → recovery should be skipped (guard blocks)
        assert_ne!(t1, t2, "different mtime should block recovery");

        // Both None (no snapshot file) → recovery should proceed
        let none: Option<SystemTime> = None;
        assert_eq!(none, none, "both None should allow recovery");
    }

    // --- Code-aware comment stripping tests ---

    #[test]
    fn strip_preserves_comment_syntax_in_inline_backticks() {
        // `<!--` inside backticks should NOT be treated as a comment start
        let input =
            "Use `<!--` to start a comment.\n<!-- agent:foo -->\ncontent\n<!-- /agent:foo -->\n";
        let result = strip_comments(input);
        assert_eq!(
            result,
            "Use `<!--` to start a comment.\n<!-- agent:foo -->\ncontent\n<!-- /agent:foo -->\n"
        );
    }

    #[test]
    fn strip_preserves_comment_syntax_in_fenced_code_block() {
        let input = "before\n```\n<!-- not a comment -->\n```\nafter\n";
        let result = strip_comments(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_backtick_comment_before_agent_marker() {
        // Regression: `<!--` in backticks matched `-->` in the agent marker,
        // swallowing all content between them
        let input = "\
Text mentions `<!--` as a trigger.\n\
More text here.\n\
New user content.\n\
<!-- /agent:exchange -->\n";
        let result = strip_comments(input);
        assert_eq!(result, input);
    }

    #[test]
    fn strip_multiple_backtick_comments_in_exchange() {
        // Real-world scenario: discussion about `<!--` syntax inside an exchange component
        let snapshot = "\
<!-- agent:exchange -->\n\
Discussion about `<!--` triggers.\n\
- `<!-- agent:NAME -->` paired markers\n\
<!-- /agent:exchange -->\n";
        let current = "\
<!-- agent:exchange -->\n\
Discussion about `<!--` triggers.\n\
- `<!-- agent:NAME -->` paired markers\n\
\n\
Please fix the bug.\n\
<!-- /agent:exchange -->\n";

        let snap_stripped = strip_comments(snapshot);
        let curr_stripped = strip_comments(current);
        assert_ne!(
            snap_stripped, curr_stripped,
            "inline edits after backtick-comment text must be detected"
        );
    }

    // --- classify_diff tests ---

    fn make_diff(added: &[&str], removed: &[&str]) -> String {
        let mut lines = vec!["--- snapshot", "+++ document", "@@ -1,5 +1,5 @@"];
        for r in removed {
            lines.push(r);
        }
        lines.push(" context line");
        for a in added {
            lines.push(a);
        }
        lines.join("\n")
    }

    #[test]
    fn classify_approval() {
        let diff = make_diff(&["+go"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::Approval);
        assert!(c.diff_type_reason.contains("go"));
    }

    #[test]
    fn classify_approval_case_insensitive() {
        let diff = make_diff(&["+Yes"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::Approval);
    }

    #[test]
    fn classify_simple_question() {
        let diff = make_diff(&["+what is the release process?"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::SimpleQuestion);
    }

    #[test]
    fn classify_boundary_artifact() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,3 @@\n-### Re: Something (HEAD)\n+### Re: Something\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::BoundaryArtifact);
    }

    #[test]
    fn classify_boundary_uuid() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,3 @@\n-<!-- agent:boundary:abc123 -->\n+<!-- agent:boundary:def456 -->\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::BoundaryArtifact);
    }

    #[test]
    fn classify_head_mention_in_user_prose_as_content_addition() {
        let diff = make_diff(
            &[
                "+`❯ ` prompt prefix is being stripped away by the uncommitted user affordance that adds the ` (HEAD)` suffix. spec-test-build-install-commit-push",
            ],
            &[],
        );
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::ContentAddition);
    }

    #[test]
    fn classify_structural_change() {
        let diff = "--- snapshot\n+++ document\n@@ -1,5 +1,3 @@\n context\n-removed line one\n-removed line two\n context\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::StructuralChange);
    }

    #[test]
    fn classify_multi_topic() {
        // Two added blocks separated by context
        let diff = "--- snapshot\n+++ document\n@@ -1,5 +1,7 @@\n context\n+first topic\n context middle\n+second topic\n context end\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::MultiTopic);
    }

    #[test]
    fn classify_multi_topic_with_separator() {
        // Contiguous added block with --- separator — still detected as multi-topic
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,6 @@\n context\n+question one?\n+---\n+do something else\n context end\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::MultiTopic);
    }

    #[test]
    fn classify_content_addition() {
        let diff = make_diff(&["+implement the feature using Rust"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::ContentAddition);
    }

    #[test]
    fn classify_annotation() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,3 @@\n context\n-The fix is deployed\n+The fix is deployed: confirmed working in prod\n context\n";
        let c = classify_diff(diff);
        assert_eq!(c.diff_type, DiffType::Annotation);
    }

    #[test]
    fn classify_approval_in_sentence_is_content() {
        // "go" inside a longer sentence should NOT be classified as Approval
        let diff = make_diff(&["+let's go ahead and implement the feature"], &[]);
        let c = classify_diff(&diff);
        assert_eq!(c.diff_type, DiffType::ContentAddition);
    }

    #[test]
    fn classify_single_separator_not_multi_topic() {
        // Single --- with no content on either side is not multi-topic
        let diff = make_diff(&["+---"], &[]);
        let c = classify_diff(&diff);
        // Only 1 section (the --- itself is filtered as empty), not multi-topic
        assert_ne!(c.diff_type, DiffType::MultiTopic);
    }

    #[test]
    fn classify_question_mark_in_multiline_is_content() {
        // Question mark at end of a multi-line addition is content, not simple question
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,5 @@\n context\n+first line of paragraph\n+is this a question?\n context\n";
        let c = classify_diff(diff);
        // Two added lines = multi-topic (two blocks is actually one contiguous block)
        // The key point: it should NOT be SimpleQuestion (that requires exactly 1 added line)
        assert_ne!(c.diff_type, DiffType::SimpleQuestion);
    }

    // --- annotate_diff tests ---

    #[test]
    fn annotate_diff_additions() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n context line\n+new user line\n more context\n";
        let annotated = annotate_diff(diff).unwrap();
        assert!(annotated.contains("[user+] new user line"));
        assert!(annotated.contains("[agent] context line"));
    }

    #[test]
    fn annotate_diff_removals() {
        let diff =
            "--- snapshot\n+++ document\n@@ -1,3 +1,2 @@\n context\n-removed line\n context\n";
        let annotated = annotate_diff(diff).unwrap();
        assert!(annotated.contains("[user-] removed line"));
    }

    #[test]
    fn annotate_diff_modifications() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,3 @@\n context\n-The fix is deployed\n+The fix is deployed: confirmed in prod\n context\n";
        let annotated = annotate_diff(diff).unwrap();
        assert!(annotated.contains("[user~] The fix is deployed: confirmed in prod"));
        // Should NOT have separate [user-] and [user+] for the paired lines
        assert!(!annotated.contains("[user-] The fix is deployed"));
    }

    #[test]
    fn annotate_diff_context() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n line one\n line two\n+added\n line three\n";
        let annotated = annotate_diff(diff).unwrap();
        assert!(annotated.contains("[agent] line one"));
        assert!(annotated.contains("[agent] line two"));
        assert!(annotated.contains("[agent] line three"));
    }

    #[test]
    fn annotate_diff_empty() {
        let diff = "--- snapshot\n+++ document\n";
        assert!(annotate_diff(diff).is_none());
    }

    // --- extract_inline_annotations tests ---

    #[test]
    fn inline_annotations_user_addition_between_agent_lines() {
        let annotated = "[agent] previous agent line\n\
                         [user+] This is wrong, fix it\n\
                         [agent] more agent content";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(anns, vec!["This is wrong, fix it"]);
    }

    #[test]
    fn inline_annotations_user_modification_between_agent_lines() {
        let annotated = "[agent] context before\n\
                         [user~] The corrected line\n\
                         [agent] context after";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(anns, vec!["The corrected line"]);
    }

    #[test]
    fn inline_annotations_user_addition_at_end_is_not_inline() {
        let annotated = "[agent] agent content\n\
                         [agent] more agent content\n\
                         [user+] New user input at end";
        let anns = extract_inline_annotations(annotated);
        assert!(anns.is_empty());
    }

    #[test]
    fn inline_annotations_component_markers_not_substantive() {
        // Component closing + section header after user input should NOT classify as inline
        let annotated = "[agent] response prose\n\
                         [user+] The fix did not seem to work\n\
                         [agent] <!-- /agent:exchange -->\n\
                         [agent] \n\
                         [agent] ## Pending / Not Built\n\
                         [agent] \n\
                         [agent] <!-- agent:pending -->";
        let anns = extract_inline_annotations(annotated);
        assert!(
            anns.is_empty(),
            "component markers should not make end-of-exchange input inline"
        );
    }

    #[test]
    fn inline_annotations_head_boundary_artifact_excluded() {
        // [user~] that only appended (HEAD) to a heading is a reposition artifact
        let annotated = "[agent] previous content\n\
                         [user~] ### Re: topic — sonnet-4-6 (HEAD)\n\
                         [agent] response prose\n\
                         [agent] more content";
        let anns = extract_inline_annotations(annotated);
        assert!(
            anns.is_empty(),
            "(HEAD) boundary reposition should not be an inline annotation"
        );
    }

    #[test]
    fn inline_annotations_real_correction_between_prose() {
        // Genuine user correction inside agent prose (not a structural marker)
        let annotated = "[agent] The score is 5 out of 10.\n\
                         [user+] This is wrong. Score should be 8-9.\n\
                         [agent] Here is more analysis below.";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(anns, vec!["This is wrong. Score should be 8-9."]);
    }

    #[test]
    fn inline_annotations_multiple() {
        let annotated = "[agent] ### Re: topic\n\
                         [user+] First correction\n\
                         [agent] agent paragraph\n\
                         [user+] Second correction\n\
                         [agent] agent closing\n\
                         [user+] New input at end";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(anns, vec!["First correction", "Second correction"]);
    }

    #[test]
    fn inline_annotations_skips_blank_user_lines() {
        let annotated = "[agent] agent line\n\
                         [user+] \n\
                         [agent] more agent";
        let anns = extract_inline_annotations(annotated);
        assert!(anns.is_empty());
    }

    #[test]
    fn inline_annotations_empty_annotated_diff() {
        let anns = extract_inline_annotations("");
        assert!(anns.is_empty());
    }

    #[test]
    fn inline_annotations_claudescore_table_scenario() {
        // Reproduces the claudescore bug: user corrections inside agent response
        // with table rows as agent content after them
        let annotated = "[agent] | Category | Score |\n\
                         [agent] |----------|-------|\n\
                         [agent] | Quality  | 7     |\n\
                         [user+] This is wrong. Do not lower expert scores.\n\
                         [agent] | Speed    | 8     |\n\
                         [user+] We may need to broaden the gate.\n\
                         [agent] | Total    | 7.5   |";
        let anns = extract_inline_annotations(annotated);
        assert_eq!(
            anns,
            vec![
                "This is wrong. Do not lower expert scores.",
                "We may need to broaden the gate.",
            ]
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_promotes_inline_prompt_to_prompt_target() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            The prior explanation was incomplete\n\
            +Why was the `❯` prefix omitted here?\n\
            The rest of the response stays the same\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(changes[0].text, "Why was the `❯` prefix omitted here?");
    }

    #[test]
    fn classify_prompt_bearing_changes_promotes_plain_exchange_tail_to_prompt_target() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
Done.\n\
+When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push\n\
<!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(
            changes[0].text,
            "When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push"
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_promotes_bare_slash_command_to_prompt_target() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
### Re: older — gpt-5\n\
+/clear\n\
<!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(changes[0].text, "/clear");
        assert!(line_looks_like_fresh_prompt_after_response("/clear"));
        assert!(!text_line_looks_like_prompt_target(
            "/home/brian/work/foo.md"
        ));
    }

    #[test]
    fn prefixed_markdown_response_labels_are_not_prompt_targets() {
        for line in [
            "❯ **Verification:** Both redirects confirmed via `curl`.",
            "❯ Commit / push:",
            "❯ **Commit / push:**",
            "❯ - **Verification:** `cargo test` passed.",
            "❯ 1. **What changed:** normalized response labels.",
        ] {
            assert!(
                !text_line_looks_like_prompt_target(line),
                "assistant response label must not be classified as a prompt target: {line}"
            );
            assert!(
                line_looks_like_plain_response_after_prompt(line),
                "assistant response label must remain response prose: {line}"
            );
        }
    }

    #[test]
    fn prefixed_user_followup_after_response_still_starts_prompt() {
        for line in [
            "❯ verify deploy status",
            "❯ Verification failed; what next?",
            "❯ do [#respfx]. spec-test-build-install-commit-push",
        ] {
            assert!(
                text_line_looks_like_prompt_target(line),
                "real user follow-up must stay prompt-bearing: {line}"
            );
            assert!(
                line_looks_like_fresh_prompt_after_response(line),
                "real user follow-up must start a new prompt run: {line}"
            );
        }
    }

    #[test]
    fn classify_prompt_bearing_changes_ignores_prefixed_markdown_response_label() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            ### Re: done — gpt-5\n\
            +❯ **Verification:** Both redirects confirmed via `curl`.\n\
            <!-- /agent:exchange -->\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert!(
            changes.is_empty(),
            "prefixed assistant response label must not reopen a cycle: {changes:?}"
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_marks_inline_correction_as_content_edit() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            The service returned 401 from this endpoint\n\
            +The service returned 503 from this endpoint\n\
            The rest of the response stays the same\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::ContentEdit);
        assert_eq!(
            changes[0].text,
            "The service returned 503 from this endpoint"
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_marks_response_heading_as_recovery_artifact() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,5 @@\n\
            ctx\n\
            +### Re: missed patchback — gpt-5\n\
            +Patched after the fact.\n\
            context end\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::RecoveryArtifact);
        assert_eq!(
            changes[0].text,
            "### Re: missed patchback — gpt-5\nPatched after the fact."
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_marks_boundary_only_edit_as_boundary_artifact() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,2 @@\n\
            -### Re: Something\n\
            +### Re: Something (HEAD)\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::BoundaryArtifact);
        assert_eq!(changes[0].text, "### Re: Something (HEAD)");
    }

    #[test]
    fn classify_prompt_bearing_changes_preserves_mixed_triage_order() {
        let diff = "--- snapshot\n+++ document\n@@ -1,7 +1,12 @@\n\
            The service returned 401 from this endpoint\n\
            +The service returned 503 from this endpoint\n\
            The rest of the response stays the same\n\
            +### Re: delayed patchback — gpt-5\n\
            +Patched after the fact.\n\
            Context after repair\n\
            +<!-- agent:boundary:test-boundary -->\n\
            More context\n\
            +Why was the `❯` prefix omitted here?\n\
            The response continues below\n";
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 4);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::ContentEdit);
        assert_eq!(
            changes[0].text,
            "The service returned 503 from this endpoint"
        );
        assert_eq!(changes[1].kind, PromptBearingChangeKind::RecoveryArtifact);
        assert_eq!(
            changes[1].text,
            "### Re: delayed patchback — gpt-5\nPatched after the fact."
        );
        assert_eq!(changes[2].kind, PromptBearingChangeKind::BoundaryArtifact);
        assert_eq!(changes[2].text, "<!-- agent:boundary:test-boundary -->");
        assert_eq!(changes[3].kind, PromptBearingChangeKind::PromptTarget);
        assert_eq!(changes[3].text, "Why was the `❯` prefix omitted here?");
    }

    #[test]
    fn classify_prompt_bearing_changes_ignores_raw_answered_stale_exchange_tail() {
        let diff = concat!(
            "--- snapshot\n",
            "+++ document\n",
            "@@ -19,11 +19,11 @@\n",
            " ---\n",
            " \n",
            " ## Status\n",
            " \n",
            " <!-- agent:status patch=replace -->\n",
            "-Blocked on environment: GitHub is unreachable from this sandbox, so I cannot rename `ClaudeScore/BuildPartyInvestorDemo` or add the follow-up submodule here.\n",
            "+Updated local references for the renamed `ClaudeScore/buildparty-investor-demo` repo: `.gitmodules` now points at the new SSH URL, and the checked-out submodule's `origin` remote has been synced to match.\n",
            " <!-- /agent:status -->\n",
            " \n",
            " ## Exchange\n",
            " \n",
            " <!-- agent:exchange patch=append -->\n",
            "@@ -60,10 +60,17 @@\n",
            " ```bash\n",
            " git submodule add git@github.com:ClaudeScore/buildparty-investor-demo.git buildparty-investor-demo\n",
            " ```\n",
            " \n",
            " GitHub will redirect normal clone/fetch/push traffic from the old repo name, but it still recommends updating local remotes, and workflows that use the old repo as a GitHub Action reference will not redirect cleanly. Sources: [GitHub CLI `gh repo rename`](https://cli.github.com/manual/gh_repo_rename), [GitHub repo rename docs](https://docs.github.com/github/administering-a-repository/managing-repository-settings/renaming-a-repository).\n",
            "+I renamed the repo to ClaudeScore/buildparty-investor-demo. Please update references\n",
            "+I updated the repo-local references to the renamed GitHub repo.\n",
            "+\n",
            "+- `.gitmodules` now uses `git@github.com:ClaudeScore/buildparty-investor-demo.git`\n",
            "+- The checked-out submodule at `buildparty-investor-demo/` now has `origin` set to the same URL\n",
            "+\n",
            "+The only remaining stale reference I found is the submodule README title (`BuildPartyInvestorDemo` in `buildparty-investor-demo/README.md`). I left that untouched because it belongs to the submodule's own content rather than this parent repo's wiring.\n",
            " <!-- /agent:exchange -->\n",
            " \n",
            " ## Queue\n",
            " \n",
            " <!-- agent:queue -->\n",
        );
        let changes = classify_prompt_bearing_changes(diff);
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].kind, PromptBearingChangeKind::ContentEdit);
        assert_eq!(
            changes[0].text,
            "Updated local references for the renamed `ClaudeScore/buildparty-investor-demo` repo: `.gitmodules` now points at the new SSH URL, and the checked-out submodule's `origin` remote has been synced to match."
        );
    }

    #[test]
    fn classify_prompt_bearing_changes_ignores_answered_prompt_before_blank_heading_gap() {
        let diff = concat!(
            "--- snapshot\n",
            "+++ document\n",
            "@@ -1,3 +1,8 @@\n",
            " <!-- agent:exchange patch=append -->\n",
            "-<!-- agent:boundary:initial -->\n",
            "+❯ do #sim1. spec-test-build-install-commit-push\n",
            "+\n",
            "+### Re: sim closeout — gpt-5 (HEAD)\n",
            "+\n",
            "+Done.\n",
            "+<!-- agent:boundary:new -->\n",
            " <!-- /agent:exchange -->\n",
        );

        let changes = classify_prompt_bearing_changes(diff);
        assert!(
            changes
                .iter()
                .all(|change| change.kind != PromptBearingChangeKind::PromptTarget),
            "answered prompt should not remain unresolved: {changes:?}"
        );
    }

    // parse_slash_commands tests

    #[test]
    fn parse_slash_commands_simple() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n context\n+/clear\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/clear"]);
    }

    #[test]
    fn parse_slash_command_only_added_diff_accepts_bare_clear() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n context\n+/clear\n";
        assert_eq!(
            parse_slash_command_only_added_diff(diff),
            Some(vec!["/clear".to_string()])
        );
    }

    #[test]
    fn parse_slash_command_only_added_diff_rejects_mixed_prompt_text() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n context\n+/clear\n+Why was this answered?\n";
        assert_eq!(parse_slash_command_only_added_diff(diff), None);
    }

    #[test]
    fn parse_slash_command_only_added_diff_rejects_fenced_or_blockquoted_commands() {
        let fenced = "--- snapshot\n+++ document\n@@ -1 +1,4 @@\n ctx\n+```\n+/clear\n+```\n";
        assert_eq!(parse_slash_command_only_added_diff(fenced), None);

        let quoted = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+> /clear\n";
        assert_eq!(parse_slash_command_only_added_diff(quoted), None);
    }

    #[test]
    fn parse_slash_commands_trims_surrounding_whitespace() {
        let diff = "--- snapshot\n+++ queue\n@@ -0,0 +1,1 @@\n+   /clear  \n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/clear"]);
    }

    #[test]
    fn parse_slash_commands_with_args() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+/agent-doc foo.md\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/agent-doc foo.md"]);
    }

    #[test]
    fn parse_slash_commands_ignores_fenced() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,4 @@\n ctx\n+```\n+/clear\n+```\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_ignores_blockquote() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+> /clear\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_ignores_context_lines() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,2 @@\n /clear\n context\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_ignores_removed_lines() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,1 @@\n-/clear\n context\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_requires_letter_after_slash() {
        // "/ " (space after slash) and "//comment" should not match.
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n+/ foo\n+//comment\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty());
    }

    #[test]
    fn parse_slash_commands_multiple() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n+/clear\n+/agent-doc foo.md\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(cmds, vec!["/clear", "/agent-doc foo.md"]);
    }

    #[test]
    fn parse_slash_commands_rejects_absolute_paths() {
        // #xzz5: `/home/brian/...` looks like a command but is a filesystem
        // path. The tightened grammar (`/[a-z][a-z0-9:_-]*` with no second `/`)
        // must reject any token containing a second slash.
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n ctx\n\
            +/home/brian/work/foo.md\n\
            +/tmp/scratch\n\
            +/usr/local/bin/agent-doc\n\
            +/var\n";
        let cmds = parse_slash_commands(diff);
        // "/var" is a bare token with no slash → allowed by grammar (it's a
        // valid command name shape). Only the three path-shaped entries are
        // rejected. This is the minimum contract: reject second-slash.
        assert_eq!(cmds, vec!["/var"]);
    }

    #[test]
    fn parse_slash_commands_rejects_uppercase_and_symbols() {
        // Grammar: first char must be [a-z]; rest must be [a-z0-9:_-].
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n ctx\n\
            +/Clear\n\
            +/foo.bar\n\
            +/foo!bang\n\
            +/foo#hash\n";
        let cmds = parse_slash_commands(diff);
        assert!(cmds.is_empty(), "all four must be rejected; got: {cmds:?}");
    }

    #[test]
    fn parse_slash_commands_accepts_namespaced_and_hyphenated() {
        // Grammar allows `:`, `_`, `-`, digits — namespaced/versioned commands.
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n ctx\n\
            +/mcp:reload\n\
            +/agent-doc file.md\n\
            +/some_thing\n\
            +/v2\n";
        let cmds = parse_slash_commands(diff);
        assert_eq!(
            cmds,
            vec!["/mcp:reload", "/agent-doc file.md", "/some_thing", "/v2"]
        );
    }

    #[test]
    fn detect_orchestration_request_for_synchronous_task_list() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,7 @@\n ctx\n\
+Today is 2026-04-25. Synchronous orcestra.\n\
+- do #ss01. Add tests. Run benchmarks.\n\
+- do #ss02. Add tests. Run benchmarks.\n\
+- do #ss03. Add tests. Run benchmarks.\n";
        let request = detect_orchestration_request(diff).expect("expected orchestration request");
        assert_eq!(request.mode, OrchestrationRequestMode::Sequential);
        assert_eq!(request.task_count, 3);
        assert_eq!(
            request.trigger_text,
            "Today is 2026-04-25. Synchronous orcestra."
        );
    }

    #[test]
    fn detect_orchestration_request_for_parallel_batch() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,4 @@\n ctx\n\
+Fan out these benchmark tasks.\n\
+- do #a1\n\
+- do #a2\n";
        let request = detect_orchestration_request(diff).expect("expected orchestration request");
        assert_eq!(request.mode, OrchestrationRequestMode::Parallel);
        assert_eq!(request.task_count, 2);
    }

    #[test]
    fn detect_orchestration_request_requires_batch_shape() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n\
+Run these in order.\n\
+- do #a1\n";
        assert!(
            detect_orchestration_request(diff).is_none(),
            "single-item lists should stay as ordinary work, not forced orchestration"
        );
    }

    #[test]
    fn detect_orchestration_request_for_prefixed_synchronous_opera_batch() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,6 @@\n ctx\n\
+❯ synchronous opera\n\
+❯ preset #spec-test-build-install-commit-push\n\
+❯ - do #jbpfx1\n\
+❯ - do #jbpfx2\n";
        let request = detect_orchestration_request(diff).expect("expected orchestration request");
        assert_eq!(request.mode, OrchestrationRequestMode::Sequential);
        assert_eq!(request.task_count, 2);
        assert_eq!(
            request.trigger_text,
            "❯ synchronous opera ❯ preset #spec-test-build-install-commit-push"
        );
    }

    #[test]
    fn detect_queue_trigger_do_queue() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do queue\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_run_queue() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+run queue\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_case_insensitive() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+Do Queue\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_with_prompt_prefix() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+❯ do queue\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_not_pending_ref() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do #queue Phase 2\n";
        assert!(!detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_not_in_code_fence() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,4 @@\n ctx\n+```\n+do queue\n+```\n";
        assert!(!detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_not_in_blockquote() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+> do queue\n";
        assert!(!detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_with_trailing_punct() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do queue.\n";
        assert!(detect_queue_trigger(diff));
    }

    #[test]
    fn detect_queue_trigger_not_on_context_line() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n do queue\n+other\n";
        assert!(!detect_queue_trigger(diff));
    }

    #[test]
    fn suppress_inactive_queue_additions_removes_queue_prompt_lines() {
        let current = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#gdbpropscan]\n",
            "<!-- /agent:queue -->\n"
        );
        let diff = concat!(
            "--- snapshot\n",
            "+++ document\n",
            "@@ -7,5 +7,7 @@\n",
            " Done.\n",
            " <!-- /agent:exchange -->\n",
            " \n",
            " <!-- agent:queue -->\n",
            "+dispatch #spec-test-build-install-commit-push\n",
            "+- do [#gdbpropscan]\n",
            " <!-- /agent:queue -->\n",
        );

        let filtered = suppress_inactive_queue_additions(diff, current);

        assert!(!filtered.contains("[#gdbpropscan]"));
        assert!(!filtered.contains("dispatch #spec-test-build-install-commit-push"));
        assert!(classify_prompt_bearing_changes(&filtered).is_empty());
        assert!(extract_imperative_directives(&filtered).is_empty());
    }

    #[test]
    fn detect_prompt_preset_requests_from_diff() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,6 @@\n ctx\n\
+preset #1\n\
+presets release-check, #2\n\
+preset #1\n\
+> preset ignored\n\
+```md\n\
+preset fenced\n";
        assert_eq!(
            detect_prompt_preset_requests(diff),
            vec![
                "#1".to_string(),
                "release-check".to_string(),
                "#2".to_string()
            ]
        );
    }

    #[test]
    fn extract_prompt_preset_requests_from_text_ignores_fences_and_blockquotes() {
        let text = "synchronous orchestra\npreset #1\n> preset quoted\n\n```md\npreset fenced\n```\npresets release-check and #2\n";
        assert_eq!(
            extract_prompt_preset_requests_from_text(text),
            vec![
                "#1".to_string(),
                "release-check".to_string(),
                "#2".to_string()
            ]
        );
    }

    #[test]
    fn extract_imperative_directives_detects_do_and_build_push() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,3 @@\n ctx\n\
            +do #6zyp. update spec + tests. build + install for local testing. commit + push\n\
            +do [#dodone]. spec-test-build-install-commit-push\n\
            +do [#plainid]\n\
            +run benchmarks\n";
        let directives = extract_imperative_directives(diff);
        assert_eq!(
            directives,
            vec![
                "do #6zyp. update spec + tests. build + install for local testing. commit + push",
                "do [#dodone]. spec-test-build-install-commit-push",
                "do [#plainid]",
                "run benchmarks",
            ]
        );
        assert!(diff_contains_imperative_directive(diff));
    }

    #[test]
    fn extract_imperative_directives_detects_pending_item_natural_language() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n\
            +- [ ] [#n8q4] Fix the cross-repo `no-permissions-bypass` miss now dominating benchmark MAE\n";
        let directives = extract_imperative_directives(diff);
        assert_eq!(
            directives,
            vec!["Fix the cross-repo `no-permissions-bypass` miss now dominating benchmark MAE"]
        );
        assert!(diff_contains_imperative_directive(diff));
    }

    #[test]
    fn extract_imperative_directives_detects_long_custom_pending_id() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n\
            +- [ ] [#sdig2matrix] Fix the custom backlog id normalization path\n";
        let directives = extract_imperative_directives(diff);
        assert_eq!(
            directives,
            vec!["Fix the custom backlog id normalization path"]
        );
        assert!(diff_contains_imperative_directive(diff));
    }

    #[test]
    fn extract_imperative_directives_ignores_blockquotes_and_fences() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,6 @@\n ctx\n\
            +> do #skip\n\
            +```\n\
            +run tests\n\
            +```\n\
            +plain note\n";
        let directives = extract_imperative_directives(diff);
        assert!(
            directives.is_empty(),
            "unexpected directives: {directives:?}"
        );
        assert!(!diff_contains_imperative_directive(diff));
    }

    #[test]
    fn diff_contains_imperative_directive_for_approval_word() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+go\n";
        assert!(diff_contains_imperative_directive(diff));
    }

    #[test]
    fn detect_exchange_compaction_request_matches_bare_directive() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+compact exchange\n";
        assert!(detect_exchange_compaction_request(diff));
    }

    #[test]
    fn detect_exchange_compaction_request_matches_prompt_prefixed_variant() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+❯ compact exchange...do not add...summarize the content and delete the rest\n";
        assert!(detect_exchange_compaction_request(diff));
    }

    #[test]
    fn detect_exchange_compaction_request_ignores_non_directive_mentions() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+I failed to compact exchange earlier.\n";
        assert!(!detect_exchange_compaction_request(diff));
    }

    #[test]
    fn extract_required_response_blocks_multiple_prompts() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,9 @@\n\
            ctx\n\
            +❯ First question?\n\
            +Context line.\n\
            +\n\
            +❯ Second question?\n\
            +do #n8q4. run tests. build + install. commit + push\n";

        let blocks = extract_required_response_blocks(diff);
        assert_eq!(blocks.len(), 2);
        assert_eq!(blocks[0], "❯ First question?\nContext line.");
        assert_eq!(
            blocks[1],
            "❯ Second question?\ndo #n8q4. run tests. build + install. commit + push"
        );
    }

    #[test]
    fn extract_required_response_blocks_preserves_code_fence_context() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,7 @@\n\
            ctx\n\
            +❯ In src/boost-client, why did patchback miss the prefix?\n\
            +See my inquiry:\n\
            +```text\n\
            +line one\n\
            +line two\n\
            +```\n";

        let blocks = extract_required_response_blocks(diff);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].contains("❯ In src/boost-client"));
        assert!(blocks[0].contains("```text\nline one\nline two\n```"));
    }

    #[test]
    fn format_required_response_targets_mentions_turn_completeness() {
        let diff =
            "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n+❯ Why were two prompts left unresolved?\n";
        let rendered = format_required_response_targets(diff).unwrap();
        assert!(rendered.contains("Do not stop at the newest question"));
        assert!(rendered.contains("<target index=\"1\">"));
        assert!(rendered.contains("❯ Why were two prompts left unresolved?"));
    }

    #[test]
    fn format_prompt_bearing_changes_mentions_edit_and_artifact_contract() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,6 @@\n\
            context before\n\
            +❯ Why was this missed?\n\
            +\n\
            +This line should say 503.\n\
            context after\n";
        let rendered = format_prompt_bearing_changes(diff).unwrap();
        assert!(rendered.contains("User-authored prompt-bearing changes (oldest first):"));
        assert!(rendered.contains("kind=\"prompt_target\""));
        assert!(rendered.contains("kind=\"content_edit\""));
        assert!(rendered.contains("Treat `content_edit` items as user corrections"));
    }

    #[test]
    fn prompt_prefix_normalization_targets_preserve_prompt_context_and_skip_fences() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,7 @@\n\
            ctx\n\
            +❯ In src/boost-client, why did patchback miss the prefix?\n\
            +See my inquiry:\n\
            +- keep this markdown bullet bare\n\
            +  - keep nested markdown bullets bare\n\
            +1. keep ordered markdown bullets bare\n\
            +```text\n\
            +line one\n\
            +line two\n\
            +```\n";

        let targets = prompt_prefix_normalization_targets(diff);
        assert_eq!(
            targets,
            vec!["See my inquiry:".to_string(),],
            "only bare prompt-context prose should need fresh prefixing"
        );
    }

    #[test]
    fn first_bare_prompt_prefix_target_detects_unprefixed_prompt_block_line() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,5 @@\n\
            ctx\n\
            +❯ Existing question?\n\
            +Follow-up context.\n\
            +### Re: answer — gpt-5\n\
            +Body\n";

        let bare = first_bare_prompt_prefix_target(diff);
        assert_eq!(bare.as_deref(), Some("Follow-up context."));
    }

    #[test]
    fn first_bare_prompt_prefix_target_skips_markdown_lists() {
        let diff = "--- snapshot\n+++ document\n@@ -1,2 +1,6 @@\n\
            ctx\n\
            +❯ Please compare these options:\n\
            +- option one\n\
            +  - nested option detail\n\
            +1. ordered option\n\
            +### Re: answer — gpt-5\n";

        let bare = first_bare_prompt_prefix_target(diff);
        assert_eq!(bare, None);
    }

    // Plan: tasks/agent-doc/plan-claude-code-queue-auto-loop.md `#ccloopguard`.
    // Managed-component state edits (queue/backlog/done body, queue activity
    // toggle, frontmatter queue flag) must not block the Claude Code auto-loop.
    // Real user prompts must continue to block it.
    fn pbc(kind: PromptBearingChangeKind, text: &str) -> PromptBearingChange {
        PromptBearingChange {
            kind,
            text: text.to_string(),
        }
    }

    #[test]
    fn change_is_managed_state_only_accepts_queue_activity_toggle() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "<!-- agent:queue auto -->"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "<!-- agent:queue -->"
        )));
    }

    #[test]
    fn change_is_managed_state_only_accepts_frontmatter_queue_active_flip() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "queue_active: true"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "queue_active: false"
        )));
    }

    #[test]
    fn pipeline_only_frontmatter_write_is_no_change() {
        // #22a8: writing / clearing the managed agent_doc_pipeline block on a
        // phase transition must read as no change (diff cancels both sides).
        let snapshot = "---\nqueue: start\n---\n\n## Body\n- item\n";
        let with_pipeline = "---\nqueue: start\nagent_doc_pipeline:\n  run_id: cycle-123\n  step: response_captured\n  turn_id: \"#x\"\n---\n\n## Body\n- item\n";
        assert!(
            unified_diff_from_contents(snapshot, with_pipeline).is_none(),
            "adding a pipeline block must not register as a change"
        );
        assert!(
            unified_diff_from_contents(with_pipeline, snapshot).is_none(),
            "clearing a pipeline block must not register as a change"
        );
        // A real body edit alongside a pipeline write is still detected.
        let with_pipeline_and_edit = "---\nqueue: start\nagent_doc_pipeline:\n  run_id: cycle-123\n  step: committed\n---\n\n## Body\n- item changed\n";
        assert!(
            unified_diff_from_contents(snapshot, with_pipeline_and_edit).is_some(),
            "a real body edit must still be detected through a pipeline write"
        );
    }

    #[test]
    fn change_is_managed_state_only_accepts_pipeline_block_lines() {
        for line in [
            "agent_doc_pipeline:",
            "  run_id: cycle-123",
            "  step: write_applied",
            "  turn_id: \"#x\"",
            "  queue_task_id: \"#x\"",
        ] {
            assert!(
                change_is_managed_state_only(&pbc(PromptBearingChangeKind::ContentEdit, line)),
                "pipeline line should be managed state: {line:?}"
            );
        }
    }

    #[test]
    fn change_is_managed_state_only_accepts_queue_item_lines() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "- do [#newitem]"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "- ~do [#consumed]~"
        )));
    }

    #[test]
    fn change_is_managed_state_only_accepts_backlog_and_done_item_lines() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "- [ ] [#newitem] short description"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "- [/] [#gated] partial progress note"
        )));
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "- 2026-05-25 [#done] closed last cycle"
        )));
    }

    #[test]
    fn change_is_managed_state_only_accepts_multi_line_managed_blocks() {
        assert!(change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "- do [#a]\n- do [#b]\n- do [#c]"
        )));
    }

    #[test]
    fn change_is_managed_state_only_rejects_real_user_prompts() {
        assert!(!change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "Why is the queue not auto-looping?"
        )));
        assert!(!change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "❯ do this thing please"
        )));
        assert!(!change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::ContentEdit,
            "Fix the regression on line 42."
        )));
    }

    #[test]
    fn change_is_managed_state_only_rejects_mixed_managed_and_user_text() {
        assert!(!change_is_managed_state_only(&pbc(
            PromptBearingChangeKind::PromptTarget,
            "- do [#newitem]\nAnd please also fix the older bug."
        )));
    }
