//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_element_exchange::{
    dedupe_adjacent_prompt_prefix_duplicates_in_doc, dedupe_live_prompt_prefix_variants_in_doc,
    dedupe_prompt_lines_against_before_doc, exchange_shrink_guard_block,
    live_exchange_without_ack_content_retry_required,
};
#[cfg(test)]
use agent_doc_element_exchange::{exchange_content, normalized_prompt_counts};

/// Guard against accidental exchange content truncation.
///
/// Compares the exchange component content in the current file against the
/// proposed content. If the existing exchange is substantial (>100 bytes) and
/// the new exchange is <10% of the old, refuse the write. Returns `Ok(())` if
/// the write should proceed, or an error message if it should be refused.
pub(crate) fn check_exchange_shrink_guard(
    content_at_start: &str,
    content_ours: &str,
    file: &Path,
) -> Result<()> {
    if let Some(block) = exchange_shrink_guard_block(
        content_at_start,
        content_ours,
        SHRINK_GUARD_MIN_BYTES,
        SHRINK_GUARD_MAX_RATIO,
    ) {
        crate::ops_log::log_op(
            file,
            &format!(
                "shrink_guard_blocked file={} old_len={} new_len={} ratio={:.3}",
                file.display(),
                block.old_exchange_len,
                block.new_exchange_len,
                block.ratio
            ),
        );
        anyhow::bail!(
            "exchange content would shrink from {} to {} bytes ({:.0}% of original) — \
             refusing write to prevent accidental truncation. If this is intentional, \
             use `agent-doc compact` or re-run with meaningful content.",
            block.old_exchange_len,
            block.new_exchange_len,
            block.ratio * 100.0
        );
    }

    Ok(())
}

pub(crate) fn file_ipc_consumed_without_live_exchange_ack(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    before: Option<&str>,
    after: &str,
    ack_content_proven: bool,
) -> bool {
    let Some(before) = before else {
        return false;
    };
    if !live_exchange_without_ack_content_retry_required(
        baseline,
        Some(before),
        after,
        ack_content_proven,
    ) {
        return false;
    }

    let before_hash = agent_doc_hash::content_hash(before);
    let after_hash = agent_doc_hash::content_hash(after);
    eprintln!(
        "[write] file IPC consumed for {} with live exchange edits but no ack-content proof and no exchange materialization — retry required before snapshot/commit",
        file.display()
    );
    crate::ops_log::log_op(
        file,
        &format!(
            "file_ipc_live_exchange_unacknowledged file={} source={} patch_id={} before_hash={} after_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            before_hash,
            after_hash
        ),
    );
    log_ipc_proof_failure(
        file,
        source,
        patch_id,
        "live_exchange_without_ack_content",
        "retry_without_disk_write",
        &format!("before_hash={} after_hash={}", before_hash, after_hash),
    );
    true
}

pub(crate) fn dedupe_live_prompt_prefix_variants_in_tail(
    content: &str,
    file: &Path,
) -> (String, bool) {
    let Some(repaired) = dedupe_live_prompt_prefix_variants_in_doc(content) else {
        return (content.to_string(), false);
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_prefix_variant_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

pub(crate) fn dedupe_adjacent_prompt_prefix_duplicates(
    content: &str,
    file: &Path,
) -> (String, bool) {
    let Some(repaired) = dedupe_adjacent_prompt_prefix_duplicates_in_doc(content) else {
        return (content.to_string(), false);
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_prefix_variant_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

pub(crate) fn dedupe_prompt_lines_against_before(
    before: &str,
    after: &str,
    file: &Path,
) -> (String, bool) {
    let Some(repaired) = dedupe_prompt_lines_against_before_doc(before, after) else {
        return (after.to_string(), false);
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "ipc_prompt_duplicate_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

pub(crate) fn remove_post_exchange_duplicate_prompt_comments_with_log(
    content: &str,
    file: &Path,
    source: &str,
    preserve_doc: Option<&str>,
    preserve_current_doc: Option<&str>,
) -> (String, bool) {
    let preserve_docs = [preserve_doc, preserve_current_doc]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let Some(cleaned) =
        agent_doc_template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
            content,
            &preserve_docs,
        )
    else {
        return (content.to_string(), false);
    };
    crate::ops_log::log_op(
        file,
        &format!(
            "post_exchange_duplicate_prompt_comment_removed file={} source={} before_commit=true",
            file.display(),
            source
        ),
    );
    (cleaned, true)
}

#[cfg(test)]
mod core_tests {
    #![allow(unused_imports)]
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn ipc_snapshot_dedupes_extra_prompt_copy_against_before_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = "<!-- agent:exchange patch=append -->\nOld content.\nlive prompt\n<!-- /agent:exchange -->\n";
        let after = "<!-- agent:exchange patch=append -->\nOld content.\n❯ live prompt\nlive prompt\n### Re: response\nDone.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), after, "test_ipc").unwrap();

        assert!(changed);
        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(
            !repaired.contains("❯ live prompt\nlive prompt"),
            "duplicate unprefixed prompt should be removed: {repaired}"
        );
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get("live prompt")
                .copied(),
            Some(1)
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("ipc_prompt_duplicate_repaired"));
        assert!(log.contains("ipc_snapshot_deduped"));
    }

    #[test]
    fn ipc_snapshot_dedupes_duplicate_singleton_component_from_before_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let before = concat!(
            "<!-- agent:status -->\n",
            "ready\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue priority go -->\n",
            "- do [#canonical]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#canonical] keep me\n",
            "<!-- /agent:backlog -->\n"
        );
        let after = before.replace(
            "<!-- agent:backlog -->",
            "<!-- agent:queue preset=\"#stale\" priority go -->\n- do [#stale]\n<!-- /agent:queue -->\n\n<!-- agent:backlog -->",
        );
        fs::write(&doc, before).unwrap();

        let (repaired, changed) =
            dedupe_ipc_snapshot_content(&doc, Some(before), &after, "test_ipc").unwrap();

        assert!(changed);
        assert_eq!(
            repaired.matches("<!-- agent:queue").count(),
            1,
            "duplicate singleton queue block must be removed:\n{repaired}"
        );
        assert!(repaired.contains("- do [#canonical]"));
        assert!(!repaired.contains("- do [#stale]"));
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&repaired),
            None
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("duplicate_singleton_component_repaired"));
        assert!(log.contains("ipc_snapshot_singleton_components_deduped"));
        assert!(log.contains("ipc_snapshot_deduped"));
    }

    #[test]
    fn duplicate_prompt_artifact_repair_runs_canonical_pipeline() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "Please keep this duplicate prompt around for canonical cleanup coverage #spec-test-build-install-commit-push";
        let prefix_short = "agent-doc on corky running opencode, the arrow key functionality works at first but once a turn starts the key log shows re ";
        let prefix_long = "agent-doc on corky running opencode, the arrow key functionality works at first but once a turn starts the key log shows received ";
        let before = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "{prompt}\n",
                "<!-- agent:boundary:old -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let after = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "{prompt}\n",
                "<!-- agent:boundary:new -->\n",
                "{prefix_short}\n",
                "{prefix_long}\n",
                "### Re: duplicate prompt cleanup — gpt-5\n\n",
                "Done.\n",
                "### Re: duplicate prompt cleanup — gpt-5\n\n",
                "Done.\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt,
            prefix_short = prefix_short,
            prefix_long = prefix_long
        );
        fs::write(&doc, &before).unwrap();

        let (repaired, report) = repair_duplicate_prompt_artifacts(
            &after,
            &doc,
            DuplicatePromptRepairOptions::new("test-canonical")
                .with_before(Some(&before))
                .preserving(Some(&before)),
        )
        .unwrap();

        assert_eq!(
            report,
            DuplicatePromptRepairReport {
                response_blocks: true,
                answered_tail: false,
                post_exchange_comments: true,
                prompt_lines_against_before: true,
                live_prefix_variants: true,
            }
        );
        assert_eq!(
            repaired.matches("### Re: duplicate prompt cleanup").count(),
            1,
            "duplicate response block should be removed:\n{repaired}"
        );
        assert!(repaired.contains(&format!("❯ {prompt}\n<!-- agent:boundary:new -->")));
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get(prompt)
                .copied(),
            Some(1)
        );
        assert!(
            !repaired.contains(&format!("❯ {prompt}\n{prompt}")),
            "before-content prompt duplicate should be removed:\n{repaired}"
        );
        assert!(!repaired.contains(prefix_short));
        assert!(repaired.contains(prefix_long));
        assert!(
            repaired.contains("\n<!--\n-->\n\n<!-- agent:backlog -->"),
            "post-exchange duplicate prompt comment should keep the comment shell:\n{repaired}"
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("duplicate_prompt_artifact_repair"));
        assert!(log.contains("source=test-canonical"));
        assert!(log.contains("response_blocks=true"));
        assert!(log.contains("post_exchange_comments=true"));
        assert!(log.contains("prompt_lines_against_before=true"));
        assert!(log.contains("live_prefix_variants=true"));
    }
    #[test]
    fn commit_prompt_repair_dedupes_exact_prefixed_raw_prompt_copy() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("diag.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let prompt = "lucas-huang may not have the necessary packages to use the runbooks. Please add development dependencies so any programmer can use the runbooks.";
        let snapshot = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        let current = format!(
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "{prompt}\n",
                "#spec-test-commit-push\n",
                "<!-- agent:boundary:edf37a04 -->\n",
                "<!-- /agent:exchange -->\n"
            ),
            prompt = prompt
        );
        fs::write(&doc, &current).unwrap();

        let repaired =
            repair_commit_prompt_artifacts_against_snapshot(&doc, &snapshot, &current).unwrap();

        assert!(repaired.contains(&format!("❯ {prompt}\n#spec-test-commit-push")));
        assert!(
            !repaired.contains(&format!("❯ {prompt}\n{prompt}")),
            "commit pre-stage repair should remove the raw duplicate:\n{repaired}"
        );
        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get(prompt)
                .copied(),
            Some(1)
        );
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("source=commit-pre-stage"));
        assert!(log.contains("prompt_lines_against_before=true"));
    }
    #[test]
    fn normalize_final_template_content_dedupes_direct_merge_prompt_copy() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("doc.md");
        fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let snapshot = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "<!-- agent:boundary:old -->\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n",
        );
        let merged = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "Old content.\n",
            "❯ live prompt\n",
            "live prompt\n",
            "### Re: response — gpt-5\n\n",
            "Done.\n",
            "<!-- agent:boundary:new -->\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, before).unwrap();

        let repaired = normalize_final_template_content(
            &doc,
            before,
            Some(snapshot),
            Some(before),
            merged,
            Some("### Re: response — gpt-5\n\nDone.\n"),
        )
        .unwrap();

        assert_eq!(
            normalized_prompt_counts(exchange_content(&repaired).unwrap())
                .get("live prompt")
                .copied(),
            Some(1)
        );
        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(repaired.contains("Done."));
        let log = fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(log.contains("ipc_prompt_duplicate_repaired"));
    }
}
