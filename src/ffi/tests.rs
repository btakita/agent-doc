    use super::*;

    fn parse_visual_tokens_json(doc: &str) -> Vec<serde_json::Value> {
        let c_doc = CString::new(doc).unwrap();
        let ptr = unsafe { agent_doc_visual_tokens_json(c_doc.as_ptr()) };
        assert!(!ptr.is_null(), "visual token JSON should not be null");
        let json_str = unsafe { CStr::from_ptr(ptr) }.to_str().unwrap().to_string();
        unsafe { agent_doc_free_string(ptr) };
        serde_json::from_str(&json_str).unwrap()
    }

    fn ffi_json_value(result: FfiJsonResult) -> serde_json::Value {
        if !result.error.is_null() {
            let error = unsafe { CStr::from_ptr(result.error) }
                .to_str()
                .unwrap()
                .to_string();
            unsafe { agent_doc_free_string(result.error) };
            panic!("unexpected FFI error: {error}");
        }
        assert!(!result.json.is_null(), "FFI JSON should not be null");
        let json = unsafe { CStr::from_ptr(result.json) }
            .to_str()
            .unwrap()
            .to_string();
        unsafe { agent_doc_free_string(result.json) };
        serde_json::from_str(&json).unwrap()
    }

    fn utf16_len(text: &str) -> usize {
        text.encode_utf16().count()
    }

    #[test]
    fn parse_components_roundtrip() {
        let doc = "before\n<!-- agent:status -->\nhello\n<!-- /agent:status -->\nafter\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_parse_components(c_doc.as_ptr()) };
        assert_eq!(result.count, 1);
        assert!(!result.json.is_null());
        let json_str = unsafe { CStr::from_ptr(result.json) }.to_str().unwrap();
        let parsed: Vec<serde_json::Value> = serde_json::from_str(json_str).unwrap();
        assert_eq!(parsed[0]["name"], "status");
        assert_eq!(parsed[0]["content"], "hello\n");
        unsafe { agent_doc_free_string(result.json) };
    }

    #[test]
    fn apply_node_patches_ffi_preserves_live_buffer_drift() {
        let doc = CString::new(
            "\
operator note
<!-- agent:queue -->
- do [#alpha]
- do [#beta]
- live buffer addition
<!-- /agent:queue -->
",
        )
        .unwrap();
        let patches = CString::new(
            r#"[
{"component":"queue","node_key":"queue:0:beta:0","op":"strike"},
{"component":"queue","node_key":"queue:0:gamma:0","op":"insert","content":"- do [#gamma]\n","after":"queue:0:beta:0"}
]"#,
        )
        .unwrap();

        let result = unsafe { agent_doc_apply_node_patches(doc.as_ptr(), patches.as_ptr()) };

        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("operator note\n"));
        assert!(text.contains("- live buffer addition\n"));
        assert!(text.contains("- ~~do [#beta]~~\n- do [#gamma]\n"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn apply_node_patches_ffi_rejects_unknown_op() {
        let doc =
            CString::new("<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n").unwrap();
        let patches =
            CString::new(r#"[{"component":"queue","node_key":"queue:0:alpha:0","op":"unknown"}]"#)
                .unwrap();

        let result = unsafe { agent_doc_apply_node_patches(doc.as_ptr(), patches.as_ptr()) };

        assert!(result.text.is_null());
        assert!(!result.error.is_null());
        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(error.contains("unsupported node patch op"));
        unsafe { agent_doc_free_string(result.error) };
    }

    #[test]
    fn admin_queue_control_ffi_returns_typed_receipt_json() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin-ffi.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-admin-ffi\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_orchestration::session_actor::record_session_start_direct(
            &doc,
            "session-admin-ffi",
            "%41",
            "@1",
            1,
        )
        .unwrap();

        let root = CString::new(dir.path().to_string_lossy().to_string()).unwrap();
        let document = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let action = CString::new("pause").unwrap();
        let reason = CString::new("ffi pause").unwrap();
        let accepted = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                1,
                reason.as_ptr(),
                std::ptr::null(),
            )
        };
        let accepted = ffi_json_value(accepted);
        assert_eq!(accepted["operation_kind"], "queue_paused");
        assert_eq!(accepted["status"], "accepted");
        assert!(accepted["receipt_id"].as_u64().unwrap() > 0);

        let stale = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                0,
                reason.as_ptr(),
                std::ptr::null(),
            )
        };
        let stale = ffi_json_value(stale);
        assert_eq!(stale["status"], "rejected");
        assert_eq!(stale["failed_stage"], "stale_generation");
        assert_eq!(stale["current_generation"], 1);
    }

    #[test]
    fn admin_inspect_ffi_returns_queue_diagnostics_json() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("tasks/admin-inspect-ffi.md");
        std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
        std::fs::write(
            &doc,
            "---\nagent_doc_session: session-inspect-ffi\nagent: codex\n---\nBody\n",
        )
        .unwrap();
        agent_doc_orchestration::session_actor::record_session_start_direct(
            &doc,
            "session-inspect-ffi",
            "%42",
            "@1",
            1,
        )
        .unwrap();

        let root = CString::new(dir.path().to_string_lossy().to_string()).unwrap();
        let document = CString::new(doc.to_string_lossy().to_string()).unwrap();
        let action = CString::new("drain").unwrap();
        let reason = CString::new("ffi drain").unwrap();
        let item_id = CString::new("next").unwrap();
        let receipt = unsafe {
            agent_doc_admin_queue_control_json(
                root.as_ptr(),
                document.as_ptr(),
                action.as_ptr(),
                1,
                reason.as_ptr(),
                item_id.as_ptr(),
            )
        };
        assert_eq!(ffi_json_value(receipt)["status"], "accepted");

        let inspection = unsafe {
            agent_doc_admin_inspect_json(
                root.as_ptr(),
                document.as_ptr(),
                std::ptr::null(),
                std::ptr::null(),
            )
        };
        let inspection = ffi_json_value(inspection);
        assert_eq!(inspection["record"]["pane_id"], "%42");
        assert_eq!(inspection["queue_control"]["state"], "draining");
        assert!(
            inspection["admin_operations"]
                .as_array()
                .unwrap()
                .iter()
                .any(|operation| operation["operation_kind"] == "queue_draining"
                    && operation["status"] == "accepted")
        );
    }

    #[test]
    fn visual_tokens_json_uses_utf16_document_offsets() {
        let doc = "\
é
😀
❯ do #qey0. spec-test-build-install-commit-push
### Re: café — gpt-5
- [recommended] next
";
        let tokens = parse_visual_tokens_json(doc);
        let find = |kind: &str| {
            tokens
                .iter()
                .find(|token| token["kind"] == kind)
                .unwrap_or_else(|| panic!("missing token kind {kind}"))
        };

        let prompt = find("prompt");
        assert_eq!(
            prompt["start"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n")
        );
        assert_eq!(
            prompt["end"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n❯ do #qey0. spec-test-build-install-commit-push")
        );

        let heading = find("response_heading");
        assert_eq!(
            heading["start"].as_u64().unwrap() as usize,
            utf16_len("é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n")
        );
        assert_eq!(
            heading["end"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5"
            )
        );

        let label = find("label_tag");
        assert_eq!(
            label["start"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5\n- "
            )
        );
        assert_eq!(
            label["end"].as_u64().unwrap() as usize,
            utf16_len(
                "é\n😀\n❯ do #qey0. spec-test-build-install-commit-push\n### Re: café — gpt-5\n- [recommended]"
            )
        );
    }

    #[test]
    fn apply_patch_replace() {
        let doc = "<!-- agent:output -->\nold\n<!-- /agent:output -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_name = CString::new("output").unwrap();
        let c_content = CString::new("new content\n").unwrap();
        let c_mode = CString::new("replace").unwrap();
        let result = unsafe {
            agent_doc_apply_patch(
                c_doc.as_ptr(),
                c_name.as_ptr(),
                c_content.as_ptr(),
                c_mode.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("new content"));
        assert!(!text.contains("old"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn apply_patch_with_boundary_marks_new_exchange_heading_with_head() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: earlier — gpt-5\nBody.\n<!-- agent:boundary:abc12345 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_name = CString::new("exchange").unwrap();
        let c_content = CString::new("### Re: latest — gpt-5\nNew body.\n").unwrap();
        let c_mode = CString::new("append").unwrap();
        let c_boundary = CString::new("abc12345").unwrap();
        let result = unsafe {
            agent_doc_apply_patch_with_boundary(
                c_doc.as_ptr(),
                c_name.as_ptr(),
                c_content.as_ptr(),
                c_mode.as_ptr(),
                c_boundary.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("### Re: earlier — gpt-5\n"));
        assert!(text.contains("### Re: latest — gpt-5 (HEAD)\n"));
        assert_eq!(text.matches("(HEAD)").count(), 1, "got:\n{text}");
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn normalize_template_structure_ffi_repairs_safe_duplicate_scaffold() {
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- /agent:exchange -->\n",
            "###\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#aaaa] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let c_doc = CString::new(doc).unwrap();

        let result = unsafe { agent_doc_normalize_template_structure(c_doc.as_ptr()) };

        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert_eq!(text.matches("<!-- /agent:exchange -->").count(), 1);
        assert_eq!(text.matches("<!-- agent:queue -->").count(), 1);
        assert_eq!(text.matches("<!-- agent:backlog -->").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn normalize_template_structure_ffi_rejects_mixed_duplicate_scaffold() {
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
        let c_doc = CString::new(doc).unwrap();

        let result = unsafe { agent_doc_normalize_template_structure(c_doc.as_ptr()) };

        assert!(result.text.is_null());
        assert!(!result.error.is_null());
        let error = unsafe { CStr::from_ptr(result.error) }.to_str().unwrap();
        assert!(
            error.contains("mixed duplicate scaffold"),
            "unexpected error: {error}"
        );
        unsafe { agent_doc_free_string(result.error) };
    }

    #[test]
    fn merge_frontmatter_adds_field() {
        let doc = "---\nagent_doc_session: abc\n---\nBody\n";
        let fields = "model: opus";
        let c_doc = CString::new(doc).unwrap();
        let c_fields = CString::new(fields).unwrap();
        let result = unsafe { agent_doc_merge_frontmatter(c_doc.as_ptr(), c_fields.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("model: opus"));
        assert!(text.contains("agent_doc_session: abc"));
        assert!(text.contains("Body"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_boundary_removes_stale() {
        let doc = "<!-- agent:exchange patch=append -->\ntext\n<!-- agent:boundary:aaaa1111 -->\nmore\n<!-- agent:boundary:bbbb2222 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_reposition_boundary_to_end(c_doc.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        // Should have exactly one boundary marker at the end
        let boundary_count = text.matches("<!-- agent:boundary:").count();
        assert_eq!(
            boundary_count, 1,
            "should have exactly 1 boundary, got {}",
            boundary_count
        );
        // The boundary should be just before the close tag
        assert!(text.contains("more\n<!-- agent:boundary:"));
        assert!(text.contains(" -->\n<!-- /agent:exchange -->"));
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_boundary_with_id_reuses_requested_marker() {
        let doc = "<!-- agent:exchange patch=append -->\ntext\n<!-- agent:boundary:aaaa1111 -->\nmore\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_id = CString::new("keep-this-id").unwrap();
        let result =
            unsafe { agent_doc_reposition_boundary_to_end_with_id(c_doc.as_ptr(), c_id.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(text.contains("<!-- agent:boundary:keep-this-id -->"));
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_preserve_head_keeps_head_markers() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: topic (HEAD)\ntext\n<!-- agent:boundary:aaaa1111 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let result = unsafe { agent_doc_reposition_boundary_to_end_preserve_head(c_doc.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(
            text.contains("### Re: topic (HEAD)"),
            "preserve_head FFI must keep (HEAD); got:\n{text}"
        );
        assert!(!text.contains("aaaa1111"), "old boundary gone");
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn reposition_preserve_head_with_id_keeps_head_and_id() {
        let doc = "<!-- agent:exchange patch=append -->\n### Re: topic (HEAD)\ntext\n<!-- agent:boundary:aaaa1111 -->\n<!-- /agent:exchange -->\n";
        let c_doc = CString::new(doc).unwrap();
        let c_id = CString::new("my-id").unwrap();
        let result = unsafe {
            agent_doc_reposition_boundary_to_end_preserve_head_with_id(
                c_doc.as_ptr(),
                c_id.as_ptr(),
            )
        };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert!(
            text.contains("### Re: topic (HEAD)"),
            "preserve_head FFI must keep (HEAD); got:\n{text}"
        );
        assert!(
            text.contains("<!-- agent:boundary:my-id -->"),
            "explicit id used; got:\n{text}"
        );
        assert_eq!(text.matches("<!-- agent:boundary:").count(), 1);
        unsafe { agent_doc_free_string(result.text) };
    }

    #[test]
    fn is_idle_untracked_returns_true() {
        let path = CString::new("/tmp/ffi-test-untracked-file.md").unwrap();
        let result = unsafe { agent_doc_is_idle(path.as_ptr(), 500) };
        assert_eq!(result, 1, "untracked file should report idle");
    }

    #[test]
    fn is_idle_after_change_returns_false() {
        let path = CString::new("/tmp/ffi-test-just-changed.md").unwrap();
        unsafe { agent_doc_document_changed(path.as_ptr()) };
        let result = unsafe { agent_doc_is_idle(path.as_ptr(), 2000) };
        assert_eq!(
            result, 0,
            "file changed <2s ago should not be idle with 2000ms window"
        );
    }

    #[test]
    fn resolve_project_path_finds_nested_submodule() {
        use tempfile::TempDir;
        let outer = TempDir::new().unwrap();
        // Outer project root has .agent-doc/
        std::fs::create_dir_all(outer.path().join(".agent-doc")).unwrap();
        // Nested submodule with its own .agent-doc/
        let sub = outer.path().join("src/session-share");
        std::fs::create_dir_all(sub.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(sub.join("tasks")).unwrap();
        let doc = sub.join("tasks/claudescore.md");
        std::fs::write(&doc, "# test\n").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        assert!(
            !result.project_root.is_null(),
            "project_root should be non-null"
        );
        assert!(
            !result.relative_path.is_null(),
            "relative_path should be non-null"
        );

        let root = unsafe { CStr::from_ptr(result.project_root) }
            .to_str()
            .unwrap();
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        // Nearest .agent-doc/ is the submodule, not the outer project.
        let expected_root = sub.canonicalize().unwrap();
        assert_eq!(std::path::Path::new(root), expected_root);
        assert_eq!(rel, "tasks/claudescore.md");

        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn resolve_project_path_prefers_nearest_ancestor() {
        use tempfile::TempDir;
        let outer = TempDir::new().unwrap();
        std::fs::create_dir_all(outer.path().join(".agent-doc")).unwrap();
        let mid = outer.path().join("mid");
        std::fs::create_dir_all(mid.join(".agent-doc")).unwrap();
        let deep = mid.join("deep/subdir");
        std::fs::create_dir_all(&deep).unwrap();
        let doc = deep.join("doc.md");
        std::fs::write(&doc, "").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        let root = unsafe { CStr::from_ptr(result.project_root) }
            .to_str()
            .unwrap();
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        assert_eq!(
            std::path::Path::new(root),
            mid.canonicalize().unwrap(),
            "should prefer nearest (mid) over outer"
        );
        assert_eq!(rel, "deep/subdir/doc.md");
        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn resolve_project_path_file_directly_in_root() {
        use tempfile::TempDir;
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("plan.md");
        std::fs::write(&doc, "").unwrap();

        let c_path = CString::new(doc.to_str().unwrap()).unwrap();
        let result = unsafe { agent_doc_resolve_project_path(c_path.as_ptr()) };
        assert!(!result.project_root.is_null());
        let rel = unsafe { CStr::from_ptr(result.relative_path) }
            .to_str()
            .unwrap();
        assert_eq!(rel, "plan.md");
        unsafe {
            agent_doc_free_string(result.project_root);
            agent_doc_free_string(result.relative_path);
        }
    }

    #[test]
    fn crdt_merge_no_base() {
        let c_ours = CString::new("hello world").unwrap();
        let c_theirs = CString::new("hello world").unwrap();
        let result =
            unsafe { agent_doc_crdt_merge(ptr::null(), 0, c_ours.as_ptr(), c_theirs.as_ptr()) };
        assert!(result.error.is_null());
        assert!(!result.text.is_null());
        let text = unsafe { CStr::from_ptr(result.text) }.to_str().unwrap();
        assert_eq!(text, "hello world");
        unsafe {
            agent_doc_free_string(result.text);
            agent_doc_free_state(result.state, result.state_len);
        };
    }
