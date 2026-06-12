    use super::*;
    use std::path::Path;
    use tempfile::TempDir;

    fn setup_project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        dir
    }

    /// Test helper: legacy `&Path` signature for `apply_patches`. Loads no
    /// configs (tests don't need them), derives summary from file stem.
    #[allow(dead_code)]
    fn apply_patches_via_path(
        doc: &str,
        patches: &[PatchBlock],
        unmatched: &str,
        file: &Path,
    ) -> Result<String> {
        let summary = file.file_stem().and_then(|s| s.to_str());
        let empty_str = std::collections::HashMap::new();
        let empty_usize = std::collections::HashMap::new();
        apply_patches_pure(doc, patches, unmatched, summary, &empty_str, &empty_usize)
    }

    /// Test helper: legacy `&Path` signature for `apply_patches_with_overrides`.
    #[allow(dead_code)]
    fn apply_patches_with_overrides_via_path(
        doc: &str,
        patches: &[PatchBlock],
        unmatched: &str,
        file: &Path,
        mode_overrides: &std::collections::HashMap<String, String>,
    ) -> Result<String> {
        let summary = file.file_stem().and_then(|s| s.to_str());
        let empty_str = std::collections::HashMap::new();
        let empty_usize = std::collections::HashMap::new();
        apply_patches_with_overrides_pure(
            doc,
            patches,
            unmatched,
            summary,
            &empty_str,
            &empty_usize,
            mode_overrides,
        )
    }

    #[test]
    fn parse_single_patch() {
        let response = "<!-- patch:status -->\nBuild passing.\n<!-- /patch:status -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "Build passing.\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_multiple_patches() {
        let response = "\
<!-- patch:status -->
All green.
<!-- /patch:status -->

<!-- patch:log -->
- New entry
<!-- /patch:log -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "All green.\n");
        assert_eq!(patches[1].name, "log");
        assert_eq!(patches[1].content, "- New entry\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_with_unmatched_content() {
        let response = "Some free text.\n\n<!-- patch:status -->\nOK\n<!-- /patch:status -->\n\nTrailing text.\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
        assert!(unmatched.contains("Some free text."));
        assert!(unmatched.contains("Trailing text."));
    }

    #[test]
    fn parse_empty_response() {
        let (patches, unmatched) = parse_patches("").unwrap();
        assert!(patches.is_empty());
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_no_patches() {
        let response = "Just a plain response with no patch blocks.";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert!(patches.is_empty());
        assert_eq!(unmatched, "Just a plain response with no patch blocks.");
    }

    #[test]
    fn apply_patches_replace() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("\nold\n"));
        assert!(result.contains("<!-- agent:status -->"));
    }

    #[test]
    fn apply_patches_unmatched_creates_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let result = apply_patches_via_path(doc, &[], "Extra info here", &doc_path).unwrap();
        assert!(result.contains("<!-- agent:exchange -->"));
        assert!(result.contains("Extra info here"));
        assert!(result.contains("<!-- /agent:exchange -->"));
    }

    #[test]
    fn apply_patches_unmatched_appends_to_existing_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:status -->\nok\n<!-- /agent:status -->\n\n<!-- agent:exchange -->\nprevious\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let result = apply_patches_via_path(doc, &[], "new stuff", &doc_path).unwrap();
        assert!(result.contains("previous"));
        assert!(result.contains("new stuff"));
        // Should not create a second exchange component
        assert_eq!(result.matches("<!-- agent:exchange -->").count(), 1);
    }

    #[test]
    fn apply_patches_missing_component_routes_to_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n\n<!-- agent:exchange -->\nprevious\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "nonexistent".to_string(),
            content: "overflow data\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        // Missing component content should be routed to exchange
        assert!(
            result.contains("overflow data"),
            "missing patch content should appear in exchange"
        );
        assert!(
            result.contains("previous"),
            "existing exchange content should be preserved"
        );
    }

    #[test]
    fn apply_patches_missing_component_creates_exchange() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "# Dashboard\n\n<!-- agent:status -->\nok\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "nonexistent".to_string(),
            content: "overflow data\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        // Should auto-create exchange component
        assert!(
            result.contains("<!-- agent:exchange -->"),
            "should create exchange component"
        );
        assert!(
            result.contains("overflow data"),
            "overflow content should be in exchange"
        );
    }

    #[test]
    fn is_template_mode_detection() {
        assert!(is_template_mode(Some("template")));
        assert!(!is_template_mode(Some("append")));
        assert!(!is_template_mode(None));
    }

    #[test]
    fn parse_patches_ignores_markers_in_fenced_code_block() {
        let response = "\
<!-- patch:exchange -->
Here is how you use component markers:

```markdown
<!-- agent:exchange -->
example content
<!-- /agent:exchange -->
```

<!-- /patch:exchange -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "exchange");
        assert!(patches[0].content.contains("```markdown"));
        assert!(patches[0].content.contains("<!-- agent:exchange -->"));
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_ignores_patch_markers_in_fenced_code_block() {
        // Patch markers inside a code block should not be treated as real patches
        let response = "\
<!-- patch:exchange -->
Real content here.

```markdown
<!-- patch:fake -->
This is just an example.
<!-- /patch:fake -->
```

<!-- /patch:exchange -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1, "should only find the outer real patch");
        assert_eq!(patches[0].name, "exchange");
        assert!(
            patches[0].content.contains("<!-- patch:fake -->"),
            "code block content should be preserved"
        );
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_ignores_markers_in_tilde_fence() {
        let response = "\
<!-- patch:status -->
OK
<!-- /patch:status -->

~~~
<!-- patch:fake -->
example
<!-- /patch:fake -->
~~~
";
        let (patches, _unmatched) = parse_patches(response).unwrap();
        // Only the real patch should be found; the fake one inside ~~~ is ignored
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "status");
    }

    #[test]
    fn parse_patches_ignores_closing_marker_in_code_block() {
        // The closing marker for a real patch is inside a code block,
        // so the parser should skip it and find the real closing marker outside
        let response = "\
<!-- patch:exchange -->
Example:

```
<!-- /patch:exchange -->
```

Real content continues.
<!-- /patch:exchange -->
";
        let (patches, _unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "exchange");
        assert!(patches[0].content.contains("Real content continues."));
    }

    #[test]
    fn parse_patches_normal_markers_still_work() {
        // Sanity check: normal patch parsing without code blocks still works
        let response = "\
<!-- patch:status -->
All systems go.
<!-- /patch:status -->
<!-- patch:log -->
- Entry 1
<!-- /patch:log -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[0].content, "All systems go.\n");
        assert_eq!(patches[1].name, "log");
        assert_eq!(patches[1].content, "- Entry 1\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_accepts_replace_icebox() {
        let response = "\
<!-- replace:icebox -->
- [ ] [#park1] Parked follow-up
<!-- /replace:icebox -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "icebox");
        assert_eq!(patches[0].content, "- [ ] [#park1] Parked follow-up\n");
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patches_orphaned_opener_does_not_leak_into_unmatched() {
        // Bug #p2xm: an unclosed `<!-- patch:exchange -->` was leaking into
        // unmatched text and getting appended to exchange verbatim.
        let response = "\
Some real content here.
<!-- patch:exchange -->
This opener has no matching close.
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert!(
            patches.is_empty(),
            "orphaned opener should not produce a patch"
        );
        assert_eq!(
            unmatched, "Some real content here.\nThis opener has no matching close.",
            "unmatched should contain text before and after the orphaned marker, but not the marker itself"
        );
    }

    #[test]
    fn parse_patches_orphaned_opener_between_valid_patches() {
        // Orphaned opener between two valid patches — only the valid ones parse,
        // text around the orphan becomes unmatched, marker itself is consumed.
        let response = "\
<!-- patch:status -->
All good.
<!-- /patch:status -->
Interstitial text.
<!-- patch:exchange -->
<!-- patch:log -->
- Log entry
<!-- /patch:log -->
";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 2);
        assert_eq!(patches[0].name, "status");
        assert_eq!(patches[1].name, "log");
        assert_eq!(unmatched, "Interstitial text.");
    }

    // --- Inline attribute mode resolution tests ---

    #[test]
    fn inline_attr_mode_overrides_config() {
        // Component has mode=replace inline, but config.toml says append
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        // Write config with append mode for status
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.status]\npatch = \"append\"\n",
        )
        .unwrap();
        // But the inline attr says replace
        let doc = "<!-- agent:status mode=replace -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        // Inline replace should win over config append
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn inline_attr_mode_overrides_default() {
        // exchange defaults to append, but inline says replace
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange mode=replace -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn no_inline_attr_no_config_falls_back_to_default() {
        // No inline attr, no config → built-in defaults
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        // exchange defaults to append
        assert!(result.contains("old\n"));
        assert!(result.contains("new\n"));
    }

    #[test]
    fn inline_patch_attr_overrides_config() {
        // Component has patch=replace inline, but config.toml says append
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.status]\npatch = \"append\"\n",
        )
        .unwrap();
        let doc = "<!-- agent:status patch=replace -->\nold\n<!-- /agent:status -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn inline_patch_attr_overrides_mode_attr() {
        // Both patch= and mode= present; patch= wins
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc =
            "<!-- agent:exchange patch=replace mode=append -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn stream_override_beats_inline_attr() {
        // Stream mode overrides should still beat inline attrs
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange mode=append -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "new\n".to_string(),
            attrs: Default::default(),
        }];
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("exchange".to_string(), "replace".to_string());
        let result =
            apply_patches_with_overrides_via_path(doc, &patches, "", &doc_path, &overrides)
                .unwrap();
        // Stream override (replace) should win over inline attr (append)
        assert!(result.contains("new\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn exchange_replace_override_replaces_unmatched_content() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange patch=append -->\nold\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let mut overrides = std::collections::HashMap::new();
        overrides.insert("exchange".to_string(), "replace".to_string());
        let result = apply_patches_with_overrides_via_path(
            doc,
            &[],
            "Compacted summary.\n",
            &doc_path,
            &overrides,
        )
        .unwrap();

        assert!(result.contains("Compacted summary.\n"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn exchange_replace_override_keeps_explicit_exchange_patch_authoritative() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:exchange patch=append -->\nold\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "Compacted summary.\n".to_string(),
            attrs: Default::default(),
        }];
        let mut overrides = std::collections::HashMap::new();
        overrides.insert("exchange".to_string(), "replace".to_string());
        let result = apply_patches_with_overrides_via_path(
            doc,
            &patches,
            "trailing note",
            &doc_path,
            &overrides,
        )
        .unwrap();

        assert!(result.contains("Compacted summary.\n"));
        assert!(result.contains("trailing note"));
        assert!(!result.contains("old\n"));
    }

    #[test]
    fn apply_patches_ignores_component_tags_in_code_blocks() {
        // Component tags inside a fenced code block should not be patch targets.
        // Only the real top-level component should receive the patch content.
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
# Scaffold Guide

Here is an example of a component:

```markdown
<!-- agent:status -->
example scaffold content
<!-- /agent:status -->
```

<!-- agent:status -->
real status content
<!-- /agent:status -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "status".to_string(),
            content: "patched status\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();

        // The real component should be patched
        assert!(
            result.contains("patched status\n"),
            "real component should receive the patch"
        );
        // The code block example should be untouched
        assert!(
            result.contains("example scaffold content"),
            "code block content should be preserved"
        );
        // The code block's markers should still be there
        assert!(
            result.contains("```markdown\n<!-- agent:status -->"),
            "code block markers should be preserved"
        );
    }

    #[test]
    fn unmatched_content_uses_boundary_marker() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n",
            "<!-- agent:exchange patch=append -->\n",
            "User prompt here.\n",
            "<!-- agent:boundary:test-uuid-123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        // No patch blocks — only unmatched content (simulates skill not wrapping in patch blocks)
        let patches = vec![];
        let unmatched = "### Re: Response\n\nResponse content here.\n";

        let result = apply_patches_via_path(doc, &patches, unmatched, &file).unwrap();

        // Response should be inserted at the boundary marker position (after prompt)
        let prompt_pos = result.find("User prompt here.").unwrap();
        let response_pos = result.find("### Re: Response").unwrap();
        assert!(
            response_pos > prompt_pos,
            "response should appear after the user prompt (boundary insertion)"
        );

        // Boundary marker should be consumed (replaced by response)
        assert!(
            !result.contains("test-uuid-123"),
            "boundary marker should be consumed after insertion"
        );
    }

    #[test]
    fn explicit_patch_uses_boundary_marker() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n",
            "<!-- agent:exchange patch=append -->\n",
            "User prompt here.\n",
            "<!-- agent:boundary:patch-uuid-456 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        // Explicit patch block targeting exchange
        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: Response\n\nResponse content.\n".to_string(),
            attrs: Default::default(),
        }];

        let result = apply_patches_via_path(doc, &patches, "", &file).unwrap();

        // Response should be after prompt (boundary consumed)
        let prompt_pos = result.find("User prompt here.").unwrap();
        let response_pos = result.find("### Re: Response").unwrap();
        assert!(
            response_pos > prompt_pos,
            "response should appear after user prompt"
        );

        // Boundary marker should be consumed
        assert!(
            !result.contains("patch-uuid-456"),
            "boundary marker should be consumed by explicit patch"
        );
    }

    #[test]
    fn boundary_reinserted_even_when_original_doc_has_no_boundary() {
        // Regression: the snowball bug — once one cycle loses the boundary,
        // every subsequent cycle also loses it because orig_had_boundary finds nothing.
        let dir = setup_project();
        let file = dir.path().join("test.md");
        // Document with exchange but NO boundary marker
        let doc =
            "<!-- agent:exchange patch=append -->\nUser prompt here.\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, doc).unwrap();

        let response = "<!-- patch:exchange -->\nAgent response.\n<!-- /patch:exchange -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        let result = apply_patches_via_path(doc, &patches, &unmatched, &file).unwrap();

        // Must have a boundary at end of exchange, even though original had none
        assert!(
            result.contains("<!-- agent:boundary:"),
            "boundary must be re-inserted even when original doc had no boundary: {result}"
        );
    }

    #[test]
    fn boundary_survives_multiple_cycles() {
        // Simulate two consecutive write cycles — boundary must persist
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = "<!-- agent:exchange patch=append -->\nPrompt 1.\n<!-- /agent:exchange -->\n";
        std::fs::write(&file, doc).unwrap();

        // Cycle 1
        let response1 = "<!-- patch:exchange -->\nResponse 1.\n<!-- /patch:exchange -->\n";
        let (patches1, unmatched1) = parse_patches(response1).unwrap();
        let result1 = apply_patches_via_path(doc, &patches1, &unmatched1, &file).unwrap();
        assert!(
            result1.contains("<!-- agent:boundary:"),
            "cycle 1 must have boundary"
        );

        // Cycle 2 — use cycle 1's output as the new doc (simulates next write)
        let response2 = "<!-- patch:exchange -->\nResponse 2.\n<!-- /patch:exchange -->\n";
        let (patches2, unmatched2) = parse_patches(response2).unwrap();
        let result2 = apply_patches_via_path(&result1, &patches2, &unmatched2, &file).unwrap();
        assert!(
            result2.contains("<!-- agent:boundary:"),
            "cycle 2 must have boundary"
        );
    }

    #[test]
    fn exchange_boundary_insert_adds_blank_line_before_response_heading() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: Prior -- gpt-5\n\n",
            "This guards against a future regression.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        let response = "<!-- patch:exchange -->\n### Re: Follow-up -- gpt-5\n\nDone.\n<!-- /patch:exchange -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        let result = apply_patches_via_path(doc, &patches, &unmatched, &file).unwrap();

        assert!(
            result.contains("future regression.\n\n### Re: Follow-up"),
            "response heading should be separated from the previous paragraph: {result}"
        );
        assert!(
            !result.contains("future regression.\n### Re: Follow-up"),
            "response heading must not attach to the previous paragraph: {result}"
        );
    }

    #[test]
    fn exchange_fallback_append_adds_blank_line_before_response_heading() {
        let dir = setup_project();
        let file = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: Prior -- gpt-5\n\n",
            "This guards against a future regression.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();

        let response = "<!-- patch:exchange -->\n### Re: Follow-up -- gpt-5\n\nDone.\n<!-- /patch:exchange -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        let result = apply_patches_via_path(doc, &patches, &unmatched, &file).unwrap();

        assert!(
            result.contains("future regression.\n\n### Re: Follow-up"),
            "response heading should be separated from the previous paragraph: {result}"
        );
        assert!(
            !result.contains("future regression.\n### Re: Follow-up"),
            "response heading must not attach to the previous paragraph: {result}"
        );
    }

    #[test]
    fn remove_all_boundaries_skips_code_blocks() {
        let doc = "before\n```\n<!-- agent:boundary:fake-id -->\n```\nafter\n<!-- agent:boundary:real-id -->\nend\n";
        let result = remove_all_boundaries(doc);
        // The one inside the code block should survive
        assert!(
            result.contains("<!-- agent:boundary:fake-id -->"),
            "boundary inside code block must be preserved: {result}"
        );
        // The one outside should be removed
        assert!(
            !result.contains("<!-- agent:boundary:real-id -->"),
            "boundary outside code block must be removed: {result}"
        );
    }

    #[test]
    fn reposition_boundary_moves_to_end() {
        let doc = "\
<!-- agent:exchange -->
Previous response.
<!-- agent:boundary:old-id -->
User prompt here.
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        // Old boundary should be gone
        assert!(!result.contains("old-id"), "old boundary should be removed");
        // New boundary should exist
        assert!(
            result.contains("<!-- agent:boundary:"),
            "new boundary should be inserted"
        );
        // New boundary should be after the user prompt, before close tag
        let boundary_pos = result.find("<!-- agent:boundary:").unwrap();
        let prompt_pos = result.find("User prompt here.").unwrap();
        let close_pos = result.find("<!-- /agent:exchange -->").unwrap();
        assert!(
            boundary_pos > prompt_pos,
            "boundary should be after user prompt"
        );
        assert!(
            boundary_pos < close_pos,
            "boundary should be before close tag"
        );
    }

    #[test]
    fn reposition_boundary_no_exchange_unchanged() {
        let doc = "\
<!-- agent:output -->
Some content.
<!-- /agent:output -->";
        let result = reposition_boundary_to_end(doc);
        assert!(
            !result.contains("<!-- agent:boundary:"),
            "no boundary should be added to non-exchange"
        );
    }

    #[test]
    fn reposition_boundary_clean_reuses_explicit_id() {
        let doc = "\
<!-- agent:exchange -->
Previous response.
<!-- agent:boundary:old-id -->
User prompt here.
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end_clean_with_id(doc, Some("keep-this-id"));
        assert!(!result.contains("old-id"), "old boundary should be removed");
        assert!(
            result.contains("<!-- agent:boundary:keep-this-id -->"),
            "explicit boundary id should be reused"
        );
        assert_eq!(
            result.matches("<!-- agent:boundary:").count(),
            1,
            "exactly one boundary should remain"
        );
    }

    #[test]
    fn reposition_appends_head_to_last_re_heading() {
        // #hdap: reposition must append ` (HEAD)` to the last `### Re:`
        // heading inside the exchange component, stripping any stale
        // `(HEAD)` suffix from earlier headings.
        let doc = "\
<!-- agent:exchange -->
### Re: older (HEAD)
old body
### Re: newer
new body
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        assert!(
            !result.contains("### Re: older (HEAD)"),
            "stale (HEAD) on prior heading must be stripped; got:\n{result}"
        );
        assert!(
            result.contains("### Re: older\n"),
            "older heading must remain (without HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: newer (HEAD)"),
            "latest heading must get (HEAD); got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) in result; got:\n{result}"
        );
    }

    #[test]
    fn reposition_head_annotation_no_re_heading_unchanged() {
        // No `### Re:` headings → no (HEAD) added, content passes through.
        let doc = "\
<!-- agent:exchange -->
User text with no response headings.
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        assert!(
            !result.contains("(HEAD)"),
            "no heading → no (HEAD); got:\n{result}"
        );
    }

    #[test]
    fn reposition_head_annotation_skips_code_fence() {
        // ### Re: inside a fenced code block must NOT be treated as a heading.
        let doc = "\
<!-- agent:exchange -->
### Re: real heading
```markdown
### Re: fake heading in code fence
```
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        assert!(
            result.contains("### Re: real heading (HEAD)"),
            "real heading outside fence gets (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: fake heading in code fence\n"),
            "fenced heading must be untouched; got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) — fenced heading ignored; got:\n{result}"
        );
    }

    #[test]
    fn reposition_with_baseline_marks_all_new_re_headings() {
        // Patchback with multiple `### Re:` headings: every heading NOT in
        // the baseline (git HEAD) gets (HEAD); every heading IN the baseline
        // does not. This matches the "all patchback top-level headers" rule.
        let doc = "\
<!-- agent:exchange -->
### Re: old-1
body a
### Re: old-2 (HEAD)
body b
### Re: new-1
body c
### Re: new-2
body d
<!-- /agent:exchange -->";
        // Baseline contains just the two "old" headings (no (HEAD), as HEAD
        // blob is always stripped by the commit staging path).
        let mut baseline = std::collections::HashSet::new();
        baseline.insert("### Re: old-1".to_string());
        baseline.insert("### Re: old-2".to_string());

        let result = reposition_boundary_to_end_with_baseline(doc, None, Some(&baseline));

        // Both old headings lose (HEAD).
        assert!(
            result.contains("### Re: old-1\n"),
            "old-1 must not have (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: old-2\n"),
            "old-2 must not have (HEAD); got:\n{result}"
        );
        // Both new headings get (HEAD).
        assert!(
            result.contains("### Re: new-1 (HEAD)"),
            "new-1 must get (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: new-2 (HEAD)"),
            "new-2 must get (HEAD); got:\n{result}"
        );
        // Exactly two (HEAD)s — one per new heading.
        assert_eq!(
            result.matches("(HEAD)").count(),
            2,
            "exactly two (HEAD) markers; got:\n{result}"
        );
    }

    #[test]
    fn reposition_with_empty_baseline_marks_every_re_heading() {
        // First cycle / untracked file: baseline is empty. All headings are
        // "new", so all get (HEAD).
        let doc = "\
<!-- agent:exchange -->
### Re: first
a
### Re: second
b
<!-- /agent:exchange -->";
        let baseline: std::collections::HashSet<String> = std::collections::HashSet::new();
        let result = reposition_boundary_to_end_with_baseline(doc, None, Some(&baseline));
        assert!(
            result.contains("### Re: first (HEAD)"),
            "first gets (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("### Re: second (HEAD)"),
            "second gets (HEAD); got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            2,
            "exactly two (HEAD) markers; got:\n{result}"
        );
    }

    #[test]
    fn exchange_baseline_headings_extracts_stripped_re_lines() {
        let doc = "\
<!-- agent:exchange -->
### Re: one (HEAD)
body
### Re: two
more body
### Not a Re heading
body
<!-- /agent:exchange -->";
        let set = exchange_baseline_headings(doc);
        assert!(
            set.contains("### Re: one"),
            "stripped one present; got: {set:?}"
        );
        assert!(set.contains("### Re: two"), "two present; got: {set:?}");
        assert_eq!(set.len(), 2, "only Re: headings; got: {set:?}");
    }

    #[test]
    fn exchange_baseline_headings_normalizes_leading_whitespace() {
        // HEAD has an indented heading; set entry must be trim_start'd so a
        // non-indented working-tree heading matches it.
        let doc = "\
<!-- agent:exchange -->
  ### Re: indented
body
### Re: flush
more
<!-- /agent:exchange -->";
        let set = exchange_baseline_headings(doc);
        assert!(
            set.contains("### Re: indented"),
            "indented entry normalized; got: {set:?}"
        );
        assert!(
            set.contains("### Re: flush"),
            "flush entry present; got: {set:?}"
        );
    }

    #[test]
    fn reposition_with_baseline_matches_indented_heading() {
        // Baseline has "### Re: foo" (flush). Working tree has "  ### Re: foo"
        // (indented). trim_start normalization makes the lookup recognize the
        // indented heading as already-in-baseline. Because the baseline filter
        // then yields zero "new" headings, the fallback kicks in and marks the
        // last Re: heading anyway — preserving the head pointer. The key point
        // is that normalization works (the heading is recognized), not that
        // (HEAD) is absent.
        let doc = "\
<!-- agent:exchange -->
  ### Re: foo
body
### Re: bar (HEAD)
body2
<!-- /agent:exchange -->";
        let mut baseline = std::collections::HashSet::new();
        baseline.insert("### Re: foo".to_string());
        baseline.insert("### Re: bar".to_string());
        let result = reposition_boundary_to_end_with_baseline(doc, None, Some(&baseline));
        // Both headings are in baseline → filter is empty → fallback marks
        // the LAST Re: heading only. "### Re: foo" stays unmarked (proving
        // trim_start normalization worked — without it, foo would be
        // treated as new and also get (HEAD)).
        assert!(
            result.contains("  ### Re: foo\n"),
            "indented heading must remain unmarked; got:\n{result}"
        );
        assert!(
            result.contains("### Re: bar (HEAD)"),
            "last heading gets fallback (HEAD) marker; got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) via fallback; got:\n{result}"
        );
    }

    #[test]
    fn baseline_filter_empty_falls_back_to_last_heading() {
        // When every Re: heading in the working tree is already in baseline
        // (i.e., the current turn adds no new Re: sections), the filter is
        // empty. The fallback must mark the last heading so the working tree
        // retains a single "head" marker across empty-Re cycles.
        let doc = "\
<!-- agent:exchange -->
### Re: older
body
### Re: newer (HEAD)
more
<!-- /agent:exchange -->";
        let mut baseline = std::collections::HashSet::new();
        baseline.insert("### Re: older".to_string());
        baseline.insert("### Re: newer".to_string());
        let result = reposition_boundary_to_end_with_baseline(doc, None, Some(&baseline));
        assert!(
            result.contains("### Re: newer (HEAD)"),
            "last heading retains (HEAD) via fallback; got:\n{result}"
        );
        assert!(
            result.contains("### Re: older\n"),
            "older heading remains unmarked; got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) marker after fallback; got:\n{result}"
        );
    }

    #[test]
    fn reposition_head_annotation_strips_multiple_stale() {
        // Multiple stale (HEAD)s on prior headings → all stripped, only last gets it.
        let doc = "\
<!-- agent:exchange -->
### Re: one (HEAD)
a
### Re: two (HEAD)
b
### Re: three
c
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end(doc);
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one (HEAD) after reposition; got:\n{result}"
        );
        assert!(result.contains("### Re: three (HEAD)"));
        assert!(result.contains("### Re: one\n"));
        assert!(result.contains("### Re: two\n"));
    }

    #[test]
    fn preserve_head_keeps_head_markers_on_reposition() {
        let doc = "\
<!-- agent:exchange -->
### Re: older
body a
### Re: newer (HEAD)
body b
<!-- agent:boundary:old-id -->
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end_preserve_head(doc);
        assert!(
            result.contains("### Re: newer (HEAD)"),
            "preserve_head must keep (HEAD) on newest heading; got:\n{result}"
        );
        assert!(
            !result.contains("old-id"),
            "old boundary should be removed; got:\n{result}"
        );
        assert_eq!(
            result.matches("<!-- agent:boundary:").count(),
            1,
            "exactly one fresh boundary; got:\n{result}"
        );
    }

    #[test]
    fn preserve_head_with_id_keeps_head_and_uses_explicit_id() {
        let doc = "\
<!-- agent:exchange -->
### Re: topic (HEAD)
body
<!-- agent:boundary:old-id -->
<!-- /agent:exchange -->";
        let result = reposition_boundary_to_end_preserve_head_with_id(doc, Some("explicit-id"));
        assert!(
            result.contains("### Re: topic (HEAD)"),
            "preserve_head must keep (HEAD); got:\n{result}"
        );
        assert!(
            result.contains("<!-- agent:boundary:explicit-id -->"),
            "explicit boundary id should be used; got:\n{result}"
        );
        assert!(
            !result.contains("old-id"),
            "old boundary gone; got:\n{result}"
        );
    }

    #[test]
    fn clean_strips_head_but_preserve_head_keeps_it() {
        let doc = "\
<!-- agent:exchange -->
### Re: first
body a
### Re: second (HEAD)
body b
<!-- /agent:exchange -->";
        let clean = reposition_boundary_to_end_clean(doc);
        let preserved = reposition_boundary_to_end_preserve_head(doc);

        assert!(
            !clean.contains("(HEAD)"),
            "clean variant must strip (HEAD); got:\n{clean}"
        );
        assert!(
            preserved.contains("### Re: second (HEAD)"),
            "preserve_head variant must keep (HEAD); got:\n{preserved}"
        );
    }

    #[test]
    fn max_lines_inline_attr_trims_content() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:log patch=replace max_lines=3 -->\nold\n<!-- /agent:log -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "log".to_string(),
            content: "line1\nline2\nline3\nline4\nline5\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        assert!(!result.contains("line1"));
        assert!(!result.contains("line2"));
        assert!(result.contains("line3"));
        assert!(result.contains("line4"));
        assert!(result.contains("line5"));
    }

    #[test]
    fn max_lines_noop_when_under_limit() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "<!-- agent:log patch=replace max_lines=10 -->\nold\n<!-- /agent:log -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "log".to_string(),
            content: "line1\nline2\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        assert!(result.contains("line1"));
        assert!(result.contains("line2"));
    }

    #[test]
    fn max_lines_inline_beats_toml() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        std::fs::write(
            dir.path().join(".agent-doc/config.toml"),
            "[components.log]\nmax_lines = 1\n",
        )
        .unwrap();
        let doc = "<!-- agent:log patch=replace max_lines=3 -->\nold\n<!-- /agent:log -->\n";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "log".to_string(),
            content: "a\nb\nc\nd\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        // Inline max_lines=3 should win over toml max_lines=1
        assert!(result.contains("b"));
        assert!(result.contains("c"));
        assert!(result.contains("d"));
    }

    #[test]
    fn parse_patch_with_transfer_source_attr() {
        let response = "<!-- patch:exchange transfer-source=\"tasks/eval-runner.md\" -->\nTransferred content.\n<!-- /patch:exchange -->\n";
        let (patches, unmatched) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "exchange");
        assert_eq!(patches[0].content, "Transferred content.\n");
        assert_eq!(
            patches[0].attrs.get("transfer-source"),
            Some(&"\"tasks/eval-runner.md\"".to_string())
        );
        assert!(unmatched.is_empty());
    }

    #[test]
    fn parse_patch_without_attrs() {
        let response = "<!-- patch:exchange -->\nContent.\n<!-- /patch:exchange -->\n";
        let (patches, _) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert!(patches[0].attrs.is_empty());
    }

    #[test]
    fn parse_patch_with_multiple_attrs() {
        let response =
            "<!-- patch:output mode=replace max_lines=50 -->\nContent.\n<!-- /patch:output -->\n";
        let (patches, _) = parse_patches(response).unwrap();
        assert_eq!(patches.len(), 1);
        assert_eq!(patches[0].name, "output");
        assert_eq!(patches[0].attrs.get("mode"), Some(&"replace".to_string()));
        assert_eq!(patches[0].attrs.get("max_lines"), Some(&"50".to_string()));
    }

    #[test]
    fn apply_patches_dedup_exchange_adjacent_echo() {
        // Simulates the bug: agent echoes user prompt as first line of exchange patch.
        // The existing exchange already ends with the prompt line.
        // After apply_patches, the prompt should appear exactly once.
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
❯ How do I configure .mise.toml?
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        // Agent echoes the prompt as first line of its response patch
        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "❯ How do I configure .mise.toml?\n\n### Re: configure .mise.toml\n\nUse `[env]` section.\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();

        let count = result.matches("❯ How do I configure .mise.toml?").count();
        assert_eq!(
            count, 1,
            "prompt line should appear exactly once, got:\n{result}"
        );
        assert!(
            result.contains("### Re: configure .mise.toml"),
            "response heading should be present"
        );
        assert!(
            result.contains("Use `[env]` section."),
            "response body should be present"
        );
    }

    #[test]
    fn apply_patches_dedup_preserves_blank_lines() {
        // Blank lines between sections must not be collapsed by dedup.
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
Previous response.
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "\n\n### Re: something\n\nAnswer here.\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        assert!(
            result.contains("Previous response."),
            "existing content preserved"
        );
        assert!(
            result.contains("### Re: something"),
            "response heading present"
        );
        // Multiple blank lines should survive (dedup only targets non-blank)
        assert!(result.contains('\n'), "blank lines preserved");
    }

    #[test]
    fn apply_patches_dedup_preserves_adjacent_code_fences() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
Some text.
```
code block 1
```
```
code block 2
```
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: test — opus-4-6\n\nResponse.\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        let fence_count = result.matches("```").count();
        assert_eq!(
            fence_count, 4,
            "all four code fences must survive dedup, got:\n{result}"
        );
        assert!(
            result.contains("```\n```"),
            "adjacent code fences must be preserved"
        );
    }

    #[test]
    fn apply_patches_append_preserves_response_leading_code_fence_after_prompt_fence() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
❯ show fenced prompt
```
prompt body
```
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "```\nresponse body\n```\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();

        assert_eq!(
            result.matches("```").count(),
            4,
            "prompt and response fences must all survive append:\n{result}"
        );
        assert!(
            result.contains("```\n```\nresponse body\n```"),
            "response opening fence must remain after the prompt closing fence:\n{result}"
        );
    }

    #[test]
    fn apply_patches_marks_new_exchange_headings_with_head() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = "\
<!-- agent:exchange patch=append -->
### Re: earlier — gpt-5

Existing answer.
<!-- /agent:exchange -->
";
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: latest — gpt-5\n\nFresh answer.\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_with_overrides_via_path(
            doc,
            &patches,
            "",
            &doc_path,
            &Default::default(),
        )
        .unwrap();

        assert!(
            result.contains("### Re: earlier — gpt-5\n"),
            "existing response heading must stay clean; got:\n{result}"
        );
        assert!(
            result.contains("### Re: latest — gpt-5 (HEAD)\n"),
            "new response heading must surface as transient HEAD; got:\n{result}"
        );
        assert_eq!(
            result.matches("(HEAD)").count(),
            1,
            "exactly one transient HEAD marker expected; got:\n{result}"
        );
    }

    #[test]
    fn apply_patches_binds_exchange_response_to_oldest_matching_unresolved_prompt() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old-anchor -->\n",
            "❯ do #wcup1. spec-test-commit-push\n\n",
            "❯ do #wcx1. spec-test-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: #wcup1 — gpt-5\n\nAlready complete.\n".to_string(),
            attrs: Default::default(),
        }];
        let result = apply_patches_with_overrides_via_path(
            doc,
            &patches,
            "",
            &doc_path,
            &Default::default(),
        )
        .unwrap();

        let wcup1_prompt = result.find("❯ do #wcup1. spec-test-commit-push").unwrap();
        let wcup1_response = result.find("### Re: #wcup1 — gpt-5 (HEAD)").unwrap();
        let new_boundary = result.rfind("<!-- agent:boundary:").unwrap();
        let wcx1_prompt = result.find("❯ do #wcx1. spec-test-commit-push").unwrap();

        assert!(
            wcup1_prompt < wcup1_response,
            "response must land after the matched older prompt:\n{result}"
        );
        assert!(
            wcup1_response < new_boundary,
            "new boundary must move behind the response:\n{result}"
        );
        assert!(
            new_boundary < wcx1_prompt,
            "newer unresolved prompts must remain after the new boundary:\n{result}"
        );
    }

    #[test]
    fn apply_patches_rejects_exchange_response_that_skips_older_unresolved_prompt() {
        let dir = setup_project();
        let doc_path = dir.path().join("test.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:old-anchor -->\n",
            "❯ do #wcup1. spec-test-commit-push\n\n",
            "❯ do #wcx1. spec-test-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc_path, doc).unwrap();

        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: #wcx1 — gpt-5\n\nNot done yet.\n".to_string(),
            attrs: Default::default(),
        }];
        let err = apply_patches_with_overrides_via_path(
            doc,
            &patches,
            "",
            &doc_path,
            &Default::default(),
        )
        .expect_err("later-matching response should fail closed");
        assert!(
            err.to_string().contains("skip older unresolved prompt"),
            "unexpected error: {err}"
        );
    }

    fn exchange_component(doc: &str) -> Component {
        component::parse(doc)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "exchange")
            .unwrap()
    }

    #[test]
    fn append_anchor_falls_back_when_single_prompt_edited_after_baseline() {
        // `#patchback-prompt-edit-resilience`: the operator edited the sole
        // unresolved prompt (text AND `❯ ` prefix) after the baseline was
        // captured, so the baseline tail no longer matches the current document
        // verbatim. The exact-match locate fails, but the single-prompt fallback
        // anchors the response after the edited prompt instead of failing closed.
        let original = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:b1 -->\n",
            "❯ Why is it still running?\n",
            "<!-- /agent:exchange -->\n",
        );
        // Current: boundary repositioned to the end; prompt edited to drop the
        // `❯ ` prefix and change "it is" -> "its".
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "Why its still running?\n",
            "<!-- agent:boundary:b2 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let comp = exchange_component(current);
        let patch = "### Re: still running — gpt-5\n\nBy design.\n";
        let result = append_exchange_patch_after_prompt_anchor(original, current, &comp, patch)
            .unwrap()
            .expect("single edited prompt must anchor via the resilient fallback");
        let prompt_pos = result.find("Why its still running?").unwrap();
        let resp_pos = result.find("### Re: still running").unwrap();
        let boundary_pos = result.rfind("<!-- agent:boundary:").unwrap();
        assert!(
            prompt_pos < resp_pos,
            "response must land after the edited prompt:\n{result}"
        );
        assert!(
            resp_pos < boundary_pos,
            "new boundary must move behind the response:\n{result}"
        );
    }

    #[test]
    fn append_anchor_fails_closed_when_multiple_prompts_drift_from_baseline() {
        // With more than one unresolved prompt, an edited baseline is genuinely
        // ambiguous (we can't tell which edited prompt the response targets), so
        // it fails closed with an actionable refresh-the-baseline diagnostic
        // rather than guessing.
        let original = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "<!-- agent:boundary:b1 -->\n",
            "❯ First original question?\n\n",
            "❯ Second original question?\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ First EDITED question?\n\n",
            "❯ Second EDITED question?\n",
            "<!-- agent:boundary:b2 -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let comp = exchange_component(current);
        let patch = "### Re: answer — gpt-5\n\nBody.\n";
        let err = append_exchange_patch_after_prompt_anchor(original, current, &comp, patch)
            .expect_err("ambiguous multi-prompt baseline drift must fail closed");
        assert!(
            err.to_string().contains("drifted from the baseline")
                && err.to_string().contains("refresh the baseline"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn apply_mode_append_strips_leading_overlap() {
        // When new_content starts with the last non-blank line of existing,
        // apply_mode("append") should not duplicate that line.
        let existing = "❯ How do I configure .mise.toml?\n";
        let new_content = "❯ How do I configure .mise.toml?\n\n### Re: configure\n\nUse `[env]`.\n";
        let result = apply_mode("append", existing, new_content);
        let count = result.matches("❯ How do I configure .mise.toml?").count();
        assert_eq!(count, 1, "overlap line should appear exactly once");
        assert!(result.contains("### Re: configure"));
    }

    #[test]
    fn apply_mode_append_preserves_leading_code_fence_overlap() {
        let existing = "❯ show fenced prompt\n```\nprompt body\n```\n";
        let new_content = "```\nresponse body\n```\n";

        let result = apply_mode("append", existing, new_content);

        assert_eq!(
            result.matches("```").count(),
            4,
            "code fence delimiters are structural, not duplicate prompt overlap:\n{result}"
        );
        assert!(
            result.contains("```\n```\nresponse body\n```"),
            "adjacent prompt/response fences must remain distinct:\n{result}"
        );
    }

    #[test]
    fn strip_trailing_caret_removes_bare_prompt_line() {
        let content = "Answer text.\n❯\n";
        assert_eq!(strip_trailing_caret_lines(content), "Answer text.\n");
    }

    #[test]
    fn strip_trailing_caret_removes_multiple_trailing_lines() {
        let content = "Answer.\n❯\n❯\n";
        assert_eq!(strip_trailing_caret_lines(content), "Answer.\n");
    }

    #[test]
    fn strip_trailing_caret_preserves_mid_content_caret() {
        // `❯` mid-content (e.g. user prompt quoted in response) must survive.
        let content = "### Re: topic\n\n❯ user question echoed\n\nAnswer.\n";
        assert_eq!(strip_trailing_caret_lines(content), content);
    }

    #[test]
    fn strip_trailing_caret_preserves_caret_with_text() {
        // Line that starts with `❯ ` and has other text is user content; don't strip.
        let content = "Answer.\n❯ follow-up\n";
        assert_eq!(strip_trailing_caret_lines(content), content);
    }

    #[test]
    fn strip_trailing_caret_handles_no_trailing_newline() {
        let content = "Answer.\n❯";
        assert_eq!(strip_trailing_caret_lines(content), "Answer.");
    }

    #[test]
    fn strip_trailing_caret_noop_when_no_caret() {
        let content = "Answer.\n";
        assert_eq!(strip_trailing_caret_lines(content), content);
    }

    #[test]
    fn apply_patches_strips_trailing_caret_from_exchange() {
        let doc = "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n❯ prior question\n<!-- /agent:exchange -->\n";
        let patches = vec![PatchBlock {
            name: "exchange".to_string(),
            content: "### Re: thing\n\nAnswer.\n❯\n".to_string(),
            attrs: Default::default(),
        }];
        let doc_path = std::path::PathBuf::from("/tmp/test.md");
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        // Extract just the exchange component content
        let components = component::parse(&result).unwrap();
        let exchange = components.iter().find(|c| c.name == "exchange").unwrap();
        let content = exchange.content(&result);
        // No bare `❯` on its own line immediately before the boundary marker.
        let has_bare_caret_before_boundary = content
            .lines()
            .collect::<Vec<_>>()
            .windows(2)
            .any(|w| w[0].trim() == "❯" && w[1].starts_with("<!-- agent:boundary"));
        assert!(
            !has_bare_caret_before_boundary,
            "bare ❯ line must not appear before boundary marker. content:\n{}",
            content
        );
    }

    #[test]
    fn apply_patches_preserves_caret_in_non_exchange() {
        // A patch targeting a non-exchange component should preserve trailing `❯`
        // (no special rule there).
        let doc = "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n<!-- agent:notes patch=replace -->\n<!-- /agent:notes -->\n";
        let patches = vec![PatchBlock {
            name: "notes".to_string(),
            content: "note body\n❯\n".to_string(),
            attrs: Default::default(),
        }];
        let doc_path = std::path::PathBuf::from("/tmp/test.md");
        let result = apply_patches_via_path(doc, &patches, "", &doc_path).unwrap();
        let components = component::parse(&result).unwrap();
        let notes = components.iter().find(|c| c.name == "notes").unwrap();
        assert!(
            notes.content(&result).contains("❯"),
            "non-exchange content retains ❯"
        );
    }

    #[test]
    fn apply_mode_append_no_overlap_unchanged() {
        // When new_content does NOT start with the last non-blank line of existing,
        // apply_mode("append") should concatenate normally.
        let existing = "Previous content.\n";
        let new_content = "### Re: something\n\nAnswer.\n";
        let result = apply_mode("append", existing, new_content);
        assert_eq!(result, "Previous content.\n### Re: something\n\nAnswer.\n");
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_moves_tail_back_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:pending -->\n\n",
            "## Assistant\n\n",
            "Follow-up response.\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let pending_open = repaired.find("<!-- agent:pending -->").unwrap();
        let assistant = repaired.find("## Assistant").unwrap();

        assert!(
            assistant < exchange_close,
            "assistant tail should move back inside exchange:\n{repaired}"
        );
        assert!(
            pending_open > exchange_close,
            "pending should remain outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:boundary:").count(),
            1,
            "repair should leave exactly one boundary marker"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_rejects_ambiguous_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Assistant\n\n",
            "Escaped answer.\n\n",
            "## Todo / Backlog\n\n",
            "- not conversation content\n"
        );

        let err = repair_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string().contains("escaped `agent:exchange`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_moves_plain_trailing_suffix_after_todo() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "## User\n",
            "compact exchange\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:pending -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:pending -->\n\n",
            "<!-- agent:todo patch=replace -->\n",
            "- [ ] backlog\n",
            "<!-- /agent:todo -->\n\n",
            "Exchange compacted. No new work was run in this turn.\n\n",
            "## Assistant\n\n",
            "Exchange compacted. No new work was run in this turn.\n\n",
            "## User\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let pending_open = repaired.find("<!-- agent:pending -->").unwrap();
        let todo_open = repaired.find("<!-- agent:todo patch=replace -->").unwrap();
        let trailing_summary = repaired
            .rfind("Exchange compacted. No new work was run in this turn.")
            .unwrap();

        assert!(
            trailing_summary < exchange_close,
            "plain trailing suffix should move back inside exchange:\n{repaired}"
        );
        assert!(
            pending_open > exchange_close && todo_open > exchange_close,
            "sibling components should stay outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:boundary:").count(),
            1,
            "repair should leave exactly one boundary marker"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_comment_only_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "comment-only suffix should stay outside exchange"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Can this stay hidden?\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "prompt-like text inside ordinary HTML comments must not be moved into exchange"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_ignores_unterminated_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Still typing this scratch note.\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc).unwrap();
        assert!(
            repaired.is_none(),
            "prompt-like text inside a transiently unclosed ordinary HTML comment must not be moved into exchange"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_moves_gap_before_backlog_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "### Re: gap — gpt-5\n\n",
            "Escaped answer.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: gap — gpt-5").unwrap();
        let gap_marker = repaired.find("\n###\n\n").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "gap response should move back inside exchange:\n{repaired}"
        );
        assert!(
            gap_marker > exchange_close,
            "plain gap marker should remain outside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn repair_conversation_tail_outside_exchange_moves_prompt_before_gap_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "do [#oobprompt]. spec-test-build-install-commit-push\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let prompt = repaired
            .find("do [#oobprompt]. spec-test-build-install-commit-push")
            .unwrap();
        let gap_marker = repaired.find("\n###\n\n").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            prompt < exchange_close,
            "prompt should move back inside exchange:\n{repaired}"
        );
        assert!(
            gap_marker > exchange_close,
            "plain gap marker should remain outside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn repair_duplicate_exchange_close_tail_moves_escaped_response_back_inside_exchange() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "### Re: later — gpt-5\n\n",
            "Escaped answer.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_tail(doc)
            .unwrap()
            .expect("duplicate close repair should apply");
        let exchange_close = repaired.find("<!-- /agent:exchange -->").unwrap();
        let response = repaired.find("### Re: later — gpt-5").unwrap();
        let backlog = repaired.find("<!-- agent:backlog -->").unwrap();

        assert!(
            response < exchange_close,
            "escaped response should move back inside exchange:\n{repaired}"
        );
        assert!(
            backlog > exchange_close,
            "backlog should remain outside exchange:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- /agent:exchange -->").count(),
            1,
            "repair should leave exactly one exchange close marker"
        );
    }

    #[test]
    fn repair_duplicate_exchange_close_scaffold_drops_inserted_template_shell() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "JB `Run Agent Doc` failed on this document.\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_scaffold(doc)
            .unwrap()
            .expect("duplicate scaffold repair should apply");

        assert_eq!(
            repaired.matches("<!-- /agent:exchange -->").count(),
            1,
            "repair should leave one exchange close:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- agent:queue -->").count(),
            1,
            "repair should drop the duplicated queue scaffold:\n{repaired}"
        );
        assert!(repaired.contains("JB `Run Agent Doc` failed on this document."));
        assert!(component::parse(&repaired).is_ok());
    }

    #[test]
    fn repair_duplicate_exchange_close_scaffold_rejects_mixed_user_text() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "<!-- agent:boundary:abc123 -->\n",
            "c The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Completed / Reaped\n\n",
            "<!-- agent:done -->\n",
            "<!-- /agent:done -->\n",
            "corky.md The arrow\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_close_scaffold(doc).unwrap();

        assert!(
            repaired.is_none(),
            "mixed user text must not be dropped as duplicated scaffold"
        );
    }

    #[test]
    fn normalize_editor_visible_template_structure_repairs_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!--\n",
            "Use TEST_THREADS=8.\n",
            "-->\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = normalize_editor_visible_template_structure(doc)
            .expect("safe duplicate scaffold should repair before editor write");

        assert_eq!(repaired.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(repaired.matches("<!-- agent:backlog -->").count(), 1);
        guard_no_conversation_tail_outside_exchange(&repaired).unwrap();
    }

    #[test]
    fn repair_queue_escape_removes_struck_items_below_marker_in_parking_lot() {
        // #queue-completed-items-escape-below-component: struck queue items that
        // drifted into the parking-lot comment beneath `<!-- /agent:queue -->`
        // must be removed, while live struck items inside the queue, real
        // component content, and ordinary scratch comment text are preserved.
        let doc = concat!(
            "<!-- agent:queue go -->\n",
            "- ~~:round_pushpin: do [#alpha]~~\n",
            "- :round_pushpin: do [#beta]\n",
            "<!-- /agent:queue -->\n",
            "###\n",
            "<!--\n",
            "a real scratch note line\n",
            "- ~~:round_pushpin: do [#gamma]~~\n",
            "- ~~:pushpin: do [#delta]~~\n",
            "-->\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep] real backlog item\n",
            "<!-- /agent:backlog -->\n",
        );
        let repaired =
            repair_queue_struck_items_escaped_below_marker(doc).expect("displaced items removed");
        // Displaced struck items gone.
        assert!(
            !repaired.contains("[#gamma]"),
            "gamma displaced struck removed"
        );
        assert!(
            !repaired.contains("[#delta]"),
            "delta displaced struck removed"
        );
        // Live queue content preserved (inside the component span).
        assert!(
            repaired.contains("- ~~:round_pushpin: do [#alpha]~~"),
            "in-queue struck kept"
        );
        assert!(
            repaired.contains("- :round_pushpin: do [#beta]"),
            "in-queue active kept"
        );
        // Ordinary scratch text and the backlog component preserved.
        assert!(
            repaired.contains("a real scratch note line"),
            "scratch text kept"
        );
        assert!(
            repaired.contains("[#keep] real backlog item"),
            "backlog content kept"
        );
    }

    #[test]
    fn repair_queue_escape_noop_when_no_displaced_items() {
        // A clean document with no escaped struck queue items is unchanged.
        let doc = concat!(
            "<!-- agent:queue go -->\n",
            "- ~~:round_pushpin: do [#alpha]~~\n",
            "- :round_pushpin: do [#beta]\n",
            "<!-- /agent:queue -->\n",
            "###\n",
            "<!--\n",
            "- ~~an ordinary user strikethrough note~~\n",
            "-->\n",
        );
        // The ordinary `- ~~note~~` has no pushpin/directive, so it is not a
        // displaced queue item and the repair is a no-op.
        assert!(repair_queue_struck_items_escaped_below_marker(doc).is_none());
    }

    #[test]
    fn repair_queue_escape_handles_bare_lines_after_marker() {
        // Displaced struck items can also sit bare between the marker and the
        // next component (not inside a comment).
        let doc = concat!(
            "<!-- agent:queue go -->\n",
            "- :round_pushpin: do [#beta]\n",
            "<!-- /agent:queue -->\n",
            "- ~~:round_pushpin: do [#gamma]~~\n",
            "- ~~do [#delta]~~\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep] item\n",
            "<!-- /agent:backlog -->\n",
        );
        let repaired =
            repair_queue_struck_items_escaped_below_marker(doc).expect("bare displaced removed");
        assert!(!repaired.contains("[#gamma]"));
        assert!(!repaired.contains("[#delta]"));
        assert!(repaired.contains("- :round_pushpin: do [#beta]"));
        assert!(repaired.contains("[#keep] item"));
    }

    #[test]
    fn normalize_editor_visible_template_structure_preserves_duplicate_prompt_html_comment_body() {
        let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n",
                "Done.\n",
                "<!-- agent:boundary:head -->\n",
                "{prompt}\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!--\n",
                "Keep this unrelated scratch note hidden.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        let repaired = normalize_editor_visible_template_structure(&doc)
            .expect("editor-visible normalization should preserve scratch comments");

        let duplicate_comment = format!("\n<!--\n{prompt}\n-->\n");
        assert!(
            repaired.contains(&duplicate_comment),
            "editor-visible normalization has no baseline ownership proof and must preserve post-exchange scratch text:\n{repaired}"
        );
        assert!(
            repaired.contains("<!-- agent:backlog -->\n- [ ] keep me"),
            "tracked-work scaffold should remain intact:\n{repaired}"
        );
        assert!(
            repaired.contains("Keep this unrelated scratch note hidden."),
            "unrelated scratch comments must stay outside exchange:\n{repaired}"
        );
    }

    #[test]
    fn normalize_editor_visible_template_structure_preserves_mixed_prompt_comment_scratch_lines() {
        let exchange_prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let duplicate_prompt_line = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior - gpt-5\n",
                "Done.\n",
                "<!-- agent:boundary:head -->\n",
                "{exchange_prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n",
                "<!--\n",
                "{duplicate_prompt_line}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "<!-- /agent:backlog -->\n"
            ),
            exchange_prompt = exchange_prompt,
            duplicate_prompt_line = duplicate_prompt_line,
        );

        let repaired = normalize_editor_visible_template_structure(&doc)
            .expect("editor-visible normalization should preserve mixed scratch comments");

        assert!(
            repaired.contains(&format!("<!--\n{duplicate_prompt_line}")),
            "editor-visible normalization must not scrub mixed scratch comments without baseline proof:\n{repaired}"
        );
        assert!(
            repaired.contains("Look through the Claude + Codex + agent-doc session logs"),
            "unrelated scratch lines in the same ordinary comment must survive:\n{repaired}"
        );
        assert!(
            repaired.contains(&format!(
                "<!--\n{duplicate_prompt_line}\n#spec-test-build-install-commit-push\n---\nLook through"
            )),
            "the mixed comment shell and full body should be preserved:\n{repaired}"
        );
    }

    #[test]
    fn normalize_editor_visible_template_structure_preserves_scratch_comment_after_compact_summary()
    {
        let prompt = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\n",
                "Compacted content:\n",
                "- Trailing prompt/context: {prompt}\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: compact prompt duplication — gpt-5\n\n",
                "Line-by-line diff was the right diagnostic.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n",
                "###\n",
                "<!--\n",
                "Look through the Claude + Codex + agent-doc session logs\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        let repaired = normalize_editor_visible_template_structure(&doc)
            .expect("editor-visible normalization should preserve unrelated scratch comments");

        assert!(
            repaired.contains("Look through the Claude + Codex + agent-doc session logs"),
            "unrelated post-exchange scratch comment text must survive:\n{repaired}"
        );
    }

    #[test]
    fn normalize_editor_visible_template_structure_ignores_response_quoted_scratch_comment() {
        let scratch =
            "Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ Please inspect the latest route cleanup report. #spec-test-build-install-commit-push\n",
                "### Re: route cleanup — gpt-5\n\n",
                "{scratch}\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n",
                "###\n",
                "<!--\n",
                "{scratch}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "<!-- /agent:backlog -->\n"
            ),
            scratch = scratch
        );

        let repaired = normalize_editor_visible_template_structure(&doc)
            .expect("response-quoted scratch comments should not be prompt residue");

        assert!(
            repaired.contains(scratch),
            "response text must not authorize deleting matching user-owned scratch comments:\n{repaired}"
        );
        assert!(
            repaired.contains(&format!("<!--\n{scratch}\n-->")),
            "ordinary scratch comment body must stay intact:\n{repaired}"
        );
    }

    #[test]
    fn normalize_removes_duplicate_answered_prompt_tail_after_boundary() {
        let prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. Should we diff line by line?";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "### Re: mixed scratch comment deletion — gpt-5\n\n",
                "Answered already.\n",
                "<!-- agent:boundary:head -->\n",
                "❯ {prompt}\n",
                "❯ #spec-test-build-install-commit-push\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );

        let repaired = normalize_editor_visible_template_structure(&doc)
            .expect("editor-visible normalization should remove duplicate answered prompt tail");

        assert!(
            repaired.contains(&format!(
                "❯ {prompt}\n❯ #spec-test-build-install-commit-push\n### Re:"
            )),
            "answered prompt block must remain in exchange history:\n{repaired}"
        );
        assert!(
            !repaired.contains(&format!("<!-- agent:boundary:head -->\n❯ {prompt}")),
            "duplicate answered-form prompt tail after the boundary should be removed:\n{repaired}"
        );
        assert!(
            repaired.contains("<!-- agent:boundary:head -->\n<!-- /agent:exchange -->"),
            "boundary should stay at the exchange end after cleanup:\n{repaired}"
        );
    }

    #[test]
    fn guard_no_duplicate_prompt_residue_outside_exchange_rejects_plain_markdown_duplicate() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: duplicate residue — gpt-5\n\n",
                "Answered.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "# Notes\n\n",
                "{prompt}\n"
            ),
            prompt = prompt
        );

        let err = guard_no_duplicate_prompt_residue_outside_exchange(&doc).unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue outside"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn normalize_editor_visible_template_structure_rejects_plain_markdown_duplicate_residue() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: duplicate residue — gpt-5\n\n",
                "Answered.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "{prompt}\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        let err = normalize_editor_visible_template_structure(&doc).unwrap_err();

        assert!(
            err.to_string().contains("duplicate prompt residue"),
            "editor-visible normalization must fail closed on duplicate prompt Markdown residue: {err}"
        );
    }

    #[test]
    fn guard_no_duplicate_prompt_residue_outside_exchange_allows_tracked_components() {
        let prompt =
            "Please keep this exact sentence around for duplicate residue coverage in markdown";
        let doc = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: duplicate residue — gpt-5\n\n",
                "Answered.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:backlog -->\n",
                "{prompt}\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        guard_no_duplicate_prompt_residue_outside_exchange(&doc).unwrap();
    }

    #[test]
    fn normalize_editor_visible_template_structure_rejects_mixed_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n",
            "user typed into duplicated shell\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n"
        );

        let err = normalize_editor_visible_template_structure(doc).unwrap_err();
        assert!(
            err.to_string().contains("mixed duplicate scaffold"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn repair_duplicate_exchange_close_tail_rejects_ambiguous_suffix() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "<!-- /agent:exchange -->\n\n",
            "## Todo / Backlog\n\n",
            "- keep me outside exchange\n",
            "<!-- /agent:exchange -->\n"
        );

        let err = repair_duplicate_exchange_close_tail(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("duplicate close repair suffix is ambiguous"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn repair_duplicate_exchange_opener_merges_two_blocks() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ first question\n\n",
            "### Re: first — opus-4-6\n\n",
            "First answer.\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ second question\n\n",
            "### Re: second — opus-4-6\n\n",
            "Second answer.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let repaired = repair_duplicate_exchange_opener(doc)
            .unwrap()
            .expect("duplicate opener repair should apply");
        assert_eq!(
            repaired.matches("<!-- agent:exchange").count(),
            1,
            "repair should leave exactly one exchange opener:\n{repaired}"
        );
        assert_eq!(
            repaired.matches("<!-- /agent:exchange -->").count(),
            1,
            "repair should leave exactly one exchange closer:\n{repaired}"
        );
        assert!(
            repaired.contains("First answer."),
            "first block content should be preserved:\n{repaired}"
        );
        assert!(
            repaired.contains("Second answer."),
            "second block content should be preserved:\n{repaired}"
        );
        assert!(
            repaired.contains("<!-- agent:backlog -->"),
            "backlog should be preserved:\n{repaired}"
        );
        let first_pos = repaired.find("First answer.").unwrap();
        let second_pos = repaired.find("Second answer.").unwrap();
        assert!(
            first_pos < second_pos,
            "first content should appear before second:\n{repaired}"
        );
    }

    #[test]
    fn repair_duplicate_exchange_opener_returns_none_for_single() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ question\n",
            "### Re: answer — opus-4-6\n\n",
            "Answer.\n",
            "<!-- /agent:exchange -->\n"
        );

        let result = repair_duplicate_exchange_opener(doc).unwrap();
        assert!(result.is_none(), "single exchange block should return None");
    }

    #[test]
    fn strip_conversation_tail_outside_exchange_removes_escaped_heading_tail_only() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "[//]: # (leave this note outside exchange)\n\n",
            "## Assistant\n\n",
            "Escaped answer.\n"
        );

        let stripped = strip_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("escaped heading tail should be removable");
        assert!(
            stripped.contains("[//]: # (leave this note outside exchange)"),
            "comment-only note should remain outside exchange:\n{stripped}"
        );
        assert!(
            !stripped.contains("## Assistant"),
            "escaped assistant tail should be removed:\n{stripped}"
        );
    }

    #[test]
    fn strip_conversation_tail_outside_exchange_removes_gap_before_backlog_only() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "### Re: gap — gpt-5\n\n",
            "Escaped answer.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let stripped = strip_conversation_tail_outside_exchange(doc)
            .unwrap()
            .expect("escaped gap should be removable");
        assert!(
            stripped.contains("\n###\n\n<!-- agent:backlog -->"),
            "plain gap marker should stay outside exchange:\n{stripped}"
        );
        assert!(
            !stripped.contains("### Re: gap — gpt-5"),
            "escaped response should be removed from the gap:\n{stripped}"
        );
    }

    #[test]
    fn deleted_conversation_tail_cleanup_accepts_prompt_prelude_deletion() {
        let before = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "I see this routed prompt sitting outside exchange.\n",
            "It should have been entered by the managed pane.\n\n",
            "do #oobtaildel. spec-test-build-install-commit-push\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let cleaned = deleted_conversation_tail_cleanup(before, after)
            .unwrap()
            .expect("prompt-prelude cleanup should be accepted");
        assert_eq!(cleaned, after);
    }

    #[test]
    fn deleted_conversation_tail_cleanup_rejects_plain_note_deletion() {
        let before = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- /agent:exchange -->\n\n",
            "Design note: keep this outside exchange.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = before.replace("Design note: keep this outside exchange.\n\n", "");

        let cleaned = deleted_conversation_tail_cleanup(before, &after).unwrap();
        assert!(
            cleaned.is_none(),
            "ordinary note deletion should not be treated as escaped conversation cleanup"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_normal_content() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ user question\n",
            "### Re: question — opus-4-6\n\n",
            "Answer.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] some task\n",
            "<!-- /agent:backlog -->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_tail_after_session_digest() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ earlier question\n",
            "### Re: earlier — opus-4-6\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "# Session Digest\n\n",
            "Summary of work.\n\n",
            "❯ Why were the backlog items removed?\n",
            "### Re: backlog — opus-4-6\n\n",
            "Escaped answer.\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_tail_after_backlog() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "## Assistant\n\n",
            "Follow-up response.\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_gap_before_backlog() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "### Re: gap — gpt-5\n\n",
            "Escaped answer.\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_fails_on_prompt_before_gap_marker() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "do [#oobprompt]. spec-test-build-install-commit-push\n",
            "###\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        );

        let err = guard_no_conversation_tail_outside_exchange(doc).unwrap_err();
        assert!(
            err.to_string()
                .contains("outside `<!-- agent:exchange -->`"),
            "unexpected error: {err}"
        );
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_without_exchange_block() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status -->\n",
            "Active\n",
            "<!-- /agent:status -->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_comment_only_tail() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "[//]: # (leave this note outside exchange)\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn guard_no_conversation_tail_outside_exchange_passes_for_html_comment_body() {
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: earlier — gpt-5\n",
            "<!-- agent:boundary:abc123 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!--\n",
            "do #hidden. spec-test-build-install-commit-push\n",
            "Can this stay hidden?\n",
            "-->\n"
        );
        guard_no_conversation_tail_outside_exchange(doc).unwrap();
    }

    #[test]
    fn remove_duplicate_answered_tail_scrubs_prefixed_replay_residue() {
        // Answered-form residue (carries `❯ `) re-added below the boundary is
        // safely removable replay residue.
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ go\n",
            "### Re: go — gpt-5\n\n",
            "Did the thing.\n",
            "<!-- agent:boundary:head -->\n",
            "❯ go\n",
            "<!-- /agent:exchange -->\n",
        );
        let cleaned = remove_duplicate_answered_exchange_prompt_tail(doc)
            .expect("prefixed answered-form residue should be scrubbed");
        assert!(
            !cleaned.contains("head -->\n❯ go"),
            "answered-form residue tail must be removed:\n{cleaned}"
        );
        assert!(
            cleaned.contains("❯ go\n### Re: go"),
            "answered history must be preserved:\n{cleaned}"
        );
    }

    #[test]
    fn remove_duplicate_answered_tail_preserves_unprefixed_live_prompt() {
        // #ipcfullprompt-recur: a freshly-typed prompt (no `❯ `) that matches a
        // previously-answered prompt is a LIVE prompt and must never be scrubbed.
        let doc = concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ go\n",
            "### Re: go — gpt-5\n\n",
            "Did the thing.\n",
            "<!-- agent:boundary:head -->\n",
            "go\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(
            remove_duplicate_answered_exchange_prompt_tail(doc).is_none(),
            "a bare re-typed live prompt must be preserved"
        );
    }
