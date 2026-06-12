    use super::*;

    #[test]
    fn single_range() {
        let doc = "before\n<!-- agent:status -->\nHello\n<!-- /agent:status -->\nafter\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "status");
        assert_eq!(ranges[0].content(doc), "Hello\n");
    }

    #[test]
    fn nested_ranges() {
        let doc = "\
<!-- agent:outer -->
<!-- agent:inner -->
content
<!-- /agent:inner -->
<!-- /agent:outer -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 2);
        // Sorted by open_start — outer first
        assert_eq!(ranges[0].name, "outer");
        assert_eq!(ranges[1].name, "inner");
        assert_eq!(ranges[1].content(doc), "content\n");
    }

    #[test]
    fn siblings() {
        let doc = "\
<!-- agent:a -->
alpha
<!-- /agent:a -->
<!-- agent:b -->
beta
<!-- /agent:b -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 2);
        assert_eq!(ranges[0].name, "a");
        assert_eq!(ranges[0].content(doc), "alpha\n");
        assert_eq!(ranges[1].name, "b");
        assert_eq!(ranges[1].content(doc), "beta\n");
    }

    #[test]
    fn no_ranges() {
        let doc = "# Just a document\n\nWith no range templates.\n";
        let ranges = parse(doc).unwrap();
        assert!(ranges.is_empty());
    }

    #[test]
    fn unmatched_open_error() {
        let doc = "<!-- agent:orphan -->\nContent\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("unclosed component"));
    }

    #[test]
    fn unmatched_close_error() {
        let doc = "Content\n<!-- /agent:orphan -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("without matching open"));
    }

    #[test]
    fn mismatched_names_error() {
        let doc = "<!-- agent:foo -->\n<!-- /agent:bar -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("mismatched"));
    }

    #[test]
    fn invalid_name() {
        let doc = "<!-- agent:-bad -->\n<!-- /agent:-bad -->\n";
        let err = parse(doc).unwrap_err();
        assert!(err.to_string().contains("invalid component name"));
    }

    #[test]
    fn name_validation() {
        assert!(is_valid_name("status"));
        assert!(is_valid_name("my-section"));
        assert!(is_valid_name("a1"));
        assert!(is_valid_name("A"));
        assert!(!is_valid_name(""));
        assert!(!is_valid_name("-bad"));
        assert!(!is_valid_name("has space"));
        assert!(!is_valid_name("has_underscore"));
    }

    #[test]
    fn content_extraction() {
        let doc = "<!-- agent:x -->\nfoo\nbar\n<!-- /agent:x -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges[0].content(doc), "foo\nbar\n");
    }

    #[test]
    fn replace_roundtrip() {
        let doc = "before\n<!-- agent:s -->\nold\n<!-- /agent:s -->\nafter\n";
        let ranges = parse(doc).unwrap();
        let new_doc = ranges[0].replace_content(doc, "new\n");
        assert_eq!(
            new_doc,
            "before\n<!-- agent:s -->\nnew\n<!-- /agent:s -->\nafter\n"
        );
        // Re-parse should work
        let ranges2 = parse(&new_doc).unwrap();
        assert_eq!(ranges2.len(), 1);
        assert_eq!(ranges2[0].content(&new_doc), "new\n");
    }

    #[test]
    fn is_agent_marker_yes() {
        assert!(is_agent_marker(" agent:status "));
        assert!(is_agent_marker("/agent:status"));
        assert!(is_agent_marker("agent:my-thing"));
        assert!(is_agent_marker(" /agent:A1 "));
    }

    #[test]
    fn is_agent_marker_no() {
        assert!(!is_agent_marker("just a comment"));
        assert!(!is_agent_marker("agent:"));
        assert!(!is_agent_marker("/agent:"));
        assert!(!is_agent_marker("agent:-bad"));
        assert!(!is_agent_marker("some agent:fake stuff"));
    }

    #[test]
    fn regular_comments_ignored() {
        let doc = "<!-- just a comment -->\n<!-- agent:x -->\ndata\n<!-- /agent:x -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn regular_comments_with_multibyte_preview_boundary_ignored() {
        let doc = "\
<!-- 123456789012345678❯ ordinary comment -->
<!-- agent:x -->
data
<!-- /agent:x -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn multiline_comment_ignored() {
        let doc = "\
<!--
multi
line
comment
-->
<!-- agent:s -->
content
<!-- /agent:s -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "s");
    }

    #[test]
    fn empty_content() {
        let doc = "<!-- agent:empty --><!-- /agent:empty -->\n";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].content(doc), "");
    }

    #[test]
    fn markers_in_fenced_code_block_ignored() {
        let doc = "\
<!-- agent:real -->
content
<!-- /agent:real -->
```markdown
<!-- agent:fake -->
this is just an example
<!-- /agent:fake -->
```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "real");
    }

    #[test]
    fn markers_in_inline_code_ignored() {
        let doc = "\
Use `<!-- agent:example -->` markers for components.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "real");
    }

    #[test]
    fn markers_in_tilde_fence_ignored() {
        let doc = "\
<!-- agent:x -->
data
<!-- /agent:x -->
~~~
<!-- agent:y -->
example
<!-- /agent:y -->
~~~
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn markers_in_indented_fenced_code_block_ignored() {
        // CommonMark allows up to 3 spaces before fence opener
        let doc = "\
<!-- agent:exchange -->
Content here.
<!-- /agent:exchange -->

  ```markdown
  <!-- agent:fake -->
  demo without closing tag
  ```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "exchange");
    }

    #[test]
    fn indented_fence_inside_component_ignored() {
        // Indented code block inside a component should not cause mismatched errors
        let doc = "\
<!-- agent:exchange -->
Here's how to set up:

   ```markdown
   <!-- agent:status -->
   Your status here
   ```

Done explaining.
<!-- /agent:exchange -->
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "exchange");
    }

    #[test]
    fn deeply_indented_fence_ignored() {
        // Tabs and many spaces should still be detected as a fence
        let doc = "\
<!-- agent:x -->
ok
<!-- /agent:x -->
      ```
      <!-- agent:y -->
      inside fence
      ```
";
        let ranges = parse(doc).unwrap();
        assert_eq!(ranges.len(), 1);
        assert_eq!(ranges[0].name, "x");
    }

    #[test]
    fn indented_fence_code_ranges_detected() {
        let doc = "before\n  ```\n  code\n  ```\nafter\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        assert!(doc[ranges[0].0..ranges[0].1].contains("code"));
    }

    #[test]
    fn code_ranges_detected() {
        let doc = "before\n```\ncode\n```\nafter `inline` end\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 2);
        // Fenced block
        assert!(doc[ranges[0].0..ranges[0].1].contains("code"));
        // Inline span
        assert!(doc[ranges[1].0..ranges[1].1].contains("inline"));
    }

    #[test]
    fn code_ranges_double_backtick() {
        // CommonMark: `` `<!--` `` is a code span containing `<!--`
        let doc = "text `` `<!--` `` more\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let span = &doc[ranges[0].0..ranges[0].1];
        assert!(
            span.contains("<!--"),
            "double-backtick span should contain <!--: {:?}",
            span
        );
    }

    #[test]
    fn code_ranges_double_backtick_does_not_match_single() {
        // `` should not match a single ` close
        let doc = "text `` foo ` bar `` end\n";
        let ranges = find_code_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let span = &doc[ranges[0].0..ranges[0].1];
        assert_eq!(span, "`` foo ` bar ``");
    }

    #[test]
    fn double_backtick_comment_before_agent_marker() {
        // Regression: `` `<!--` `` followed by agent marker should not confuse the parser
        let doc = "\
<!-- agent:exchange -->\n\
text `` `<!--` `` description\n\
new content here\n\
<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert!(components[0].content(doc).contains("new content here"));
    }

    // --- Inline attribute tests ---

    #[test]
    fn parse_component_with_mode_attr() {
        let doc = "<!-- agent:exchange mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert_eq!(
            components[0].attrs.get("mode").map(|s| s.as_str()),
            Some("append")
        );
        assert_eq!(components[0].content(doc), "Content\n");
    }

    #[test]
    fn parse_component_with_multiple_attrs() {
        let doc = "<!-- agent:log mode=prepend timestamp=true -->\nData\n<!-- /agent:log -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "log");
        assert_eq!(
            components[0].attrs.get("mode").map(|s| s.as_str()),
            Some("prepend")
        );
        assert_eq!(
            components[0].attrs.get("timestamp").map(|s| s.as_str()),
            Some("true")
        );
    }

    #[test]
    fn parse_component_no_attrs_backward_compat() {
        let doc = "<!-- agent:status -->\nOK\n<!-- /agent:status -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "status");
        assert!(components[0].attrs.is_empty());
    }

    #[test]
    fn is_agent_marker_with_attrs() {
        assert!(is_agent_marker(" agent:exchange mode=append "));
        assert!(is_agent_marker("agent:status mode=replace"));
        assert!(is_agent_marker("agent:log mode=prepend timestamp=true"));
    }

    #[test]
    fn closing_tag_unchanged_with_attrs() {
        // Closing tags never have attributes
        let doc = "<!-- agent:status mode=replace -->\n- [x] Done\n<!-- /agent:status -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        let new_doc = components[0].replace_content(doc, "- [ ] Todo\n");
        assert!(new_doc.contains("<!-- agent:status mode=replace -->"));
        assert!(new_doc.contains("<!-- /agent:status -->"));
        assert!(new_doc.contains("- [ ] Todo"));
    }

    #[test]
    fn parse_component_with_patch_attr() {
        let doc = "<!-- agent:exchange patch=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "exchange");
        assert_eq!(components[0].patch_mode(), Some("append"));
        assert_eq!(components[0].content(doc), "Content\n");
    }

    #[test]
    fn patch_attr_takes_precedence_over_mode() {
        let doc = "<!-- agent:exchange patch=replace mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), Some("replace"));
    }

    #[test]
    fn mode_attr_backward_compat() {
        let doc = "<!-- agent:exchange mode=append -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), Some("append"));
    }

    #[test]
    fn no_patch_or_mode_attr() {
        let doc = "<!-- agent:exchange -->\nContent\n<!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        assert_eq!(components[0].patch_mode(), None);
    }

    // --- Inline backtick code span exclusion tests ---

    #[test]
    fn single_backtick_component_tag_ignored() {
        // A component tag wrapped in single backticks should not be parsed
        let doc = "\
Use `<!-- agent:pending -->` to mark pending sections.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
    }

    #[test]
    fn double_backtick_component_tag_ignored() {
        // A component tag wrapped in double backticks should not be parsed
        let doc = "\
Use ``<!-- agent:pending -->`` to mark pending sections.
<!-- agent:real -->
content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
    }

    #[test]
    fn component_tags_not_in_backticks_still_work() {
        // Tags outside of any backticks are parsed normally
        let doc = "\
<!-- agent:a -->
alpha
<!-- /agent:a -->
<!-- agent:b patch=append -->
beta
<!-- /agent:b -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 2);
        assert_eq!(components[0].name, "a");
        assert_eq!(components[1].name, "b");
        assert_eq!(components[1].patch_mode(), Some("append"));
    }

    #[test]
    fn mixed_backtick_and_real_tags() {
        // Some tags in backticks (ignored), some not (parsed)
        let doc = "\
Here is an example: `<!-- agent:fake -->` and ``<!-- /agent:fake -->``.
<!-- agent:real -->
real content
<!-- /agent:real -->
Another example: `<!-- agent:also-fake patch=replace -->` is just documentation.
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
        assert_eq!(components[0].content(doc), "real content\n");
    }

    #[test]
    fn inline_code_mid_line_with_surrounding_text_ignored() {
        // Edge case: component tag inside inline code span on a line with other content
        // before and after — must not be parsed as a real component marker.
        let doc = "\
Wrap markers like `<!-- agent:status -->` in backticks to show them literally.
<!-- agent:real -->
actual content
<!-- /agent:real -->
";
        let components = parse(doc).unwrap();
        assert_eq!(components.len(), 1);
        assert_eq!(components[0].name, "real");
        assert_eq!(components[0].content(doc), "actual content\n");
    }

    #[test]
    fn parse_attrs_unit() {
        let attrs = parse_attrs("mode=append");
        assert_eq!(attrs.get("mode").map(|s| s.as_str()), Some("append"));

        let attrs = parse_attrs("mode=replace timestamp=true");
        assert_eq!(attrs.len(), 2);

        let attrs = parse_attrs("");
        assert!(attrs.is_empty());

        // Bare tokens (no =) are parsed as boolean flags with empty string values
        let attrs = parse_attrs("mode=append broken novalue=");
        assert_eq!(attrs.len(), 2);
        assert_eq!(attrs.get("mode").map(|s| s.as_str()), Some("append"));
        assert_eq!(attrs.get("broken").map(|s| s.as_str()), Some(""));

        // auto flag (used by agent:queue)
        let attrs = parse_attrs("auto");
        assert_eq!(attrs.len(), 1);
        assert!(attrs.contains_key("auto"));
    }

    #[test]
    fn append_with_boundary_skips_code_block() {
        // Boundary marker inside a code block should be ignored;
        // the real marker outside should be used.
        let boundary_id = "real-uuid";
        let doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             user prompt\n\
             ```\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             ```\n\
             more user text\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        let components = parse(&doc).unwrap();
        let comp = &components[0];
        let result =
            comp.append_with_boundary(&doc, "### Re: Response\n\nContent here.", boundary_id);

        // Response should replace the REAL marker (outside code block),
        // not the one inside the code block.
        assert!(result.contains("### Re: Response"));
        assert!(result.contains("more user text"));
        // The code block example should be preserved
        assert!(result.contains(&format!("<!-- agent:boundary:{boundary_id} -->\n```")));
        // The real marker should be consumed (replaced by response)
        assert!(!result.contains(&format!(
            "more user text\n<!-- agent:boundary:{boundary_id} -->\n<!-- /agent:exchange -->"
        )));
    }

    #[test]
    fn append_with_boundary_no_code_block() {
        // Normal case: boundary marker not in a code block
        let boundary_id = "simple-uuid";
        let doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             user prompt\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        let components = parse(&doc).unwrap();
        let comp = &components[0];
        let result = comp.append_with_boundary(&doc, "### Re: Answer\n\nDone.", boundary_id);

        assert!(result.contains("### Re: Answer"));
        assert!(result.contains("user prompt"));
        // Original marker should be consumed, but a NEW boundary re-inserted
        assert!(!result.contains(&format!("agent:boundary:{boundary_id}")));
        assert!(result.contains("agent:boundary:"));
    }

    #[test]
    fn append_with_boundary_skips_already_present_content() {
        let boundary_id = "simple-uuid";
        let doc = format!(
            "<!-- agent:exchange patch=append -->\n\
             ### Re: Duplicate — gpt-5 (HEAD)\n\
             \n\
             Already applied.\n\
             <!-- agent:boundary:old-id -->\n\
             <!-- agent:boundary:{boundary_id} -->\n\
             <!-- /agent:exchange -->\n"
        );
        let components = parse(&doc).unwrap();
        let comp = &components[0];
        let result = comp.append_with_boundary(
            &doc,
            "### Re: Duplicate — gpt-5\n\nAlready applied.\n",
            boundary_id,
        );

        assert_eq!(result, doc);
    }

    #[test]
    fn append_with_caret_skips_already_present_content() {
        let doc = "<!-- agent:exchange patch=append -->\n\
                   User prompt.\n\
                   ### Re: Duplicate — gpt-5\n\
                   \n\
                   Already applied.\n\
                   <!-- /agent:exchange -->\n";
        let components = parse(doc).unwrap();
        let comp = &components[0];
        let result =
            comp.append_with_caret(doc, "### Re: Duplicate — gpt-5\n\nAlready applied.\n", None);

        assert_eq!(result, doc);
    }

    // --- strip_comments tests (moved from diff.rs) ---

    #[test]
    fn strip_comments_removes_html_comment() {
        let result = strip_comments("before\n<!-- a comment -->\nafter\n");
        assert_eq!(result, "before\nafter\n");
    }

    #[test]
    fn non_agent_html_comment_ranges_cover_multiline_body() {
        let doc = concat!(
            "before\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "-->\n",
            "<!-- agent:exchange -->\n",
            "<!-- /agent:exchange -->\n"
        );

        let ranges = find_non_agent_html_comment_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let hidden = doc.find("do #hidden").unwrap();
        assert!(
            ranges
                .iter()
                .any(|&(start, end)| hidden >= start && hidden < end),
            "ordinary comment body should be inside a non-agent comment range"
        );
        assert!(
            ranges
                .iter()
                .all(|&(start, end)| !doc[start..end].contains("agent:exchange")),
            "agent component markers must not be treated as ordinary comments"
        );
    }

    #[test]
    fn non_agent_html_comment_ranges_cover_unterminated_tail() {
        let doc = concat!(
            "before\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "still typing\n"
        );

        let ranges = find_non_agent_html_comment_ranges(doc);
        assert_eq!(ranges.len(), 1);
        let (start, end) = ranges[0];
        assert_eq!(
            &doc[start..end],
            "<!--\ndo #hidden. spec-test-build-install-commit-push\nstill typing\n"
        );
        assert_eq!(end, doc.len());
    }

    #[test]
    fn strip_comments_preserves_agent_markers() {
        let input = "text\n<!-- agent:status -->\ncontent\n<!-- /agent:status -->\n";
        let result = strip_comments(input);
        assert!(result.contains("<!-- agent:status -->"));
        assert!(result.contains("<!-- /agent:status -->"));
    }

    #[test]
    fn strip_comments_removes_link_ref() {
        let result = strip_comments("[//]: # (hidden note)\nvisible\n");
        assert_eq!(result, "visible\n");
    }

    #[test]
    fn html_comment_in_content_does_not_eat_close_marker() {
        let doc = "\
<!-- agent:pending -->
- [ ] [#abc] Rule: first line (not starting with ### or ❯ or <!-- ) gets prefix
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "pending");
        assert!(comps[0].content(doc).contains("<!-- )"));
    }

    #[test]
    fn nested_html_comment_like_text_in_exchange() {
        let doc = "\
<!-- agent:exchange -->
User typed <!-- some note --> in the text.
<!-- /agent:exchange -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "exchange");
    }

    #[test]
    fn pending_item_with_literal_html_comment_opener() {
        // Regression: a pending item containing `<!-- ` (literal HTML comment start)
        // must not consume the `<!-- /agent:pending -->` close marker.
        let doc = "\
<!-- agent:pending -->
- [ ] [#r7hw] Component parser: non-agent `<!-- ` in content ate close markers
- [ ] [#xyz] Another item
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "pending");
        let content = comps[0].content(doc);
        assert!(content.contains("#r7hw"));
        assert!(content.contains("#xyz"));
    }

    #[test]
    fn multiple_non_agent_html_comments_in_pending() {
        // Multiple `<!-- ... -->` fragments that are NOT agent markers.
        let doc = "\
<!-- agent:pending -->
- [ ] [#a] Fix rule: skip lines starting with <!-- or -->
- [ ] [#b] Handle <!-- partial comment
- [ ] [#c] Normal item
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "pending");
        let content = comps[0].content(doc);
        assert!(content.contains("#a"));
        assert!(content.contains("#b"));
        assert!(content.contains("#c"));
    }

    #[test]
    fn exchange_and_pending_both_with_html_comments_in_content() {
        // Both components contain non-agent HTML comments — parser must handle
        // siblings correctly without eating across component boundaries.
        let doc = "\
<!-- agent:exchange -->
The rule checks for <!-- prefix before deciding.
### Re: topic — opus
Fix applied to skip non-agent <!-- sequences.
<!-- /agent:exchange -->

<!-- agent:pending -->
- [ ] [#cfdy] Verify <!-- in pending items
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 2);
        assert_eq!(comps[0].name, "exchange");
        assert_eq!(comps[1].name, "pending");
        assert!(comps[0].content(doc).contains("<!-- prefix"));
        assert!(comps[1].content(doc).contains("#cfdy"));
    }

    #[test]
    fn is_backlog_component_accepts_both_names() {
        assert!(is_backlog_component("backlog"));
        assert!(is_backlog_component("pending"));
        assert!(!is_backlog_component("exchange"));
        assert!(!is_backlog_component("status"));
        assert!(!is_backlog_component("pending-done"));
        assert!(!is_backlog_component("backlog-done"));
        assert!(!is_backlog_component("done"));
    }

    #[test]
    fn is_backlog_done_component_accepts_only_canonical_done() {
        assert!(is_backlog_done_component("done"));
        assert!(!is_backlog_done_component("backlog-done"));
        assert!(!is_backlog_done_component("pending-done"));
        assert!(!is_backlog_done_component("backlog"));
        assert!(!is_backlog_done_component("pending"));
        assert!(!is_backlog_done_component("icebox"));
    }

    #[test]
    fn is_review_component_accepts_name() {
        assert!(is_review_component("review"));
        assert!(!is_review_component("backlog"));
        assert!(!is_review_component("icebox"));
    }

    #[test]
    fn is_icebox_component_accepts_name() {
        assert!(is_icebox_component("icebox"));
        assert!(!is_icebox_component("backlog"));
        assert!(!is_icebox_component("exchange"));
        assert!(!is_icebox_component("icebox-archive"));
    }

    #[test]
    fn tracked_work_component_includes_review() {
        assert!(is_tracked_work_component("backlog"));
        assert!(is_tracked_work_component("pending"));
        assert!(is_tracked_work_component("review"));
        assert!(is_tracked_work_component("icebox"));
        assert!(!is_tracked_work_component("done"));
    }

    #[test]
    fn icebox_component_parsed() {
        let doc = "\
<!-- agent:icebox -->
- Parked idea
<!-- /agent:icebox -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "icebox");
        assert!(is_icebox_component(&comps[0].name));
        assert!(comps[0].content(doc).contains("Parked idea"));
    }

    #[test]
    fn backlog_component_parsed_from_new_marker() {
        let doc = "\
<!-- agent:backlog -->
- [ ] [#abc] First item
<!-- /agent:backlog -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "backlog");
        assert!(is_backlog_component(&comps[0].name));
        assert!(comps[0].content(doc).contains("#abc"));
    }

    #[test]
    fn legacy_pending_marker_still_parsed() {
        let doc = "\
<!-- agent:pending -->
- [ ] [#xyz] Legacy item
<!-- /agent:pending -->
";
        let comps = parse(doc).unwrap();
        assert_eq!(comps.len(), 1);
        assert_eq!(comps[0].name, "pending");
        assert!(is_backlog_component(&comps[0].name));
    }

    #[test]
    fn strip_backlog_patch_attr_removes_patch_replace() {
        let doc = "<!-- agent:backlog patch=replace -->\n- item\n<!-- /agent:backlog -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(
            result,
            "<!-- agent:backlog -->\n- item\n<!-- /agent:backlog -->\n"
        );
    }

    #[test]
    fn strip_backlog_patch_attr_removes_pending_patch_replace() {
        let doc = "<!-- agent:pending patch=replace -->\n- item\n<!-- /agent:pending -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(
            result,
            "<!-- agent:pending -->\n- item\n<!-- /agent:pending -->\n"
        );
    }

    #[test]
    fn strip_backlog_patch_attr_removes_mode_replace() {
        let doc = "<!-- agent:backlog mode=replace -->\n- item\n<!-- /agent:backlog -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(
            result,
            "<!-- agent:backlog -->\n- item\n<!-- /agent:backlog -->\n"
        );
    }

    #[test]
    fn strip_backlog_patch_attr_preserves_other_attrs() {
        let doc =
            "<!-- agent:backlog patch=replace max_lines=50 -->\n- item\n<!-- /agent:backlog -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(
            result,
            "<!-- agent:backlog max_lines=50 -->\n- item\n<!-- /agent:backlog -->\n"
        );
    }

    #[test]
    fn strip_backlog_patch_attr_noop_for_exchange() {
        let doc = "<!-- agent:exchange patch=append -->\ncontent\n<!-- /agent:exchange -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(result, doc);
    }

    #[test]
    fn strip_backlog_patch_attr_noop_when_no_attr() {
        let doc = "<!-- agent:backlog -->\n- item\n<!-- /agent:backlog -->\n";
        let result = strip_backlog_patch_attr(doc);
        assert_eq!(result, doc);
    }

    #[test]
    fn converge_queue_auto_strips_auto() {
        let doc = "<!-- agent:queue auto -->\n- do [#x]\n<!-- /agent:queue -->\n";
        let result = converge_queue_auto(doc, false).expect("tag changed");
        assert_eq!(
            result,
            "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n"
        );
    }

    #[test]
    fn converge_queue_auto_adds_auto() {
        let doc = "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n";
        let result = converge_queue_auto(doc, true).expect("tag changed");
        assert_eq!(
            result,
            "<!-- agent:queue auto -->\n- do [#x]\n<!-- /agent:queue -->\n"
        );
    }

    #[test]
    fn converge_queue_auto_preserves_other_attrs() {
        let doc = "<!-- agent:queue auto patch=append -->\n- do [#x]\n<!-- /agent:queue -->\n";
        let result = converge_queue_auto(doc, false).expect("tag changed");
        assert_eq!(
            result,
            "<!-- agent:queue patch=append -->\n- do [#x]\n<!-- /agent:queue -->\n"
        );
    }

    #[test]
    fn converge_queue_auto_normalizes_boolean_attrs_without_corrupting_preset() {
        let doc = concat!(
            "<!-- agent:queue auto=true priority=true preset=\"#spec-test-build-install-commit-push\"=true go=true -->\n",
            "- do [#x]\n",
            "<!-- /agent:queue -->\n",
        );
        let result = converge_queue_auto(doc, false).expect("tag normalized");
        assert_eq!(
            result,
            concat!(
                "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->\n",
                "- do [#x]\n",
                "<!-- /agent:queue -->\n",
            )
        );
    }

    #[test]
    fn converge_queue_auto_noop_when_already_matching() {
        let active = "<!-- agent:queue auto -->\n- do [#x]\n<!-- /agent:queue -->\n";
        assert_eq!(converge_queue_auto(active, true), None);
        let inactive = "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n";
        assert_eq!(converge_queue_auto(inactive, false), None);
    }

    #[test]
    fn converge_queue_auto_none_without_queue_component() {
        let doc = "<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n";
        assert_eq!(converge_queue_auto(doc, false), None);
    }

    // #prompt-duplicated-while-typing: dedup must be agnostic to the `❯ ` user-prompt
    // prefix, since a synthesized exchange patch and the live buffer can differ only
    // by it. Otherwise the prompt re-appends and duplicates while typing.
    #[test]
    fn append_patch_already_present_ignores_user_prompt_prefix() {
        assert!(
            append_patch_already_present("expand the section", "❯ expand the section"),
            "bare buffer vs prefixed patch must dedup"
        );
        assert!(
            append_patch_already_present("❯ expand the section", "expand the section"),
            "prefixed buffer vs bare patch must dedup"
        );
        assert!(
            append_patch_already_present("❯ expand the section", "❯ expand the section"),
            "both prefixed must dedup"
        );
    }

    #[test]
    fn append_patch_distinct_prompts_not_deduped() {
        assert!(
            !append_patch_already_present("❯ expand the section", "❯ summarize the section"),
            "genuinely distinct prompts must not be treated as duplicates"
        );
    }

    // #prompt-duplicated-while-typing (L1 structural buffer-snapshot race),
    // captured live in tasks/resume.md 2026-05-29: the synthesized patch held a
    // half-typed snapshot ("- [#s93y]: Add to") while the live buffer had been
    // typed further ("- [#s93y]: Add to resume"). The two regions are identical
    // except that one line; the old `contains` check broke at the divergence and
    // re-appended the entire tail. Dedup must recognize the snapshot relationship.
    #[test]
    fn append_patch_dedupes_midblock_typing_snapshot() {
        let live = concat!(
            "- [#yfg1]: I dropped the real-time asset-generation. Kept the investor demos.\n",
            "- [#bndq]: confirmed\n",
            "- do [#s75b]\n",
            "- do [#vapk]\n",
            "- [#s93y]: Add to resume\n",
            "- [#ca8w]\n",
            "Add MonsterRodHolders and EFS to the resume if not already added.",
        );
        let snapshot = concat!(
            "- [#yfg1]: I dropped the real-time asset-generation. Kept the investor demos.\n",
            "- [#bndq]: confirmed\n",
            "- do [#s75b]\n",
            "- do [#vapk]\n",
            "- [#s93y]: Add to\n",
            "- [#ca8w]\n",
            "Add MonsterRodHolders and EFS to the resume if not already added.",
        );
        assert!(
            append_patch_already_present(live, snapshot),
            "earlier keystroke snapshot must dedup against the live, further-typed region"
        );
    }

    #[test]
    fn append_patch_typing_snapshot_does_not_collapse_distinct_lines() {
        // A genuinely distinct prompt block (not a prefix snapshot) must still
        // append — the divergent line is not a prefix of the live line.
        let live = "- do task one\n- and the second thing\n- finally";
        let distinct = "- do task one\n- and a different thing\n- finally";
        assert!(
            !append_patch_already_present(live, distinct),
            "a mid-line replacement (not a prefix) is a distinct edit, not a snapshot"
        );
    }

    #[test]
    fn append_with_boundary_does_not_duplicate_typing_snapshot() {
        // The boundary stays high in the exchange; the live region below it has
        // been typed past the synthesized patch's snapshot. Appending the stale
        // snapshot must be a full no-op (document unchanged), not a duplication.
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — opus-4-8\n\n",
            "Answer.\n",
            "<!-- agent:boundary:70bccf9b -->\n",
            "- [#yfg1]: ok\n",
            "- [#s93y]: Add to resume\n",
            "- [#ca8w]\n",
            "Add MonsterRodHolders and EFS to the resume if not already added.\n",
            "<!-- /agent:exchange -->\n",
        );
        let components = parse(doc).unwrap();
        let exchange = components.iter().find(|c| c.name == "exchange").unwrap();
        let snapshot = concat!(
            "- [#yfg1]: ok\n",
            "- [#s93y]: Add to\n",
            "- [#ca8w]\n",
            "Add MonsterRodHolders and EFS to the resume if not already added.",
        );
        let result = exchange.append_with_boundary(doc, snapshot, "70bccf9b");
        assert_eq!(
            result, doc,
            "stale typing-snapshot append must be a no-op:\n{result}"
        );
        assert_eq!(
            result.matches("<!-- /agent:exchange -->").count(),
            1,
            "exchange close marker must not duplicate:\n{result}"
        );
    }

    #[test]
    fn append_with_caret_does_not_duplicate_prefixed_prompt() {
        // The live buffer already holds the bare typed prompt after the boundary; a
        // synthesized exchange patch carries the same prompt with the `❯ ` prefix.
        // Appending must be a no-op (single copy), not a duplicate.
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — opus-4-8\n\n",
            "Answer.\n",
            "<!-- agent:boundary:abc12345 -->\n",
            "expand the Confidential AI Startup section\n",
            "<!-- /agent:exchange -->\n",
        );
        let components = parse(doc).unwrap();
        let exchange = components.iter().find(|c| c.name == "exchange").unwrap();
        let result =
            exchange.append_with_caret(doc, "❯ expand the Confidential AI Startup section", None);
        assert_eq!(
            result
                .matches("expand the Confidential AI Startup section")
                .count(),
            1,
            "prefixed synthesized patch must not duplicate the already-typed prompt:\n{result}"
        );
    }
