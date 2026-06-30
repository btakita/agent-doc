//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;
use agent_doc_element_exchange::{
    exchange_prompt_prefix_eligible_lines, is_exchange_response_heading_for_prefix_repair,
    normalization_target_counts, starts_prompt_run_after_response,
    starts_targeted_prompt_repair_after_response,
};

/// Extract lines that were normalized by `normalize_user_prompts_in_exchange`.
///
/// Compares `before` and `after` exchange content line-by-line and returns
/// lines where `before` had plain text and `after` has `❯ <text>` at the
/// same position — i.e., lines the normalization step added `❯ ` to this cycle.
///
/// Line-by-line comparison avoids false negatives when the exchange already
/// contains `❯ <text>` lines at OTHER positions (which would cause a
/// HashSet-based check to incorrectly skip newly normalized lines).
///
/// These are passed to the IPC plugin so it can apply the same normalization
/// to the live editor document.
pub fn extract_normalization_targets(before: &str, after: &str) -> Vec<String> {
    let before_comps = element::parse(before).unwrap_or_default();
    let after_comps = element::parse(after).unwrap_or_default();

    let before_exc = before_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(before))
        .unwrap_or("");
    let after_exc = after_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(after))
        .unwrap_or("");

    if before_exc == after_exc {
        return vec![];
    }

    // Line-by-line: find positions where before had `text` and after has `❯ text`.
    // Using position comparison prevents false negatives when the exchange already
    // contains `❯ text` lines elsewhere (HashSet membership would exclude them).
    let mut targets = Vec::new();

    for (before_line, after_line) in before_exc.lines().zip(after_exc.lines()) {
        if let Some(stripped) = after_line.strip_prefix("❯ ") {
            // after has ❯ prefix; before must have the plain version at the same position
            if before_line == stripped {
                targets.push(stripped.to_string());
            }
        }
    }

    targets
}

/// Add `❯ ` prefix to user-added lines in exchange components.
///
/// Compares the exchange content in `baseline` against `snapshot` to identify
/// lines the user typed this cycle (Insert lines in the diff). Those lines are
/// then prefixed with `❯ ` in `content` (content_ours = baseline + agent patches).
/// Prompt-bearing lines derived from the canonical diff classifier are also
/// treated as mandatory normalization targets so repair/write/session-check
/// share one prompt-prefix contract.
///
/// Using `baseline` (not `content_ours`) for the diff is critical: after
/// `apply_patches_with_overrides`, the boundary marker is repositioned to the end
/// of the exchange. Everything before it — including the agent's new response —
/// is the "user region". Diffing `snapshot → content_ours user_region` would
/// incorrectly mark agent response lines as Insert and prefix them. Diffing
/// `snapshot → baseline` identifies only genuine user additions.
///
/// Skips lines that are blank, already start with `❯`, start with `<!--`
/// (structural component/patch/boundary markers), or sit inside a fenced code
/// block. Every other added line in the exchange user region gets the prefix —
/// the component defines the context, so content shape is not second-guessed.
/// Non-destructive if no exchange component is present or no new lines are
/// found.
///
/// Both disk and IPC write paths call this after computing `content_ours` so the
/// snapshot and merged document consistently show `❯ ` on user input.
pub fn normalize_user_prompts_in_exchange(content: &str, baseline: &str, snapshot: &str) -> String {
    let Ok(content_comps) = element::parse(content) else {
        return content.to_string();
    };
    let baseline_comps = element::parse(baseline).unwrap_or_default();
    let snap_comps = element::parse(snapshot).unwrap_or_default();

    let Some(exchange) = content_comps.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };

    let baseline_exc = baseline_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|e| e.content(baseline))
        .unwrap_or("");
    let snap_exc = snap_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|e| e.content(snapshot))
        .unwrap_or("");

    let exc_content = exchange.content(content);

    // Find the LAST boundary marker in content_ours — user region is before, agent region after.
    // Must use the last boundary (most recent cycle) — historical cycles each insert their own
    // boundary marker, so stopping at the first one would misclassify later user-input lines
    // (between historical boundaries) as "agent region" and skip ❯  prefix restoration.
    let boundary_prefix = "<!-- agent:boundary:";
    let boundary_pos = {
        let mut pos = exc_content.len();
        let mut offset = 0;
        for line in exc_content.lines() {
            if line.trim().starts_with(boundary_prefix) {
                pos = offset; // keep updating — use the last boundary found
            }
            offset += line.len() + 1;
        }
        pos
    };
    let content_user_region = &exc_content[..boundary_pos];
    let content_agent_region = &exc_content[boundary_pos..];

    // Strip boundary markers from baseline and snapshot for diffing.
    // Preserves trailing newline if present in the original.
    let strip = |s: &str| -> String {
        let filtered: Vec<&str> = s
            .lines()
            .filter(|l| !l.trim().starts_with(boundary_prefix))
            .collect();
        let mut out = filtered.join("\n");
        if s.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out
    };
    let baseline_stripped = strip(baseline_exc);
    let snap_stripped = strip(snap_exc);

    // Diff snapshot → baseline to find user-added lines (not agent lines).
    // Track code-fence state so lines inside fences are excluded — they are code,
    // not user prompts, and must not receive the ❯  prefix.
    // Handles both ``` and ~~~ fences (matching CommonMark spec).
    use similar::{ChangeTag, TextDiff};

    // Option 2 invariant: inside `agent:exchange`, every added line gets the ❯ prefix.
    // The component defines the context, so content shape does not gate the decision.
    // Only structural markers (HTML comments for component/patch/boundary tags) and
    // code fences are excluded — everything else is user input.

    /// Returns Some((fence_char, fence_len)) if `trimmed` opens a new fence, else None.
    fn fence_open(trimmed: &str) -> Option<(char, usize)> {
        let fc = trimmed.chars().next()?;
        if fc != '`' && fc != '~' {
            return None;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        if fl >= 3 { Some((fc, fl)) } else { None }
    }

    /// Returns true if `trimmed` closes a fence opened with `(fence_char, fence_len)`.
    fn fence_close(trimmed: &str, fence_char: char, fence_len: usize) -> bool {
        let fc = trimmed.chars().next().unwrap_or('\0');
        if fc != fence_char {
            return false;
        }
        let fl = trimmed.chars().take_while(|&c| c == fc).count();
        fl >= fence_len && trimmed[fl..].trim().is_empty()
    }

    fn heading_level(trimmed: &str) -> Option<usize> {
        let n = trimmed.bytes().take_while(|&b| b == b'#').count();
        if (1..=6).contains(&n) && trimmed.as_bytes().get(n) == Some(&b' ') {
            Some(n)
        } else {
            None
        }
    }

    let diff_text = agent_doc_diff::unified_diff_from_contents(&snap_stripped, &baseline_stripped);
    let prompt_prefix_targets = diff_text
        .as_deref()
        .map(agent_doc_diff::prompt_prefix_normalization_targets)
        .unwrap_or_default();

    let diff = TextDiff::from_lines(snap_stripped.as_str(), baseline_stripped.as_str());
    let mut user_added = std::collections::HashSet::<String>::new();
    let mut agent_inserted = std::collections::HashSet::<String>::new();
    let mut in_baseline_fence = false;
    let mut baseline_fence_char = '`';
    let mut baseline_fence_len = 3usize;
    let mut in_agent_block = false;
    let mut saw_deleted_heading = false;
    // `#repair-orphan-prefix-bug`: track whether the scanner is inside an
    // assistant `### Re:` block and whether that block had a body line deleted.
    // A body REPLACEMENT (delete + insert) under an Equal heading is assistant
    // content; a pure append after an unchanged response stays a user prompt.
    let mut in_re_block = false;
    let mut re_block_saw_body_delete = false;
    for change in diff.iter_all_changes() {
        let line = change.value().trim_end_matches('\n');
        let trimmed = line.trim();
        let is_heading = heading_level(trimmed).is_some();
        // Equal and Insert lines are present in baseline — track their fence state.
        // Capture pre-update state to correctly detect closing delimiters as fence markers.
        let was_in_fence = in_baseline_fence;
        if change.tag() == ChangeTag::Delete {
            saw_deleted_heading = !in_baseline_fence && is_heading;
            if in_re_block
                && !in_baseline_fence
                && !is_heading
                && !trimmed.is_empty()
                && !trimmed.starts_with("<!--")
                && fence_open(trimmed).is_none()
            {
                re_block_saw_body_delete = true;
            }
            continue;
        }
        let heading_replaces_deleted_heading =
            change.tag() == ChangeTag::Insert && is_heading && saw_deleted_heading;
        saw_deleted_heading = false;
        if change.tag() != ChangeTag::Delete {
            if !in_baseline_fence {
                if let Some((fc, fl)) = fence_open(trimmed) {
                    in_baseline_fence = true;
                    baseline_fence_char = fc;
                    baseline_fence_len = fl;
                }
            } else if fence_close(trimmed, baseline_fence_char, baseline_fence_len) {
                in_baseline_fence = false;
            }
            if !in_baseline_fence {
                if heading_level(trimmed).is_some() {
                    in_agent_block =
                        change.tag() == ChangeTag::Insert && !heading_replaces_deleted_heading;
                    // Track whether we are inside an assistant `### Re:` block so
                    // that a body REPLACEMENT under an already-present (Equal)
                    // heading is recognized as assistant content rather than a
                    // user prompt (#repair-orphan-prefix-bug).
                    in_re_block = trimmed.starts_with("### Re:");
                    re_block_saw_body_delete = false;
                } else if in_agent_block && trimmed.is_empty() {
                    // Blank assistant-response lines do not prove the following
                    // prose is user input. Only explicit prompt-run starts below
                    // can return the scanner to user-owned transcript lines.
                } else if in_agent_block
                    && (starts_targeted_prompt_repair_after_response(trimmed, true)
                        || trimmed.starts_with('❯')
                        || trimmed.starts_with("<!--"))
                {
                    in_agent_block = false;
                }
                // A prompt-run start (or explicit `❯`) after the response body
                // returns the scanner to user-owned transcript lines, ending the
                // `### Re:` replacement window.
                if in_re_block
                    && (starts_targeted_prompt_repair_after_response(trimmed, true)
                        || trimmed.starts_with('❯'))
                {
                    in_re_block = false;
                    re_block_saw_body_delete = false;
                }
            }
        }
        // A line is a fence delimiter if it opens a fence (fence_open), or closes the current
        // one (was_in_fence before update, and matches close pattern).
        let is_fence_delim = fence_open(trimmed).is_some()
            || (was_in_fence && fence_close(trimmed, baseline_fence_char, baseline_fence_len));
        // Insert body lines that replace deleted body under an existing `### Re:`
        // heading are assistant content (#repair-orphan-prefix-bug), not prompts.
        let is_re_block_replacement = in_re_block && re_block_saw_body_delete;
        if change.tag() == ChangeTag::Insert
            && !in_baseline_fence
            && !in_agent_block
            && !is_re_block_replacement
            && !heading_replaces_deleted_heading
            && !trimmed.is_empty()
            && !trimmed.starts_with('❯')
            && !trimmed.starts_with("<!--")
            && !is_fence_delim
            // `#provauth3`: a compaction Session Summary line is binary-authored,
            // not user input — never stamp it with the `❯` prompt prefix even
            // though it appears as an inserted line relative to the pre-compact
            // snapshot. Origin is known, so the content-diff guess is overridden.
            && !agent_doc_diff::line_is_binary_authored_compact_summary(trimmed)
        {
            user_added.insert(line.to_string());
        } else if change.tag() == ChangeTag::Insert && (in_agent_block || is_re_block_replacement) {
            agent_inserted.insert(line.to_string());
        }
    }

    for line in prompt_prefix_targets {
        if !agent_inserted.contains(&line) {
            user_added.insert(line);
        }
    }

    if user_added.is_empty() {
        return content.to_string();
    }

    // Apply ❯  prefix to user-added lines in content_user_region.
    // Agent response lines (not in user_added) pass through unchanged.
    // Track code-fence state (``` and ~~~) so prefix is never added inside fences.
    let mut in_content_fence = false;
    let mut content_fence_char = '`';
    let mut content_fence_len = 3usize;
    let mut normalized_user = String::new();
    for line in content_user_region.lines() {
        let trimmed = line.trim();
        if !in_content_fence {
            if let Some((fc, fl)) = fence_open(trimmed) {
                in_content_fence = true;
                content_fence_char = fc;
                content_fence_len = fl;
            }
        } else if fence_close(trimmed, content_fence_char, content_fence_len) {
            in_content_fence = false;
        }
        if !in_content_fence && user_added.contains(line) {
            normalized_user.push_str("❯ ");
        }
        normalized_user.push_str(line);
        normalized_user.push('\n');
    }
    if !content_user_region.is_empty() && !content_user_region.ends_with('\n') {
        normalized_user.truncate(normalized_user.len() - 1);
    }
    if content_user_region.is_empty() {
        normalized_user.clear();
    }

    let new_exc_content = format!("{}{}", normalized_user, content_agent_region);
    exchange.replace_content(content, &new_exc_content)
}

pub(crate) fn preserve_head_exchange_prompt_prefix_state(content: &str, head: &str) -> String {
    let Ok(head_components) = element::parse(head) else {
        return content.to_string();
    };
    let Some(head_exchange) = head_components.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };
    let mut head_unprefixed = HashMap::<String, usize>::new();
    let mut head_prefixed = HashMap::<String, usize>::new();
    for line in head_exchange.content(head).lines() {
        let line = line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('❯')
            || trimmed.starts_with("<!--")
            || is_exchange_response_heading_for_prefix_repair(trimmed)
        {
            continue;
        }
        *head_unprefixed.entry(line.to_string()).or_default() += 1;
    }
    for line in exchange_prompt_prefix_eligible_lines(head_exchange.content(head), None) {
        let line = line.trim_end();
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with("<!--") {
            continue;
        }
        if let Some(stripped) = line.strip_prefix("❯ ") {
            *head_prefixed.entry(stripped.to_string()).or_default() += 1;
        }
    }
    if head_unprefixed.is_empty() && head_prefixed.is_empty() {
        return content.to_string();
    }

    let Ok(content_components) = element::parse(content) else {
        return content.to_string();
    };
    let Some(exchange) = content_components.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };
    let exchange_content = exchange.content(content);
    let mut changed = false;
    let mut rebuilt = String::with_capacity(exchange_content.len());
    let target_counts =
        normalization_target_counts(&head_prefixed.keys().cloned().collect::<Vec<String>>());
    let mut in_response_block = false;
    for segment in exchange_content.split_inclusive('\n') {
        let (line, newline) = segment
            .strip_suffix('\n')
            .map(|line| (line, "\n"))
            .unwrap_or((segment, ""));
        let trimmed = line.trim();
        if trimmed.starts_with("<!-- agent:boundary:") {
            in_response_block = false;
        } else if is_exchange_response_heading_for_prefix_repair(trimmed) {
            in_response_block = true;
        }
        let is_target = target_counts
            .get(line.trim_end())
            .copied()
            .unwrap_or_default()
            > 0;
        let eligible = if in_response_block {
            starts_prompt_run_after_response(trimmed, is_target)
        } else {
            true
        };
        if let Some(unprefixed) = line.strip_prefix("❯ ")
            && let Some(remaining) = head_unprefixed.get_mut(unprefixed)
            && *remaining > 0
        {
            rebuilt.push_str(unprefixed);
            *remaining -= 1;
            changed = true;
        } else if eligible
            && !line.starts_with("❯ ")
            && let Some(remaining) = head_prefixed.get_mut(line)
            && *remaining > 0
        {
            rebuilt.push_str("❯ ");
            rebuilt.push_str(line);
            *remaining -= 1;
            changed = true;
        } else {
            rebuilt.push_str(line);
        }
        if in_response_block && eligible && starts_prompt_run_after_response(trimmed, is_target) {
            in_response_block = false;
        }
        rebuilt.push_str(newline);
    }
    if !changed {
        return content.to_string();
    }
    exchange.replace_content(content, &rebuilt)
}

pub fn enforce_imperative_response_contract(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
    response: &str,
) -> Result<()> {
    let baseline_owned = baseline
        .map(ToOwned::to_owned)
        .or_else(|| snapshot::load(file).ok().flatten());
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
    if !agent_doc_diff::diff_contains_imperative_directive(diff_text) {
        return Ok(());
    }
    if agent_doc_turn::response_text::response_satisfies_imperative_contract(response) {
        return Ok(());
    }
    let trigger = agent_doc_diff::extract_imperative_directives(diff_text)
        .into_iter()
        .next()
        .unwrap_or_else(|| "approval".to_string());
    crate::ops_log::log_op(
        file,
        &format!(
            "imperative_response_rejected file={} trigger={}",
            file.display(),
            truncate_signal(&trigger, 80)
        ),
    );
    anyhow::bail!(
        "imperative document directive requires concrete execution evidence or a concrete blocker; rejected status-only/meta response for `{}`",
        truncate_signal(&trigger, 80)
    );
}

pub(crate) fn template_mode_overrides_for_current_doc(
    file: &Path,
    baseline: Option<&str>,
    current_content: &str,
) -> std::collections::HashMap<String, String> {
    let mut overrides = std::collections::HashMap::new();
    let baseline_owned = baseline
        .map(ToOwned::to_owned)
        .or_else(|| snapshot::load(file).ok().flatten());
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

pub(crate) fn truncate_signal(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        let mut cut = max;
        while cut > 0 && !value.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}...", &value[..cut])
    }
}

/// Maximum number of `❯ `-prefix lines a single normalization cycle may add.
///
/// A legitimate user input rarely produces more than a few dozen prefixed lines
/// in one write cycle. When this threshold is exceeded, it indicates snapshot/
/// baseline divergence (stale baseline, boundary misalignment, or snapshot
/// reset) rather than genuine user input — applying the prefix would corrupt
/// the file at scale. See `normalize_user_prompts_in_exchange_safe`.
pub const MAX_NORMALIZE_USER_LINES: usize = 50;

/// Safe wrapper around [`normalize_user_prompts_in_exchange`] that adds:
///
/// 1. **Forensic logging** — every call writes `normalize_user_prompts`
///    metrics (`snap_len`, `base_len`, `applied`) to `ops.log` so divergence
///    incidents can be caught in the wild.
/// 2. **Safety rail** — if more than [`MAX_NORMALIZE_USER_LINES`] prefixes
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
    let mut normalized = normalize_user_prompts_in_exchange(content, baseline, snapshot);
    if normalized != content
        && let Ok(Some(head)) = crate::git::show_head(file)
    {
        let preserved = preserve_head_exchange_prompt_prefix_state(&normalized, &head);
        if preserved != normalized {
            crate::ops_log::log_op(
                file,
                &format!(
                    "normalize_preserved_head_prompt_prefix_state file={}",
                    file.display()
                ),
            );
            normalized = preserved;
        }
    }

    // Count `❯ ` prefixes before/after to measure how many lines this call applied.
    // Note: also count a prefix at offset 0 (no leading newline).
    fn count_prefixes(s: &str) -> usize {
        let mut n = s.matches("\n❯ ").count();
        if s.starts_with("❯ ") {
            n += 1;
        }
        n
    }
    let before = count_prefixes(content);
    let after = count_prefixes(&normalized);
    let applied = after.saturating_sub(before);

    crate::ops_log::log_op(
        file,
        &format!(
            "normalize_user_prompts snap_len={} base_len={} applied={}",
            snapshot.len(),
            baseline.len(),
            applied
        ),
    );

    if applied > MAX_NORMALIZE_USER_LINES {
        eprintln!(
            "[normalize] WARN: {} ❯-prefixes would be applied, exceeds threshold {} for {} — \
             suspected snapshot/baseline divergence. Skipping ❯ prefix application this cycle.",
            applied,
            MAX_NORMALIZE_USER_LINES,
            file.display()
        );
        crate::ops_log::log_op(
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

/// Verify that the sidecar content preserved the expected `❯ ` prefixes.
///
/// For each non-blank target in `normalize_prefix_lines`, checks whether the
/// exchange user region contains the required number of `❯ <target>`
/// occurrences. Duplicate targets must be preserved by occurrence, not just by
/// set membership, because prompt presets often repeat verbatim across turns.
/// Returns `true` when all expected prefixes are present (or when there are no
/// targets to check).
pub fn verify_sidecar_normalization(sidecar: &str, normalize_prefix_lines: &[String]) -> bool {
    if normalize_prefix_lines.is_empty() {
        return true;
    }

    let sidecar_exchange = element::parse(sidecar)
        .ok()
        .and_then(|components| {
            components
                .iter()
                .find(|component| component.name == "exchange")
                .map(|component| component.content(sidecar).to_string())
        })
        .unwrap_or_else(|| sidecar.to_string());
    let target_counts = normalization_target_counts(normalize_prefix_lines);

    let mut prefixed_counts = std::collections::HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(&sidecar_exchange, Some(&target_counts)) {
        let trimmed = line.trim_end();
        if let Some(stripped) = trimmed.strip_prefix("❯ ") {
            *prefixed_counts.entry(stripped.to_string()).or_default() += 1;
        }
    }

    for (target, required) in target_counts {
        if prefixed_counts.get(&target).copied().unwrap_or(0) < required {
            return false;
        }
    }
    true
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
mod verify_sidecar_normalization_tests {
    use super::verify_sidecar_normalization;
    use agent_doc_template::patchback::enforce_orchestrate_patchback_contract;

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
    use fs2::FileExt;
    use std::fs;
    use std::fs::OpenOptions;
    use std::time::Duration;
    use tempfile::TempDir;

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
