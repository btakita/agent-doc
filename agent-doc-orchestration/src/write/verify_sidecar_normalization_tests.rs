    use super::{enforce_orchestrate_template_patch_contract, verify_sidecar_normalization};

    #[test]
    fn empty_targets_always_passes() {
        assert!(verify_sidecar_normalization("anything", &[]));
    }

    #[test]
    fn all_targets_prefixed() {
        let sidecar = "some line\n❯ do #task1\n❯ do #task2\nother line";
        let targets = vec!["do #task1".to_string(), "do #task2".to_string()];
        assert!(verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn missing_prefix_detected() {
        let sidecar = "some line\n❯ do #task1\ndo #task2\nother line";
        let targets = vec!["do #task1".to_string(), "do #task2".to_string()];
        assert!(!verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn trailing_whitespace_mismatch_tolerated() {
        let sidecar = "❯ do #task1\n❯ do #task2  \n";
        let targets = vec!["do #task1  ".to_string(), "do #task2".to_string()];
        assert!(verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn blank_targets_skipped() {
        let sidecar = "❯ do #task1\nother";
        let targets = vec!["do #task1".to_string(), "".to_string(), "   ".to_string()];
        assert!(verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn target_at_start_of_sidecar() {
        let sidecar = "❯ first line\nrest";
        let targets = vec!["first line".to_string()];
        assert!(verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn target_not_in_sidecar_at_all() {
        let sidecar = "line one\nline two\n";
        let targets = vec!["nonexistent line".to_string()];
        assert!(!verify_sidecar_normalization(sidecar, &targets));
    }

    #[test]
    fn sidecar_missing_prefix_when_target_has_trailing_whitespace() {
        // Simulates the IntelliJ trailing-space bug: binary sent "do the thing "
        // (trailing space), IntelliJ stripped to "do the thing" in the buffer,
        // plugin's original exact-match failed silently, sidecar has no prefix.
        // verify_sidecar_normalization must detect this.
        let sidecar = "some other line\ndo the thing\nmore content";
        let targets = vec!["do the thing ".to_string()];
        assert!(
            !verify_sidecar_normalization(sidecar, &targets),
            "missing prefix must be detected even when target has trailing whitespace"
        );
    }

    #[test]
    fn orchestrate_contract_rejects_non_exchange_patch() {
        let patches = vec![crate::template::PatchBlock::new("status", "updated")];
        let err = enforce_orchestrate_template_patch_contract(Some("orchestrate"), &patches, "")
            .unwrap_err();
        assert!(err.to_string().contains("patch:exchange"));
    }

    #[test]
    fn orchestrate_contract_rejects_unmatched_transcript() {
        let patches = vec![crate::template::PatchBlock::new("exchange", "ok")];
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &patches,
            "### Re: raw transcript — gpt-5",
        )
        .unwrap_err();
        assert!(err.to_string().contains("raw unmatched content"));
    }

    #[test]
    fn orchestrate_contract_allows_exchange_only_patch() {
        let patches = vec![crate::template::PatchBlock::new("exchange", "ok")];
        enforce_orchestrate_template_patch_contract(Some("orchestrate"), &patches, "")
            .expect("exchange-only orchestrate patch should be accepted");
    }

    #[test]
    fn orchestrate_contract_allows_clean_plain_response() {
        enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "### Re: orchplainresp — gpt-5\n\nImplemented and verified.",
        )
        .expect("clean plain orchestrate response should synthesize exchange append");
    }

    #[test]
    fn orchestrate_contract_allows_explicit_multi_component_patch() {
        let patches = vec![
            crate::template::PatchBlock::new("exchange", "response"),
            crate::template::PatchBlock::new("status", "updated"),
        ];
        enforce_orchestrate_template_patch_contract(Some("orchestrate"), &patches, "")
            .expect("explicit multi-component patch should be accepted");
    }

    #[test]
    fn orchestrate_contract_rejects_plain_transcript_prompt_lines() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "### Re: topic — gpt-5\n\nDone.\n❯ do #next",
        )
        .unwrap_err();
        assert!(err.to_string().contains("transcript prompt lines"));
    }

    #[test]
    fn orchestrate_contract_rejects_plain_transcript_headings() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "## User\nrequest\n\n## Assistant\nresponse",
        )
        .unwrap_err();
        assert!(err.to_string().contains("transcript headings"));
    }

    #[test]
    fn orchestrate_contract_rejects_plain_full_document_dump() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "<!-- agent:exchange -->\n### Re: topic — gpt-5\n<!-- /agent:exchange -->",
        )
        .unwrap_err();
        assert!(err.to_string().contains("component markers"));
    }

    #[test]
    fn orchestrate_contract_rejects_sanitized_full_document_dump() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "&lt;!-- agent:exchange --&gt;\n### Re: topic — gpt-5\n&lt;!-- /agent:exchange --&gt;",
        )
        .unwrap_err();
        assert!(err.to_string().contains("component markers"));
    }

    #[test]
    fn orchestrate_contract_rejects_multiple_plain_responses() {
        let err = enforce_orchestrate_template_patch_contract(
            Some("orchestrate"),
            &[],
            "### Re: first — gpt-5\n\nOne.\n\n### Re: second — gpt-5\n\nTwo.",
        )
        .unwrap_err();
        assert!(err.to_string().contains("only one assistant response"));
    }

    #[test]
    fn template_response_write_proof_accepts_nonempty_unmatched_body() {
        let proof = super::template_response_write_proof(&[], "### Re: topic — gpt-5\nbody\n");
        assert!(proof.has_real_body());
        assert_eq!(proof.unmatched_len, "### Re: topic — gpt-5\nbody".len());
    }

    #[test]
    fn template_response_write_proof_rejects_empty_response_shells() {
        let patches = vec![
            crate::template::PatchBlock::new("exchange", ""),
            crate::template::PatchBlock::new("frontmatter", "agent: codex"),
        ];
        let err = super::ensure_template_response_write_proof(&patches, "").unwrap_err();
        assert!(err.to_string().contains("no real response-body write"));
    }
