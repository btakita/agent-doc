//! Write normalization and response-contract IO adapters.

use anyhow::Result;
use std::path::Path;

#[cfg(test)]
use agent_doc_document::write_normalization::lift_pending_from_exchange;
#[cfg(test)]
use agent_doc_element_exchange::normalize_user_prompts_in_exchange;
#[cfg(test)]
use agent_doc_element_exchange::{extract_normalization_targets, verify_visible_normalization};

pub fn enforce_imperative_response_contract(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response: &str,
) -> Result<()> {
    enforce_imperative_response_contract_with_mutation_evidence(
        file,
        baseline,
        current_content,
        response,
        false,
    )
}

/// `#queuemirrororder`: same contract, but the caller can assert that the write
/// itself carries a concrete tracked-work mutation (a reorder or a text/metadata
/// edit) whose only possible deliverable is the document change.
///
/// Such a write has no command output, no code diff, and no commit to cite, so the
/// prose heuristics below reject every honest response it can produce — observed
/// live, where a pure `--backlog-reorder` answering `do [#typeroutersponses]` was
/// rejected as "status-only/meta" and left the cycle stranded at
/// `response_captured` with no way forward. The binary applied the mutation, which
/// is stronger evidence than any prose scan, so the guard defers to it.
pub fn enforce_imperative_response_contract_with_mutation_evidence(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response: &str,
    mutation_evidence: bool,
) -> Result<()> {
    if mutation_evidence {
        return Ok(());
    }
    let baseline_owned = baseline.map(ToOwned::to_owned).or_else(|| {
        agent_doc_snapshot_io::load_document_baseline(file)
            .ok()
            .flatten()
    });
    let Some(base) = baseline_owned.as_deref() else {
        return Ok(());
    };
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(base, current_content) else {
        return Ok(());
    };
    enforce_imperative_response_contract_for_diff(file, &diff_text, response)
}

pub fn enforce_imperative_response_contract_for_diff(
    file: &Path,
    diff_text: &str,
    response: &str,
) -> Result<()> {
    use agent_doc_turn::response_text::{
        ImperativeResponseContractDecision, imperative_response_contract_decision,
        truncate_imperative_trigger,
    };

    let trigger = match imperative_response_contract_decision(diff_text, response) {
        ImperativeResponseContractDecision::NoImperativeDirective
        | ImperativeResponseContractDecision::Satisfied => return Ok(()),
        ImperativeResponseContractDecision::Rejected { trigger } => trigger,
    };
    let trigger = truncate_imperative_trigger(&trigger, 80);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "imperative_response_rejected file={} trigger={}",
            file.display(),
            trigger
        ),
    );
    anyhow::bail!(
        "imperative document directive requires concrete execution evidence or a concrete blocker; rejected status-only/meta response for `{}`",
        trigger
    );
}

pub fn template_mode_overrides_for_current_doc(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
) -> std::collections::HashMap<String, String> {
    let mut overrides = std::collections::HashMap::new();
    let baseline_owned = baseline.map(ToOwned::to_owned).or_else(|| {
        agent_doc_snapshot_io::load_document_baseline(file)
            .ok()
            .flatten()
    });
    let Some(base) = baseline_owned.as_deref() else {
        return overrides;
    };
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(base, current_content) else {
        return overrides;
    };
    if agent_doc_diff::detect_exchange_compaction_request(&diff_text) {
        overrides.insert("exchange".to_string(), "replace".to_string());
    }
    overrides
}

/// Safe wrapper around [`normalize_user_prompts_in_exchange`] that adds:
///
/// 1. **Forensic logging** — every call writes `normalize_user_prompts`
///    metrics (`snap_len`, `base_len`, `applied`) to `ops.log` so divergence
///    incidents can be caught in the wild.
/// 2. **Safety rail** — if more than the maximum safe prefix count
///    would be applied, the normalization is discarded (content passes
///    through unchanged) and an event is logged.
/// 3. **No broad recovery side effects** — on overrun, the content passes
///    through unchanged. The caller's typed repair/closeout path remains
///    responsible for deciding whether disk, snapshot, or editor state changes.
///
/// This is the call-site-facing entry point for the write path. Tests and
/// callers that need the pure normalization behavior should continue to
/// use [`normalize_user_prompts_in_exchange`].
pub fn normalize_user_prompts_in_exchange_safe(
    content: &str,
    baseline: &str,
    snapshot: &str,
    file: &std::path::Path,
) -> String {
    agent_doc_element_exchange_io::normalize_user_prompts_in_exchange_safe_with_log(
        content,
        baseline,
        snapshot,
        file,
        |file| agent_doc_git_io::revision::show_head(file).ok().flatten(),
        agent_doc_ops_log_io::log_op,
    )
}

#[cfg(test)]
mod imperative_contract_tests {
    use super::*;

    #[test]
    fn imperative_contract_rejects_status_only_response() {
        let file = Path::new("session.md");
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do #6zyp. run tests. build + install. commit + push\n";
        let err = enforce_imperative_response_contract_for_diff(
            file,
            diff,
            "### Re: task — gpt-5\nIn progress. Continuing now.",
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("imperative document directive requires concrete execution evidence or a concrete blocker"),
            "unexpected error: {err}"
        );
    }

    /// `#queuemirrororder` (3): a pure `--backlog-reorder` has no execution
    /// evidence to offer — no command output, no code diff, no commit hash. The
    /// prose heuristics rejected every honest response it could write and stranded
    /// the cycle at `response_captured`; the applied mutation is the evidence.
    #[test]
    fn imperative_contract_defers_to_applied_metadata_mutation() {
        let file = Path::new("session.md");
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do [#typeroutersponses]\n";
        let response = "### Re: typeroutersponses — gpt-5\nReordered the backlog so the registry runs first.";
        enforce_imperative_response_contract_with_mutation_evidence(
            file,
            Some("ctx\n"),
            "ctx\ndo [#typeroutersponses]\n",
            response,
            true,
        )
        .expect("an applied metadata mutation satisfies the contract");

        let err = enforce_imperative_response_contract_for_diff(file, diff, response).unwrap_err();
        assert!(
            err.to_string().contains(
                "imperative document directive requires concrete execution evidence or a concrete blocker"
            ),
            "without the mutation the same response is still rejected: {err}"
        );
    }

    #[test]
    fn imperative_contract_allows_concrete_blocker() {
        let file = Path::new("session.md");
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+do #6zyp. run tests. build + install. commit + push\n";
        enforce_imperative_response_contract_for_diff(
            file,
            diff,
            "### Re: blocked — gpt-5\nBlocked by missing `OPENROUTER_API_KEY`; build cannot proceed.",
        )
        .expect("blocker response should be accepted");
    }

    #[test]
    fn imperative_contract_allows_execution_evidence() {
        let file = Path::new("session.md");
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+go\n";
        enforce_imperative_response_contract_for_diff(
            file,
            diff,
            "### Re: done — gpt-5\nVerification:\n- `cargo test --manifest-path src/agent-doc/Cargo.toml`\nCommit / push:\n- `abc1234`\n",
        )
        .expect("evidence response should be accepted");
    }

    #[test]
    fn lift_pending_nested_inside_exchange() {
        let doc = "\
<!-- agent:exchange patch=append -->
some exchange content
<!-- agent:pending -->
- [ ] [#abc1] task one
<!-- /agent:pending -->
<!-- /agent:exchange -->
";
        let result = lift_pending_from_exchange(doc).unwrap();
        // pending should be after exchange close, not inside it
        let ex_close = result.find("<!-- /agent:exchange -->").unwrap();
        let pend_open = result.find("<!-- agent:pending").unwrap();
        assert!(
            pend_open > ex_close,
            "pending (at {}) should be after exchange close (at {})",
            pend_open,
            ex_close
        );
        // exchange content preserved
        assert!(result.contains("some exchange content"));
        // pending content preserved
        assert!(result.contains("- [ ] [#abc1] task one"));
    }

    #[test]
    fn lift_pending_already_sibling_returns_none() {
        let doc = "\
<!-- agent:exchange patch=append -->
exchange content
<!-- /agent:exchange -->

<!-- agent:pending -->
- [ ] [#abc1] task
<!-- /agent:pending -->
";
        assert!(lift_pending_from_exchange(doc).is_none());
    }

    #[test]
    fn lift_pending_no_exchange_returns_none() {
        let doc = "\
<!-- agent:pending -->
- [ ] [#abc1] task
<!-- /agent:pending -->
";
        assert!(lift_pending_from_exchange(doc).is_none());
    }

    #[test]
    fn lift_pending_no_pending_returns_none() {
        let doc = "\
<!-- agent:exchange patch=append -->
exchange content
<!-- /agent:exchange -->
";
        assert!(lift_pending_from_exchange(doc).is_none());
    }

    #[test]
    fn lift_pending_preserves_surrounding_content() {
        let doc = "\
---
title: test
---

<!-- agent:exchange patch=append -->
response here
<!-- agent:pending -->
- [ ] [#x1] item
<!-- /agent:pending -->
<!-- /agent:exchange -->

## Footer
";
        let result = lift_pending_from_exchange(doc).unwrap();
        assert!(result.contains("---\ntitle: test\n---"));
        assert!(result.contains("response here"));
        assert!(result.contains("## Footer"));
        // Verify ordering
        let ex_close = result.find("<!-- /agent:exchange -->").unwrap();
        let pend_open = result.find("<!-- agent:pending").unwrap();
        let footer = result.find("## Footer").unwrap();
        assert!(pend_open > ex_close, "pending after exchange close");
        assert!(footer > pend_open, "footer after pending");
    }
}

#[cfg(test)]
mod verify_visible_normalization_tests {
    use super::verify_visible_normalization;
    use agent_doc_template::patchback::enforce_orchestrate_patchback_contract;

    #[test]
    fn empty_targets_always_passes() {
        assert!(verify_visible_normalization("anything", &[]));
    }

    #[test]
    fn all_targets_prefixed() {
        let sidecar = "some line\n❯ do #task1\n❯ do #task2\nother line";
        let targets = vec!["do #task1".to_string(), "do #task2".to_string()];
        assert!(verify_visible_normalization(sidecar, &targets));
    }

    #[test]
    fn missing_prefix_detected() {
        let sidecar = "some line\n❯ do #task1\ndo #task2\nother line";
        let targets = vec!["do #task1".to_string(), "do #task2".to_string()];
        assert!(!verify_visible_normalization(sidecar, &targets));
    }

    #[test]
    fn trailing_whitespace_mismatch_tolerated() {
        let sidecar = "❯ do #task1\n❯ do #task2  \n";
        let targets = vec!["do #task1  ".to_string(), "do #task2".to_string()];
        assert!(verify_visible_normalization(sidecar, &targets));
    }

    #[test]
    fn blank_targets_skipped() {
        let sidecar = "❯ do #task1\nother";
        let targets = vec!["do #task1".to_string(), "".to_string(), "   ".to_string()];
        assert!(verify_visible_normalization(sidecar, &targets));
    }

    #[test]
    fn target_at_start_of_sidecar() {
        let sidecar = "❯ first line\nrest";
        let targets = vec!["first line".to_string()];
        assert!(verify_visible_normalization(sidecar, &targets));
    }

    #[test]
    fn target_not_in_sidecar_at_all() {
        let sidecar = "line one\nline two\n";
        let targets = vec!["nonexistent line".to_string()];
        assert!(!verify_visible_normalization(sidecar, &targets));
    }

    #[test]
    fn sidecar_missing_prefix_when_target_has_trailing_whitespace() {
        // Simulates the IntelliJ trailing-space bug: binary sent "do the thing "
        // (trailing space), IntelliJ stripped to "do the thing" in the buffer,
        // plugin's original exact-match failed silently, sidecar has no prefix.
        // verify_visible_normalization must detect this.
        let sidecar = "some other line\ndo the thing\nmore content";
        let targets = vec!["do the thing ".to_string()];
        assert!(
            !verify_visible_normalization(sidecar, &targets),
            "missing prefix must be detected even when target has trailing whitespace"
        );
    }

    #[test]
    fn orchestrate_contract_rejects_non_exchange_patch() {
        let patches = vec![agent_doc_template::PatchBlock::new("status", "updated")];
        let err =
            enforce_orchestrate_patchback_contract(Some("orchestrate"), &patches, "").unwrap_err();
        assert!(err.to_string().contains("patch:exchange"));
    }

    #[test]
    fn orchestrate_contract_rejects_unmatched_transcript() {
        let patches = vec![agent_doc_template::PatchBlock::new("exchange", "ok")];
        let err = enforce_orchestrate_patchback_contract(
            Some("orchestrate"),
            &patches,
            "### Re: raw transcript — gpt-5",
        )
        .unwrap_err();
        assert!(err.to_string().contains("raw unmatched content"));
    }

    #[test]
    fn orchestrate_contract_allows_exchange_only_patch() {
        let patches = vec![agent_doc_template::PatchBlock::new("exchange", "ok")];
        enforce_orchestrate_patchback_contract(Some("orchestrate"), &patches, "")
            .expect("exchange-only orchestrate patch should be accepted");
    }

    #[test]
    fn orchestrate_contract_allows_clean_plain_response() {
        enforce_orchestrate_patchback_contract(
            Some("orchestrate"),
            &[],
            "### Re: orchplainresp — gpt-5\n\nImplemented and verified.",
        )
        .expect("clean plain orchestrate response should synthesize exchange append");
    }

    #[test]
    fn orchestrate_contract_allows_explicit_multi_component_patch() {
        let patches = vec![
            agent_doc_template::PatchBlock::new("exchange", "response"),
            agent_doc_template::PatchBlock::new("status", "updated"),
        ];
        enforce_orchestrate_patchback_contract(Some("orchestrate"), &patches, "")
            .expect("explicit multi-component patch should be accepted");
    }

    #[test]
    fn orchestrate_contract_rejects_plain_transcript_prompt_lines() {
        let err = enforce_orchestrate_patchback_contract(
            Some("orchestrate"),
            &[],
            "### Re: topic — gpt-5\n\nDone.\n❯ do #next",
        )
        .unwrap_err();
        assert!(err.to_string().contains("transcript prompt lines"));
    }

    #[test]
    fn orchestrate_contract_rejects_plain_transcript_headings() {
        let err = enforce_orchestrate_patchback_contract(
            Some("orchestrate"),
            &[],
            "## User\nrequest\n\n## Assistant\nresponse",
        )
        .unwrap_err();
        assert!(err.to_string().contains("transcript headings"));
    }

    #[test]
    fn orchestrate_contract_rejects_plain_full_document_dump() {
        let err = enforce_orchestrate_patchback_contract(
            Some("orchestrate"),
            &[],
            "<!-- agent:exchange -->\n### Re: topic — gpt-5\n<!-- /agent:exchange -->",
        )
        .unwrap_err();
        assert!(err.to_string().contains("component markers"));
    }

    #[test]
    fn orchestrate_contract_rejects_sanitized_full_document_dump() {
        let err = enforce_orchestrate_patchback_contract(
            Some("orchestrate"),
            &[],
            "&lt;!-- agent:exchange --&gt;\n### Re: topic — gpt-5\n&lt;!-- /agent:exchange --&gt;",
        )
        .unwrap_err();
        assert!(err.to_string().contains("component markers"));
    }

    #[test]
    fn orchestrate_contract_rejects_multiple_plain_responses() {
        let err = enforce_orchestrate_patchback_contract(
            Some("orchestrate"),
            &[],
            "### Re: first — gpt-5\n\nOne.\n\n### Re: second — gpt-5\n\nTwo.",
        )
        .unwrap_err();
        assert!(err.to_string().contains("only one assistant response"));
    }

    #[test]
    fn template_response_write_proof_accepts_nonempty_unmatched_body() {
        let proof = agent_doc_template::response_materialization::template_response_write_proof(
            &[],
            "### Re: topic — gpt-5\nbody\n",
        );
        assert!(proof.has_real_body());
        assert_eq!(proof.unmatched_len, "### Re: topic — gpt-5\nbody".len());
    }

    #[test]
    fn template_response_write_proof_rejects_empty_response_shells() {
        let patches = vec![
            agent_doc_template::PatchBlock::new("exchange", ""),
            agent_doc_template::PatchBlock::new("frontmatter", "agent: codex"),
        ];
        let err =
            agent_doc_template::response_materialization::ensure_template_response_write_proof(
                &patches, "",
            )
            .unwrap_err();
        assert!(err.to_string().contains("no real response-body write"));
    }

    #[test]
    fn strict_template_response_heading_accepts_exchange_patch_heading() {
        let patches = vec![agent_doc_template::PatchBlock::new(
            "exchange",
            "### Re: queue head — gpt-5\n\nAnswered.\n",
        )];
        agent_doc_template::response_materialization::ensure_strict_template_response_heading(
            &patches, "",
        )
        .unwrap();
    }

    #[test]
    fn strict_template_response_heading_accepts_unmatched_heading() {
        agent_doc_template::response_materialization::ensure_strict_template_response_heading(
            &[],
            "### Re: queue head — gpt-5\n\nAnswered.\n",
        )
        .unwrap();
    }

    #[test]
    fn strict_template_response_heading_rejects_body_only_exchange_patch() {
        let patches = vec![agent_doc_template::PatchBlock::new(
            "exchange",
            "- changed paths\n- verification\n",
        )];
        let err =
            agent_doc_template::response_materialization::ensure_strict_template_response_heading(
                &patches, "",
            )
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("strict template closeout response")
        );
    }

    #[test]
    fn strict_template_response_heading_rejects_non_exchange_patch_only() {
        let patches = vec![agent_doc_template::PatchBlock::new(
            "status",
            "### Re: misplaced — gpt-5\n\nWrong component.\n",
        )];
        let err =
            agent_doc_template::response_materialization::ensure_strict_template_response_heading(
                &patches, "",
            )
            .unwrap_err();
        assert!(err.to_string().contains("patch:exchange"));
    }

    #[test]
    fn strict_template_response_heading_accepts_streamed_visible_prefix() {
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ do #stream. spec-test-build-install-commit-push\n",
            "<!-- patch:exchange -->\n",
            "### Re: streamed — gpt-5\n",
            "<!-- agent:boundary:streamed -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let patches = vec![agent_doc_template::PatchBlock::new(
            "exchange",
            "\nImplemented and verified.\n",
        )];

        agent_doc_template::response_materialization::ensure_strict_template_response_heading_for_current_doc(
            current, &patches, "",
        )
        .unwrap();
    }

    #[test]
    fn strict_template_response_heading_rejects_prior_heading_before_live_prompt() {
        let current = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\n",
            "Done.\n",
            "❯ do #new. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let patches = vec![agent_doc_template::PatchBlock::new(
            "exchange",
            "\nImplemented and verified.\n",
        )];
        let err =
            agent_doc_template::response_materialization::ensure_strict_template_response_heading_for_current_doc(
                current, &patches, "",
            )
            .unwrap_err();
        assert!(
            err.to_string()
                .contains("strict template closeout response")
        );
    }
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use std::fs;
    use std::time::Duration;

    #[test]
    fn normalize_user_prompts_new_line_gets_prefix() {
        let snapshot =
            "<!-- agent:exchange patch=append -->\nOld content.\n<!-- /agent:exchange -->\n";
        // baseline = user added "Hello" but agent hasn't responded yet
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld content.\nHello\n<!-- /agent:exchange -->\n";
        // content_ours = baseline + agent response appended (boundary at end after pre-patch)
        let content = "<!-- agent:exchange patch=append -->\nOld content.\nHello\n<!-- agent:boundary:abc123 -->\n### Re: response\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Hello"),
            "user line should get ❯  prefix: {}",
            result
        );
        assert!(
            result.contains("Old content."),
            "old content should be preserved"
        );
        assert!(
            result.contains("### Re: response"),
            "agent response should be preserved"
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should not get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_agent_response_not_prefixed() {
        // Regression: agent response lines in content_ours (before boundary) must NOT get ❯  prefix.
        // Before the fix, apply_patches_with_overrides moves the boundary to the end of exchange,
        // so the agent's response lines ended up in the "user region" and were incorrectly prefixed.
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        // baseline: user added "My question"
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld.\nMy question\n<!-- /agent:exchange -->\n";
        // content_ours: boundary at end (after pre-patch), agent response before it
        let content = "<!-- agent:exchange patch=append -->\nOld.\nMy question\nAgent answer here.\n<!-- agent:boundary:xyz -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ My question"),
            "user question should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Agent answer"),
            "agent response should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("Agent answer here."),
            "agent response should be preserved: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_compact_summary_not_prefixed() {
        // `#provauth3`: a compaction Session Summary is binary-authored. Relative
        // to the pre-compact snapshot every summary line is an Insert, so the
        // content-diff heuristic would otherwise stamp them all with `❯` — turning
        // the summary into a fake unresolved prompt. Origin is known, so none of
        // the summary lines may receive the prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: old - gpt-5\n\nA long archived body.\n<!-- /agent:exchange -->\n";
        let summary = "### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/x.md`*\n\nCompacted content:\n- Archived 6 response topic(s): a; b; c; 3 more\n- Prior summary/context: earlier work\n";
        let baseline =
            format!("<!-- agent:exchange patch=append -->\n{summary}<!-- /agent:exchange -->\n");
        let content = format!(
            "<!-- agent:exchange patch=append -->\n{summary}<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n"
        );
        let result = normalize_user_prompts_in_exchange(&content, &baseline, snapshot);
        assert!(
            !result.contains('❯'),
            "compaction summary lines must not get the ❯ prefix:\n{result}"
        );
    }
    #[test]
    fn normalize_user_prompts_replaced_response_body_under_existing_heading_not_prefixed() {
        // Regression #repair-orphan-prefix-bug: when an orphaned response is
        // applied by replacing a placeholder body UNDER AN EXISTING `### Re:`
        // heading (e.g. a direct Edit-based patchback swapping a "Hello world"
        // placeholder for the real multi-line body), the heading line is Equal
        // in the snapshot→baseline diff. The replacement body lines are Insert
        // lines and must still be recognized as assistant-response body, not
        // user prompts — they must NOT receive the `❯ ` prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nHello world\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nReal answer line one.\nReal answer line two.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ My question\n### Re: topic — opus-4-8\nReal answer line one.\nReal answer line two.\n<!-- agent:boundary:xyz -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ Real answer line one."),
            "replaced response body line one must NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Real answer line two."),
            "replaced response body line two must NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("Real answer line one.") && result.contains("Real answer line two."),
            "response body must be preserved verbatim: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_blank_line_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nOld.\n\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nOld.\n\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        // blank line should not get prefix
        assert!(
            !result.contains("❯ \n"),
            "blank line should not be prefixed: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_heading_treated_as_agent_content() {
        // Headings in the exchange are agent response markers. A standalone heading
        // (not ❯-prefixed) is treated as agent content and does NOT get the ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\n### My heading\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### My heading\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ### My heading"),
            "heading should NOT get prefix (treated as agent content): {}",
            result
        );
        assert!(
            result.contains("### My heading"),
            "heading should be preserved: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_hash_ref_prefixed() {
        // Regression for agent-doc-bugs #vnxg: a bare hash reference like `#zj6s` inside
        // the exchange user region was being skipped by the old `starts_with('#')` guard.
        // Under Option 2, the line is user input and must receive the ❯ prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\nprior turn\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nprior turn\n#zj6s\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nprior turn\n#zj6s\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ #zj6s"),
            "hash-ref line must get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_already_prefixed_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\n❯ Already prefixed\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Already prefixed\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ❯"),
            "should not double-prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Already prefixed"),
            "prefix should be preserved"
        );
    }
    #[test]
    fn normalize_user_prompts_existing_content_unchanged() {
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\nNew question\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\nNew question\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        // Previous question already prefixed — should not double-prefix
        assert!(
            !result.contains("❯ ❯"),
            "should not double-prefix existing content: {}",
            result
        );
        // New question should get prefix
        assert!(
            result.contains("❯ New question"),
            "new line should get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_keeps_inserted_assistant_question_bare() {
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ do #old
<!-- /agent:exchange -->
";
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Why did this happen?
This should stay answer prose.
<!-- /agent:exchange -->
";
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Why did this happen?
This should stay answer prose.
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);

        assert!(
            result.contains("\nWhy did this happen?\nThis should stay answer prose.\n"),
            "assistant question/prose must stay bare:\n{result}"
        );
        assert!(
            !result.contains("\n❯ Why did this happen?")
                && !result.contains("\n❯ This should stay answer prose."),
            "inserted assistant response lines must not be prompt-prefixed:\n{result}"
        );
    }
    #[test]
    fn normalize_user_prompts_still_prefixes_real_followup_after_inserted_response() {
        let snapshot = "\
<!-- agent:exchange patch=append -->
❯ do #old
<!-- /agent:exchange -->
";
        let baseline = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Done.

do #next. spec-test-build-install-commit-push
<!-- /agent:exchange -->
";
        let content = "\
<!-- agent:exchange patch=append -->
❯ do #old
### Re: old — gpt-5

Done.

do #next. spec-test-build-install-commit-push
<!-- agent:boundary:abc -->
<!-- /agent:exchange -->
";

        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);

        assert!(
            result.contains("\n❯ do #next. spec-test-build-install-commit-push\n"),
            "canonical prompt-target extraction must still prefix the follow-up:\n{result}"
        );
        assert!(
            result.contains("\nDone.\n"),
            "assistant response prose must stay bare:\n{result}"
        );
    }
    #[test]
    fn extract_normalization_targets_preserves_duplicate_lines() {
        let before = "<!-- agent:exchange patch=append -->\nQuestion?\nspec-test-build-install-commit-push\nQuestion?\nspec-test-build-install-commit-push\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\n❯ Question?\n❯ spec-test-build-install-commit-push\n❯ Question?\n❯ spec-test-build-install-commit-push\n<!-- /agent:exchange -->\n";

        let targets = extract_normalization_targets(before, after);

        assert_eq!(
            targets,
            vec![
                "Question?".to_string(),
                "spec-test-build-install-commit-push".to_string(),
                "Question?".to_string(),
                "spec-test-build-install-commit-push".to_string(),
            ]
        );
    }
    #[test]
    fn normalize_user_prompts_code_fence_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nSome text.\n```bash\necho hello\n```\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nSome text.\n```bash\necho hello\n```\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ ```"),
            "code fence marker should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ echo hello"),
            "code fence interior should not get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Some text."),
            "regular user line should get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_code_fence_interior_skipped() {
        // Multi-line code block with text before and after — only non-fence lines get prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nQuestion here.\n```rust\nlet x = 1;\nlet y = 2;\n```\nFollow-up.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nQuestion here.\n```rust\nlet x = 1;\nlet y = 2;\n```\nFollow-up.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Question here."),
            "text before fence should get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ Follow-up."),
            "text after fence should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ let x"),
            "fence interior should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ let y"),
            "fence interior should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ```"),
            "fence marker should not get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_tilde_fence_interior_skipped() {
        // ~~~ fences must be tracked the same as ``` fences.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nBefore.\n~~~sh\necho hello\n~~~\nAfter.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nBefore.\n~~~sh\necho hello\n~~~\nAfter.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Before."),
            "text before tilde fence should get prefix: {result}"
        );
        assert!(
            result.contains("❯ After."),
            "text after tilde fence should get prefix: {result}"
        );
        assert!(
            !result.contains("❯ echo hello"),
            "tilde fence interior should not get prefix: {result}"
        );
        assert!(
            !result.contains("❯ ~~~"),
            "tilde fence marker should not get prefix: {result}"
        );
    }
    #[test]
    fn normalize_user_prompts_quoted_string_prefixed() {
        // Option 2 invariant: a quoted string the user typed is still user input,
        // so it gets the ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n\"Merge conflict with external write\"\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n\"Merge conflict with external write\"\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ \"Merge conflict"),
            "quoted user line should get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_no_exchange_passthrough() {
        let content = "No exchange here.\n";
        let baseline = "No exchange here.\n";
        let snapshot = "";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert_eq!(
            result, content,
            "document without exchange should pass through unchanged"
        );
    }
    #[test]
    fn normalize_user_prompts_restores_prefix_lost_in_file() {
        // Regression: snapshot has ❯ do but the editor file (baseline) has do without prefix.
        // This happens when the IPC normalization fails to update the editor file.
        // The binary must restore ❯  so the snapshot stays correct and the
        // next IPC write carries normalize_prefix_lines with the correct prefix target.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ done\n❯ do\n- [ ] task\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ done\ndo\n- [ ] task\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ done\ndo\n- [ ] task\n<!-- agent:boundary:abc123:doc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ do"),
            "❯  prefix must be restored when snapshot had it but file lost it: {}",
            result
        );
        assert!(
            !result.contains("\ndo\n"),
            "bare do line must not remain without prefix: {}",
            result
        );
        // ❯ done must not be double-prefixed
        assert!(!result.contains("❯ ❯"), "no double-prefix: {}", result);
    }
    #[test]
    fn normalize_user_prompts_heading_replacement_does_not_swallow_next_prompt() {
        // Regression: commit-time `(HEAD)` churn replaces an existing response heading,
        // which shows up as Delete+Insert in snapshot→baseline. That replacement must
        // not reopen an agent block and suppress ❯ prefixing for the following user line.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4\nAgent answer.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4 (HEAD)\nAgent answer.\nfix #vedj. add spec + tests. build + install for local testing\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Existing prompt\n### Re: topic — gpt-5.4 (HEAD)\nAgent answer.\nfix #vedj. add spec + tests. build + install for local testing\n<!-- agent:boundary:abc123 -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("### Re: topic — gpt-5.4 (HEAD)"),
            "replacement heading should be preserved: {}",
            result
        );
        assert!(
            result.contains("Agent answer."),
            "existing agent body should be preserved: {}",
            result
        );
        assert!(
            result.contains("❯ fix #vedj. add spec + tests. build + install for local testing"),
            "new user prompt should get prefix despite heading replacement: {}",
            result
        );
        assert!(
            !result.contains("❯ Agent answer."),
            "existing agent body should not be prefixed: {}",
            result
        );
        assert!(
            !result.contains("❯ ### Re: topic"),
            "replacement heading should not be prefixed: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_agent_table_rows_not_prefixed() {
        // Core bug: stale snapshot causes agent response table rows (inside ### Re: blocks)
        // to appear as Insert lines and incorrectly receive ❯ prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Question\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Question\n### Re: analysis — opus-4-6\n| model | score |\n|-------|-------|\n| gpt-4 | 85.0 |\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Question\n### Re: analysis — opus-4-6\n| model | score |\n|-------|-------|\n| gpt-4 | 85.0 |\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ |"),
            "table rows inside agent response should NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("| model | score |"),
            "table content should be preserved: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_agent_subheadings_not_prefixed() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n### Re: topic\nSome text.\n#### Details\nMore text.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### Re: topic\nSome text.\n#### Details\nMore text.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ "),
            "no lines should get prefix — all are agent content: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_user_text_after_equal_heading() {
        // Heading is Equal (in snapshot), user adds text after it. User text gets ❯ prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\nNew user input\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Old question\n### Re: answer\nOld answer.\nNew user input\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ New user input"),
            "user text after Equal heading should get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_agent_block_ends_at_prompt() {
        // Agent block (Insert heading) ends when ❯-prefixed line appears.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n### Re: answer\nAgent text.\n❯ New question\nFollow-up text.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### Re: answer\nAgent text.\n❯ New question\nFollow-up text.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            !result.contains("❯ Agent text"),
            "agent text should NOT get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "agent heading should NOT get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ New question"),
            "already-prefixed line should be preserved: {}",
            result
        );
        assert!(
            result.contains("❯ Follow-up text."),
            "user text after ❯ should get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_heading_in_fence_not_agent_block() {
        // A heading inside a code fence is code, not an agent response marker.
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nBefore.\n```md\n### Not a real heading\nSome code.\n```\nAfter.\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nBefore.\n```md\n### Not a real heading\nSome code.\n```\nAfter.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(
            result.contains("❯ Before."),
            "text before fence should get prefix: {}",
            result
        );
        assert!(
            result.contains("❯ After."),
            "text after fence should get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ ###"),
            "heading inside fence should not get prefix: {}",
            result
        );
        assert!(
            !result.contains("❯ Some code"),
            "code inside fence should not get prefix: {}",
            result
        );
    }
    #[test]
    fn normalize_user_prompts_multiline_prompt_after_stale_response_gets_prefix() {
        // Regression for #pfxstrip2: when a stale snapshot makes the previous
        // assistant response appear as inserted content, the normalizer enters
        // agent-block mode. A blank-separated fresh prompt run after that
        // response is still user input, and every nonblank prompt line needs
        // the prompt prefix.
        let snapshot =
            "<!-- agent:exchange patch=append -->\n❯ Previous prompt\n<!-- /agent:exchange -->\n";
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Previous prompt\n",
            "### Re: previous — gpt-5\n",
            "Implemented and verified.\n",
            "\n",
            "Please increment version to v0.1.1. Release to github. Create a plan for rollout.\n",
            "Miguel will be integrating the demo into the partner workspace.\n",
            "\n",
            "Please rename the gh repo ClaudeScore/buildparty-investor-demo to the final name.\n",
            "Also, please draft slack instructions for robert-ross and miguel-mendez.\n",
            "\n",
            "spec-test-news-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = baseline.replace(
            "<!-- /agent:exchange -->",
            "<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->",
        );

        let result = normalize_user_prompts_in_exchange(&content, baseline, snapshot);

        for expected in [
            "❯ Please increment version to v0.1.1. Release to github. Create a plan for rollout.",
            "❯ Miguel will be integrating the demo into the partner workspace.",
            "❯ Please rename the gh repo ClaudeScore/buildparty-investor-demo to the final name.",
            "❯ Also, please draft slack instructions for robert-ross and miguel-mendez.",
            "❯ spec-test-news-commit-push",
        ] {
            assert!(
                result.contains(expected),
                "missing expected prefixed prompt line {expected:?}:\n{result}"
            );
        }
        assert!(
            !result.contains("❯ Implemented and verified."),
            "stale assistant response body must stay unprefixed:\n{result}"
        );
    }
    #[test]
    fn normalize_safe_passes_through_under_threshold() {
        // Small diff (1 user-added line) — should behave exactly like the pure function.
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();

        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld.\nHello\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nOld.\nHello\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("❯ Hello"),
            "under threshold, ❯ prefix should still be applied: {result}"
        );
    }
    #[test]
    fn normalize_safe_preserves_unprefixed_agent_lines_from_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "run follow-up\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: deployed — gpt-5\n",
            "Done:\n",
            "- build passed\n",
            "run follow-up\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("\nDone:\n- build passed\n"),
            "committed agent response lines from HEAD must stay unprefixed:\n{result}"
        );
        assert!(
            result.contains("\n❯ run follow-up\n"),
            "new user prompt should still be prefixed:\n{result}"
        );
    }
    #[test]
    fn normalize_safe_preserves_prior_response_tail_before_new_prompt() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "<!-- /agent:exchange -->\n",
        );
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "do [#pfxleak3]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: previous closeout — gpt-5\n",
            "Verification:\n",
            "- All 506 assertions pass.\n",
            "Committed + pushed buildparty-investor-demo and session-share.\n",
            "do [#pfxleak3]. spec-test-build-install-commit-push\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains(
                "\n- All 506 assertions pass.\nCommitted + pushed buildparty-investor-demo and session-share.\n❯ do [#pfxleak3]. spec-test-build-install-commit-push\n"
            ),
            "prior response tail must stay bare and only the new prompt may be prefixed:\n{result}"
        );
        assert!(
            !result.contains("\n❯ - All 506 assertions pass.\n")
                && !result.contains(
                    "\n❯ Committed + pushed buildparty-investor-demo and session-share.\n"
                ),
            "assistant tail lines from HEAD must not gain prompt prefixes:\n{result}"
        );
    }
    #[test]
    fn normalize_safe_preserves_prefixed_user_lines_from_head() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();

        let file = root.join("doc.md");
        let head = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please increment version to v0.1.1.\n",
            "❯ Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, head).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        let snapshot = head;
        let baseline = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Please increment version to v0.1.1.\n",
            "Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Please increment version to v0.1.1.\n",
            "Miguel will be integrating the demo.\n",
            "### Re: done — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:abc -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let result = normalize_user_prompts_in_exchange_safe(content, baseline, snapshot, &file);
        assert!(
            result.contains("❯ Please increment version to v0.1.1."),
            "HEAD-prefixed first prompt line must regain its prefix:\n{result}"
        );
        assert!(
            result.contains("❯ Miguel will be integrating the demo."),
            "HEAD-prefixed continuation line must regain its prefix:\n{result}"
        );
        assert!(
            !result.contains("\nPlease increment version to v0.1.1.\n"),
            "bare first prompt line must not remain:\n{result}"
        );
    }
    #[test]
    fn normalize_safe_bails_over_threshold() {
        // Construct a baseline with >50 unique "user-added" lines relative to the snapshot.
        // The safety rail should refuse to apply ❯ prefix and return content unchanged.
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["init", "-q"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test User"])
            .output()
            .unwrap();
        let file = root.join("doc.md");
        std::fs::write(&file, "initial\n").unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "doc.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();
        let head_before = std::process::Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;

        let mut baseline_lines = String::new();
        let mut content_lines = String::new();
        for i in 0..60 {
            baseline_lines.push_str(&format!("user line {i}\n"));
            content_lines.push_str(&format!("user line {i}\n"));
        }
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = format!(
            "<!-- agent:exchange patch=append -->\n{baseline_lines}<!-- /agent:exchange -->\n"
        );
        let content = format!(
            "<!-- agent:exchange patch=append -->\n{content_lines}<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n"
        );

        let result = normalize_user_prompts_in_exchange_safe(&content, &baseline, snapshot, &file);
        // No ❯ prefix should be applied — content should be returned unchanged.
        assert_eq!(
            result, content,
            "over threshold, content should pass through unchanged"
        );
        assert!(
            !result.contains("❯ user line"),
            "no ❯ prefix should be applied when threshold exceeded"
        );
        let head_after = std::process::Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        assert_eq!(
            head_after, head_before,
            "normalization overrun must not force-commit the working tree"
        );
    }
    #[test]
    fn normalize_safe_threshold_exact_boundary() {
        // Exactly 50 lines — at threshold, still applies prefix (strictly greater-than check).
        let tmp = tempfile::TempDir::new().unwrap();
        let file = tmp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();

        let mut lines = String::new();
        for i in 0..50 {
            lines.push_str(&format!("line {i}\n"));
        }
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline =
            format!("<!-- agent:exchange patch=append -->\n{lines}<!-- /agent:exchange -->\n");
        let content = format!(
            "<!-- agent:exchange patch=append -->\n{lines}<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n"
        );

        let result = normalize_user_prompts_in_exchange_safe(&content, &baseline, snapshot, &file);
        // At exactly 50, prefix should be applied (> is strict).
        assert!(
            result.contains("❯ line 0"),
            "at threshold, first line should get prefix: {result}"
        );
        assert!(
            result.contains("❯ line 49"),
            "at threshold, last line should get prefix: {result}"
        );
    }
}
