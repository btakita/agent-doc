//! Exchange element I/O adapters.
//!
//! Pure exchange reconciliation lives in `agent-doc-element-exchange` and
//! `agent-doc-template`. This crate owns the path-aware logging adapters used by
//! write-side orchestration.

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_document::write_normalization::{
    MAX_NORMALIZE_USER_LINES, normalize_user_prompt_prefix_application_exceeds_threshold,
    normalize_user_prompt_prefixes_applied,
};
use agent_doc_element_exchange::{
    dedupe_adjacent_prompt_prefix_duplicates_in_doc, dedupe_live_prompt_prefix_variants_in_doc,
    dedupe_prompt_lines_against_before_doc, exchange_shrink_guard_block,
    live_exchange_without_visible_write_retry_required, normalize_user_prompts_in_exchange,
    preserve_head_exchange_prompt_prefix_state,
};

pub fn check_exchange_shrink_guard_with_log(
    content_at_start: &str,
    content_ours: &str,
    file: &Path,
    min_bytes: usize,
    max_ratio: f64,
    mut logger: impl FnMut(&Path, &str),
) -> Result<()> {
    if let Some(block) =
        exchange_shrink_guard_block(content_at_start, content_ours, min_bytes, max_ratio)
    {
        logger(
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
            "exchange content would shrink from {} to {} bytes ({:.0}% of original) - \
             refusing write to prevent accidental truncation. If this is intentional, \
             use `agent-doc compact` or re-run with meaningful content.",
            block.old_exchange_len,
            block.new_exchange_len,
            block.ratio * 100.0
        );
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn file_ipc_consumed_without_live_exchange_visible_write_with_log(
    file: &Path,
    source: &str,
    patch_id: Option<&str>,
    baseline: Option<&str>,
    before: Option<&str>,
    after: &str,
    visible_write_proven: bool,
    mut logger: impl FnMut(&Path, &str),
    mut proof_failure_logger: impl FnMut(&Path, &str, Option<&str>, &str, &str, &str),
) -> bool {
    let Some(before) = before else {
        return false;
    };
    if !live_exchange_without_visible_write_retry_required(
        baseline,
        Some(before),
        after,
        visible_write_proven,
    ) {
        return false;
    }

    let before_hash = agent_doc_hash::content_hash(before);
    let after_hash = agent_doc_hash::content_hash(after);
    eprintln!(
        "[write] file IPC consumed for {} with live exchange edits but no visible-write proof and no exchange materialization - retry required before snapshot/commit",
        file.display()
    );
    logger(
        file,
        &format!(
            "file_ipc_live_exchange_without_visible_write file={} source={} patch_id={} before_hash={} after_hash={}",
            file.display(),
            source,
            patch_id.unwrap_or("-"),
            before_hash,
            after_hash
        ),
    );
    proof_failure_logger(
        file,
        source,
        patch_id,
        "live_exchange_without_visible_write",
        "retry_without_disk_write",
        &format!("before_hash={} after_hash={}", before_hash, after_hash),
    );
    true
}

pub fn dedupe_live_prompt_prefix_variants_in_tail_with_log(
    content: &str,
    file: &Path,
    mut logger: impl FnMut(&Path, &str),
) -> (String, bool) {
    let Some(repaired) = dedupe_live_prompt_prefix_variants_in_doc(content) else {
        return (content.to_string(), false);
    };
    logger(
        file,
        &format!(
            "live_prompt_prefix_variant_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

pub fn dedupe_adjacent_prompt_prefix_duplicates_with_log(
    content: &str,
    file: &Path,
    mut logger: impl FnMut(&Path, &str),
) -> (String, bool) {
    let Some(repaired) = dedupe_adjacent_prompt_prefix_duplicates_in_doc(content) else {
        return (content.to_string(), false);
    };
    logger(
        file,
        &format!(
            "live_prompt_prefix_variant_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

pub fn dedupe_prompt_lines_against_before_with_log(
    before: &str,
    after: &str,
    file: &Path,
    mut logger: impl FnMut(&Path, &str),
) -> (String, bool) {
    let Some(repaired) = dedupe_prompt_lines_against_before_doc(before, after) else {
        return (after.to_string(), false);
    };
    logger(
        file,
        &format!(
            "ipc_prompt_duplicate_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

pub fn remove_post_exchange_duplicate_prompt_comments_with_log(
    content: &str,
    file: &Path,
    source: &str,
    preserve_doc: Option<&str>,
    preserve_current_doc: Option<&str>,
    mut logger: impl FnMut(&Path, &str),
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
    logger(
        file,
        &format!(
            "post_exchange_duplicate_prompt_comment_removed file={} source={} before_commit=true",
            file.display(),
            source
        ),
    );
    (cleaned, true)
}

pub fn dedupe_consecutive_response_blocks_with_log(
    content: &str,
    file: &Path,
    mut logger: impl FnMut(&Path, &str),
) -> String {
    let deduped = agent_doc_turn::response_replay::dedupe_responses(content);
    if deduped != content {
        eprintln!(
            "[write] dedup: removed consecutive duplicate response block(s) from {} before closeout",
            file.display()
        );
        logger(
            file,
            &format!(
                "dedupe_consecutive_response_blocks file={} before_closeout=true",
                file.display()
            ),
        );
    }
    deduped
}

#[derive(Clone, Copy, Debug)]
pub struct DuplicatePromptRepairOptions<'a> {
    pub source: &'a str,
    pub before: Option<&'a str>,
    pub preserve_doc: Option<&'a str>,
    pub preserve_current_doc: Option<&'a str>,
    pub enforce_residue_guard: bool,
}

impl<'a> DuplicatePromptRepairOptions<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source,
            before: None,
            preserve_doc: None,
            preserve_current_doc: None,
            enforce_residue_guard: true,
        }
    }

    pub fn with_before(mut self, before: Option<&'a str>) -> Self {
        self.before = before;
        self
    }

    pub fn preserving(mut self, preserve_doc: Option<&'a str>) -> Self {
        self.preserve_doc = preserve_doc;
        self
    }

    pub fn preserving_current(mut self, preserve_current_doc: Option<&'a str>) -> Self {
        self.preserve_current_doc = preserve_current_doc;
        self
    }

    pub fn without_residue_guard(mut self) -> Self {
        self.enforce_residue_guard = false;
        self
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DuplicatePromptRepairReport {
    pub response_blocks: bool,
    pub answered_tail: bool,
    pub post_exchange_comments: bool,
    pub prompt_lines_against_before: bool,
    pub live_prefix_variants: bool,
}

impl DuplicatePromptRepairReport {
    pub fn changed(self) -> bool {
        self.response_blocks
            || self.answered_tail
            || self.post_exchange_comments
            || self.prompt_lines_against_before
            || self.live_prefix_variants
    }
}

pub fn repair_duplicate_prompt_artifacts_with_log(
    content: &str,
    file: &Path,
    options: DuplicatePromptRepairOptions<'_>,
    mut logger: impl FnMut(&Path, &str),
    mut duplicate_prompt_residue_logger: impl FnMut(&Path),
) -> Result<(String, DuplicatePromptRepairReport)> {
    let mut repaired = content.to_string();
    let mut report = DuplicatePromptRepairReport::default();

    let response_deduped =
        dedupe_consecutive_response_blocks_with_log(&repaired, file, &mut logger);
    if response_deduped != repaired {
        repaired = response_deduped;
        report.response_blocks = true;
    }

    if let Some(answered_tail_deduped) =
        agent_doc_template::remove_duplicate_answered_exchange_prompt_tail(&repaired)
    {
        repaired = answered_tail_deduped;
        report.answered_tail = true;
        logger(
            file,
            &format!(
                "duplicate_answered_exchange_prompt_tail_removed file={} source={} before_commit=true",
                file.display(),
                options.source
            ),
        );
    }

    let (comment_deduped, comment_changed) =
        remove_post_exchange_duplicate_prompt_comments_with_log(
            &repaired,
            file,
            options.source,
            options.preserve_doc,
            options.preserve_current_doc,
            &mut logger,
        );
    if comment_changed {
        repaired = comment_deduped;
        report.post_exchange_comments = true;
    }

    if let Some(before) = options.before {
        let (prompt_deduped, prompt_changed) =
            dedupe_prompt_lines_against_before_with_log(before, &repaired, file, &mut logger);
        if prompt_changed {
            repaired = prompt_deduped;
            report.prompt_lines_against_before = true;
        }
    }

    let (adjacent_prefix_deduped, adjacent_prefix_changed) =
        dedupe_adjacent_prompt_prefix_duplicates_with_log(&repaired, file, &mut logger);
    if adjacent_prefix_changed {
        repaired = adjacent_prefix_deduped;
        report.live_prefix_variants = true;
    }

    let (prefix_deduped, prefix_changed) =
        dedupe_live_prompt_prefix_variants_in_tail_with_log(&repaired, file, &mut logger);
    if prefix_changed {
        repaired = prefix_deduped;
        report.live_prefix_variants = true;
    }

    if options.enforce_residue_guard {
        enforce_no_duplicate_prompt_residue(
            file,
            &repaired,
            options.source,
            &mut duplicate_prompt_residue_logger,
        )?;
    }

    if report.changed() {
        logger(
            file,
            &format!(
                "duplicate_prompt_artifact_repair file={} source={} response_blocks={} answered_tail={} post_exchange_comments={} prompt_lines_against_before={} live_prefix_variants={} before_commit=true",
                file.display(),
                options.source,
                report.response_blocks,
                report.answered_tail,
                report.post_exchange_comments,
                report.prompt_lines_against_before,
                report.live_prefix_variants
            ),
        );
    }

    Ok((repaired, report))
}

pub fn repair_commit_prompt_artifacts_against_snapshot_with_log(
    file: &Path,
    snapshot: &str,
    current: &str,
    mut logger: impl FnMut(&Path, &str),
) -> Option<String> {
    let mut repaired = current.to_string();
    let mut report = DuplicatePromptRepairReport::default();

    let (prompt_deduped, prompt_changed) =
        dedupe_prompt_lines_against_before_with_log(snapshot, &repaired, file, &mut logger);
    if prompt_changed {
        repaired = prompt_deduped;
        report.prompt_lines_against_before = true;
    }

    let (adjacent_prefix_deduped, adjacent_prefix_changed) =
        dedupe_adjacent_prompt_prefix_duplicates_with_log(&repaired, file, &mut logger);
    if adjacent_prefix_changed {
        repaired = adjacent_prefix_deduped;
        report.live_prefix_variants = true;
    }

    let (prefix_deduped, prefix_changed) =
        dedupe_live_prompt_prefix_variants_in_tail_with_log(&repaired, file, &mut logger);
    if prefix_changed {
        repaired = prefix_deduped;
        report.live_prefix_variants = true;
    }

    if report.changed() {
        logger(
            file,
            &format!(
                "duplicate_prompt_artifact_repair file={} source=commit-pre-stage response_blocks=false answered_tail=false post_exchange_comments=false prompt_lines_against_before={} live_prefix_variants={} before_commit=true",
                file.display(),
                report.prompt_lines_against_before,
                report.live_prefix_variants
            ),
        );
        Some(repaired)
    } else {
        None
    }
}

pub fn normalize_user_prompts_in_exchange_safe_with_log(
    content: &str,
    baseline: &str,
    snapshot: &str,
    file: &Path,
    mut load_head: impl FnMut(&Path) -> Option<String>,
    mut logger: impl FnMut(&Path, &str),
) -> String {
    let mut normalized = normalize_user_prompts_in_exchange(content, baseline, snapshot);
    if normalized != content
        && let Some(head) = load_head(file)
    {
        let preserved = preserve_head_exchange_prompt_prefix_state(&normalized, &head);
        if preserved != normalized {
            logger(
                file,
                &format!(
                    "normalize_preserved_head_prompt_prefix_state file={}",
                    file.display()
                ),
            );
            normalized = preserved;
        }
    }

    let applied = normalize_user_prompt_prefixes_applied(content, &normalized);

    logger(
        file,
        &format!(
            "normalize_user_prompts snap_len={} base_len={} applied={}",
            snapshot.len(),
            baseline.len(),
            applied
        ),
    );

    if normalize_user_prompt_prefix_application_exceeds_threshold(applied) {
        eprintln!(
            "[normalize] WARN: {} ❯-prefixes would be applied, exceeds threshold {} for {} - \
             suspected snapshot/baseline divergence. Skipping ❯ prefix application this cycle.",
            applied,
            MAX_NORMALIZE_USER_LINES,
            file.display()
        );
        logger(
            file,
            &format!(
                "normalize_threshold_exceeded applied={} threshold={} action=passthrough",
                applied, MAX_NORMALIZE_USER_LINES
            ),
        );
        return content.to_string();
    }

    normalized
}

fn enforce_no_duplicate_prompt_residue(
    file: &Path,
    content: &str,
    context: &str,
    duplicate_prompt_residue_logger: &mut impl FnMut(&Path),
) -> Result<()> {
    match agent_doc_template::guard_no_duplicate_prompt_residue_outside_exchange(content) {
        Ok(()) => Ok(()),
        Err(err) => {
            duplicate_prompt_residue_logger(file);
            Err(err).with_context(|| {
                format!(
                    "duplicate prompt residue check failed for {} ({context})",
                    file.display()
                )
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shrink_guard_logs_and_blocks_large_exchange_truncation() {
        let file = Path::new("doc.md");
        let before = format!(
            "<!-- agent:exchange patch=append -->\n{}\n<!-- /agent:exchange -->\n",
            "x".repeat(200)
        );
        let after = "<!-- agent:exchange patch=append -->\nsmall\n<!-- /agent:exchange -->\n";
        let mut logs = Vec::new();

        let err =
            check_exchange_shrink_guard_with_log(&before, after, file, 100, 0.10, |_, message| {
                logs.push(message.to_string())
            })
            .unwrap_err();

        assert!(err.to_string().contains("exchange content would shrink"));
        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains("shrink_guard_blocked"));
    }

    #[test]
    fn prompt_line_dedupe_logs_when_before_content_is_duplicated() {
        let file = Path::new("doc.md");
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ live prompt\n",
            "live prompt\n",
            "### Re: response\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut logs = Vec::new();

        let (repaired, changed) =
            dedupe_prompt_lines_against_before_with_log(before, after, file, |_, message| {
                logs.push(message.to_string())
            });

        assert!(changed);
        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(!repaired.contains("❯ live prompt\nlive prompt"));
        assert_eq!(logs.len(), 1);
        assert!(logs[0].contains("ipc_prompt_duplicate_repaired"));
    }

    #[test]
    fn duplicate_prompt_repair_reports_prompt_dedupe() {
        let file = Path::new("doc.md");
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "live prompt\n",
            "<!-- /agent:exchange -->\n"
        );
        let after = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ live prompt\n",
            "live prompt\n",
            "### Re: response\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );
        let mut logs = Vec::new();

        let (repaired, report) = repair_duplicate_prompt_artifacts_with_log(
            after,
            file,
            DuplicatePromptRepairOptions::new("test")
                .with_before(Some(before))
                .preserving(Some(before)),
            |_, message| logs.push(message.to_string()),
            |_| panic!("residue guard should not fire"),
        )
        .unwrap();

        assert_eq!(
            report,
            DuplicatePromptRepairReport {
                response_blocks: false,
                answered_tail: false,
                post_exchange_comments: false,
                prompt_lines_against_before: true,
                live_prefix_variants: false,
            }
        );
        assert!(repaired.contains("❯ live prompt\n### Re: response"));
        assert!(logs.iter().any(|entry| {
            entry.contains("duplicate_prompt_artifact_repair file=doc.md source=test")
        }));
    }

    #[test]
    fn normalize_user_prompts_safe_logs_metrics() {
        let file = Path::new("doc.md");
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        let baseline =
            "<!-- agent:exchange patch=append -->\nOld.\nHello\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nOld.\nHello\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let mut logs = Vec::new();

        let result = normalize_user_prompts_in_exchange_safe_with_log(
            content,
            baseline,
            snapshot,
            file,
            |_| None,
            |_, message| logs.push(message.to_string()),
        );

        assert!(result.contains("❯ Hello"));
        assert!(logs.iter().any(|entry| {
            entry.contains("normalize_user_prompts")
                && entry.contains("applied=1")
                && entry.contains("snap_len=")
        }));
    }
}
