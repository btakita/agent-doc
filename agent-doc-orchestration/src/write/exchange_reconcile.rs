//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

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
    let old_exchange_len = extract_exchange_content_len(content_at_start);
    let new_exchange_len = extract_exchange_content_len(content_ours);

    if old_exchange_len < SHRINK_GUARD_MIN_BYTES {
        return Ok(());
    }

    let ratio = new_exchange_len as f64 / old_exchange_len as f64;
    if ratio < SHRINK_GUARD_MAX_RATIO {
        crate::ops_log::log_op(
            file,
            &format!(
                "shrink_guard_blocked file={} old_len={} new_len={} ratio={:.3}",
                file.display(),
                old_exchange_len,
                new_exchange_len,
                ratio
            ),
        );
        anyhow::bail!(
            "exchange content would shrink from {} to {} bytes ({:.0}% of original) — \
             refusing write to prevent accidental truncation. If this is intentional, \
             use `agent-doc compact` or re-run with meaningful content.",
            old_exchange_len,
            new_exchange_len,
            ratio * 100.0
        );
    }

    Ok(())
}

/// Extract the byte length of the exchange component's content.
/// Returns 0 if no exchange component is found.
pub(crate) fn extract_exchange_content_len(doc: &str) -> usize {
    if let Ok(components) = component::parse(doc) {
        components
            .iter()
            .find(|c| c.name == "exchange")
            .map(|c| c.content(doc).trim().len())
            .unwrap_or(0)
    } else {
        0
    }
}

pub(crate) fn exchange_content(doc: &str) -> Option<&str> {
    component::parse(doc)
        .ok()?
        .into_iter()
        .find(|component| component.name == "exchange")
        .map(|component| component.content(doc))
}

pub(crate) fn normalized_prompt_text(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!--")
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("## User")
        || is_markdown_heading_line(trimmed)
    {
        return None;
    }
    Some(
        trimmed
            .strip_prefix('❯')
            .unwrap_or(trimmed)
            .trim()
            .to_string(),
    )
}

pub(crate) fn is_markdown_heading_line(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes) && trimmed.as_bytes().get(hashes) == Some(&b' ')
}

pub(crate) fn normalized_prompt_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for line in exchange.lines() {
        if let Some(text) = normalized_prompt_text(line) {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

pub(crate) fn response_aware_user_prompt_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for info in exchange_prompt_reconciliation_infos(exchange, None) {
        if let Some(text) = info.normalized {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

pub(crate) fn user_prompt_count_growth(reference: &str, candidate: &str) -> usize {
    let (Some(reference_exchange), Some(candidate_exchange)) =
        (exchange_content(reference), exchange_content(candidate))
    else {
        return 0;
    };
    let reference_counts = response_aware_user_prompt_counts(reference_exchange);
    let candidate_counts = response_aware_user_prompt_counts(candidate_exchange);
    candidate_counts
        .iter()
        .map(|(line, candidate_count)| {
            let reference_count = reference_counts.get(line).copied().unwrap_or(0);
            candidate_count.saturating_sub(reference_count)
        })
        .sum()
}

pub(crate) fn exchange_has_live_user_edit(baseline: Option<&str>, before: &str) -> bool {
    let Some(base) = baseline else {
        return false;
    };
    let Some(base_exchange) = exchange_content(base) else {
        return false;
    };
    let Some(before_exchange) = exchange_content(before) else {
        return false;
    };
    strip_boundary_for_dedup(base_exchange) != strip_boundary_for_dedup(before_exchange)
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
    if ack_content_proven {
        return false;
    }
    let Some(before) = before else {
        return false;
    };
    if !exchange_has_live_user_edit(baseline, before) {
        return false;
    }
    let (Some(before_exchange), Some(after_exchange)) =
        (exchange_content(before), exchange_content(after))
    else {
        return false;
    };
    if strip_boundary_for_dedup(before_exchange) != strip_boundary_for_dedup(after_exchange) {
        return false;
    }

    let before_hash = crate::ops_log::content_hash(before);
    let after_hash = crate::ops_log::content_hash(after);
    eprintln!(
        "[write] file IPC consumed for {} with live exchange edits but no ack-content proof and no exchange materialization — falling back before snapshot/commit",
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
        "direct_write_fallback",
        &format!("before_hash={} after_hash={}", before_hash, after_hash),
    );
    true
}

pub(crate) fn exchange_prompt_prefix_count(exchange: &str) -> usize {
    exchange
        .lines()
        .filter(|line| line.trim_start().starts_with("❯ "))
        .count()
}

pub(crate) fn exchange_prompt_text_duplicated(before: &str, after: &str) -> bool {
    let Some(before_exchange) = exchange_content(before) else {
        return false;
    };
    let Some(after_exchange) = exchange_content(after) else {
        return false;
    };
    let before_counts = normalized_prompt_counts(before_exchange);
    let after_counts = normalized_prompt_counts(after_exchange);
    after_counts.iter().any(|(line, after_count)| {
        let before_count = before_counts.get(line).copied().unwrap_or(0);
        before_count > 0 && *after_count > before_count
    })
}

#[derive(Clone, Debug)]
pub(crate) struct PromptLineInfo {
    segment: String,
    normalized: Option<String>,
    prefixed: bool,
    remove: bool,
}

pub(crate) fn split_line_segment(segment: &str) -> (&str, &str) {
    segment
        .strip_suffix('\n')
        .map(|line| (line, "\n"))
        .unwrap_or((segment, ""))
}

pub(crate) fn exchange_prompt_reconciliation_infos(
    exchange: &str,
    target_counts: Option<&HashMap<String, usize>>,
) -> Vec<PromptLineInfo> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;
    let mut in_code_fence = false;
    let mut infos = Vec::new();

    for segment in exchange.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        let trimmed = line.trim();
        let is_fence = is_exchange_code_fence_delimiter(trimmed);
        let was_in_code_fence = in_code_fence;
        let mut eligible = !(was_in_code_fence || is_fence);
        if eligible {
            if trimmed.starts_with(boundary_prefix) {
                in_response_block = false;
                response_heading_was_prefixed = false;
                eligible = false;
            } else if is_exchange_response_heading_for_prefix_repair(trimmed) {
                in_response_block = true;
                response_heading_was_prefixed =
                    is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
                eligible = false;
            } else if in_response_block {
                let is_target = target_counts
                    .is_some_and(|counts| normalization_target_matches_line(line, counts));
                if starts_targeted_or_prefixed_prompt_repair_after_response(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                ) {
                    in_response_block = false;
                    response_heading_was_prefixed = false;
                } else {
                    eligible = false;
                }
            }
        }

        let normalized = if eligible {
            normalized_prompt_text(line)
        } else {
            None
        };
        infos.push(PromptLineInfo {
            segment: segment.to_string(),
            normalized,
            prefixed: trimmed.starts_with("❯ "),
            remove: false,
        });
        if is_fence {
            in_code_fence = !in_code_fence;
        }
    }

    infos
}

pub(crate) fn prompt_reconciliation_counts(exchange: &str) -> HashMap<String, usize> {
    let mut counts = HashMap::new();
    for info in exchange_prompt_reconciliation_infos(exchange, None) {
        if let Some(text) = info.normalized {
            *counts.entry(text).or_default() += 1;
        }
    }
    counts
}

pub(crate) fn last_exchange_boundary_tail_start(exchange: &str) -> Option<usize> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut offset = 0usize;
    let mut tail_start = None;
    for segment in exchange.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        if line.trim().starts_with(boundary_prefix) {
            tail_start = Some(offset + segment.len());
        }
        offset += segment.len();
    }
    tail_start
}

pub(crate) fn probable_live_prompt_prefix_variant(shorter: &str, longer: &str) -> bool {
    let shorter = shorter.trim();
    let longer = longer.trim();
    if shorter.len() < 16 || longer.len() <= shorter.len() + 2 {
        return false;
    }
    if !longer.starts_with(shorter) || !longer.is_char_boundary(shorter.len()) {
        return false;
    }
    if matches!(
        shorter.chars().last(),
        Some('.' | '!' | '?' | ':' | ';' | ')' | ']')
    ) {
        return false;
    }
    true
}

pub(crate) fn is_exchange_code_fence_delimiter(trimmed: &str) -> bool {
    let Some(first) = trimmed.chars().next() else {
        return false;
    };
    if first != '`' && first != '~' {
        return false;
    }
    trimmed.chars().take_while(|ch| *ch == first).count() >= 3
}

pub(crate) fn dedupe_live_prompt_prefix_variants_in_tail(content: &str, file: &Path) -> (String, bool) {
    let Ok(components) = component::parse(content) else {
        return (content.to_string(), false);
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return (content.to_string(), false);
    };
    let exchange_content = exchange.content(content);
    let Some(tail_start) = last_exchange_boundary_tail_start(exchange_content) else {
        return (content.to_string(), false);
    };
    let tail = &exchange_content[tail_start..];
    if tail.trim().is_empty() {
        return (content.to_string(), false);
    }

    #[derive(Clone, Debug)]
    struct TailLine {
        segment: String,
        normalized: Option<String>,
        remove: bool,
    }

    let mut in_fence = false;
    let mut lines = Vec::<TailLine>::new();
    for segment in tail.split_inclusive('\n') {
        let (line, _) = split_line_segment(segment);
        let trimmed = line.trim();
        let is_fence = is_exchange_code_fence_delimiter(trimmed);
        let normalized = if !in_fence && !is_fence {
            normalized_prompt_text(line)
        } else {
            None
        };
        lines.push(TailLine {
            segment: segment.to_string(),
            normalized,
            remove: false,
        });
        if is_fence {
            in_fence = !in_fence;
        }
    }
    if !tail.ends_with('\n') && !tail.is_empty() {
        let consumed: usize = lines.iter().map(|line| line.segment.len()).sum();
        if consumed < tail.len() {
            let rest = &tail[consumed..];
            lines.push(TailLine {
                segment: rest.to_string(),
                normalized: normalized_prompt_text(rest),
                remove: false,
            });
        }
    }

    let mut changed = false;
    for idx in 0..lines.len().saturating_sub(1) {
        if lines[idx].remove || lines[idx + 1].remove {
            continue;
        }
        let Some(left) = lines[idx].normalized.as_deref() else {
            continue;
        };
        let Some(right) = lines[idx + 1].normalized.as_deref() else {
            continue;
        };
        let left_prefixed = lines[idx].segment.trim_start().starts_with("❯ ");
        let right_prefixed = lines[idx + 1].segment.trim_start().starts_with("❯ ");
        if left == right && left_prefixed != right_prefixed {
            if left_prefixed {
                lines[idx + 1].remove = true;
            } else {
                lines[idx].remove = true;
            }
            changed = true;
        } else if probable_live_prompt_prefix_variant(left, right) {
            lines[idx].remove = true;
            changed = true;
        } else if probable_live_prompt_prefix_variant(right, left) {
            lines[idx + 1].remove = true;
            changed = true;
        }
    }

    if !changed {
        return (content.to_string(), false);
    }

    let repaired_tail = lines
        .into_iter()
        .filter(|line| !line.remove)
        .map(|line| line.segment)
        .collect::<String>();
    let repaired_exchange = format!("{}{}", &exchange_content[..tail_start], repaired_tail);
    let repaired = exchange.replace_content(content, &repaired_exchange);
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_prefix_variant_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

pub(crate) fn dedupe_adjacent_prompt_prefix_duplicates(content: &str, file: &Path) -> (String, bool) {
    let Ok(components) = component::parse(content) else {
        return (content.to_string(), false);
    };
    let Some(exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return (content.to_string(), false);
    };
    let exchange_content = exchange.content(content);
    let mut lines = exchange_prompt_reconciliation_infos(exchange_content, None);
    let mut changed = false;

    for idx in 0..lines.len().saturating_sub(1) {
        if lines[idx].remove || lines[idx + 1].remove {
            continue;
        }
        let Some(left) = lines[idx].normalized.as_deref() else {
            continue;
        };
        let Some(right) = lines[idx + 1].normalized.as_deref() else {
            continue;
        };
        if left == right && lines[idx].prefixed != lines[idx + 1].prefixed {
            if lines[idx].prefixed {
                lines[idx + 1].remove = true;
            } else {
                lines[idx].remove = true;
            }
            changed = true;
        }
    }

    if !changed {
        return (content.to_string(), false);
    }

    let repaired_exchange = lines
        .into_iter()
        .filter(|line| !line.remove)
        .map(|line| line.segment)
        .collect::<String>();
    let repaired = exchange.replace_content(content, &repaired_exchange);
    crate::ops_log::log_op(
        file,
        &format!(
            "live_prompt_prefix_variant_repaired file={} before_commit=true",
            file.display()
        ),
    );
    (repaired, true)
}

pub(crate) fn dedupe_prompt_lines_against_before(before: &str, after: &str, file: &Path) -> (String, bool) {
    let Some(before_exchange) = exchange_content(before) else {
        return (after.to_string(), false);
    };
    let Ok(components) = component::parse(after) else {
        return (after.to_string(), false);
    };
    let Some(after_exchange) = components
        .iter()
        .find(|component| component.name == "exchange")
    else {
        return (after.to_string(), false);
    };

    let before_counts = prompt_reconciliation_counts(before_exchange);
    if before_counts.is_empty() {
        return (after.to_string(), false);
    }
    let mut lines: Vec<PromptLineInfo> =
        exchange_prompt_reconciliation_infos(after_exchange.content(after), Some(&before_counts));

    let mut by_text: HashMap<String, Vec<usize>> = HashMap::new();
    for (idx, line) in lines.iter().enumerate() {
        if let Some(text) = line.normalized.as_ref() {
            by_text.entry(text.clone()).or_default().push(idx);
        }
    }

    let mut changed = false;
    for (text, indexes) in by_text {
        let allowed = before_counts.get(&text).copied().unwrap_or(0);
        if allowed == 0 || indexes.len() <= allowed {
            continue;
        }

        let mut excess = indexes.len() - allowed;
        if indexes.iter().any(|idx| lines[*idx].prefixed) {
            let unprefixed_indexes: Vec<usize> = indexes
                .iter()
                .copied()
                .filter(|idx| !lines[*idx].prefixed)
                .collect();
            for idx in unprefixed_indexes {
                if excess == 0 {
                    break;
                }
                lines[idx].remove = true;
                excess -= 1;
                changed = true;
            }
        }
        if excess > 0 {
            for idx in indexes.iter().rev().copied() {
                if excess == 0 {
                    break;
                }
                if lines[idx].remove {
                    continue;
                }
                lines[idx].remove = true;
                excess -= 1;
                changed = true;
            }
        }
    }

    if !changed {
        return (after.to_string(), false);
    }

    let repaired_exchange = lines
        .into_iter()
        .filter(|line| !line.remove)
        .map(|line| line.segment)
        .collect::<String>();
    let repaired = after_exchange.replace_content(after, &repaired_exchange);
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
        crate::template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
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
