//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

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
    let before_comps = component::parse(before).unwrap_or_default();
    let after_comps = component::parse(after).unwrap_or_default();

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
    let Ok(content_comps) = component::parse(content) else {
        return content.to_string();
    };
    let baseline_comps = component::parse(baseline).unwrap_or_default();
    let snap_comps = component::parse(snapshot).unwrap_or_default();

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

    let diff_text = crate::diff::unified_diff_from_contents(&snap_stripped, &baseline_stripped);
    let prompt_prefix_targets = diff_text
        .as_deref()
        .map(crate::diff::prompt_prefix_normalization_targets)
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
    let Ok(head_components) = component::parse(head) else {
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

    let Ok(content_components) = component::parse(content) else {
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

/// Phrases that signal deferred/future work in an agent response.
/// When detected without a corresponding `--pending-add`, a warning is emitted.
pub(crate) const FUTURE_WORK_SIGNALS: &[&str] = &[
    "worth revisiting",
    "revisit later",
    "follow-up needed",
    "future work",
];

/// Core detection logic — no env var dependency.
pub fn check_future_work_signals(response: &str, has_pending_add: bool) -> Option<&'static str> {
    if has_pending_add {
        return None;
    }
    let lower = response.to_lowercase();
    for &signal in FUTURE_WORK_SIGNALS {
        if lower.contains(signal) {
            eprintln!(
                "[write] WARN: response contains future-work signal {:?} but no --pending-add was provided",
                signal
            );
            return Some(signal);
        }
    }
    None
}

pub(crate) const IMPERATIVE_STATUS_ONLY_SIGNALS: &[&str] = &[
    "in progress",
    "continuing",
    "starting",
    "working on it",
    "still working",
    "next i'll",
    "next i will",
    "i'll update",
    "i will update",
    "i'm going to",
    "i am going to",
    "let me do that",
];

pub(crate) const IMPERATIVE_META_REFUSAL_SIGNALS: &[&str] = &[
    "because you asked me to run agent-doc",
    "treated that text as document content",
    "not to execute",
    "say do #",
    "repeat the instruction in chat",
    "i stayed on the first layer",
    "operate on the session document",
];

pub(crate) const IMPERATIVE_BLOCKER_SIGNALS: &[&str] = &[
    "blocked",
    "blocker",
    "failed",
    "error",
    "cannot",
    "can't",
    "unable",
    "missing",
    "permission denied",
    "requires approval",
    "needs approval",
    "lock file",
    "timed out",
];

pub(crate) const IMPERATIVE_EVIDENCE_LABELS: &[&str] = &[
    "what changed:",
    "verification:",
    "commit / push:",
    "outcome:",
    "root cause:",
    "blocked:",
    "blocker:",
];

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
    let Some(diff_text) = crate::diff::unified_diff_from_contents(base, current_content) else {
        return Ok(());
    };
    enforce_imperative_response_contract_for_diff(file, &diff_text, response)
}

pub fn enforce_imperative_response_contract_for_diff(
    file: &Path,
    diff_text: &str,
    response: &str,
) -> Result<()> {
    if !crate::diff::diff_contains_imperative_directive(diff_text) {
        return Ok(());
    }
    if response_satisfies_imperative_contract(response) {
        return Ok(());
    }
    let trigger = crate::diff::extract_imperative_directives(diff_text)
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
    let Some(diff_text) = crate::diff::unified_diff_from_contents(base, current_content) else {
        return overrides;
    };
    if crate::diff::detect_exchange_compaction_request(&diff_text) {
        overrides.insert("exchange".to_string(), "replace".to_string());
    }
    overrides
}

pub(crate) fn response_satisfies_imperative_contract(response: &str) -> bool {
    let lower = response.to_ascii_lowercase();
    if contains_any_signal(&lower, IMPERATIVE_BLOCKER_SIGNALS) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_META_REFUSAL_SIGNALS) {
        return false;
    }
    if contains_execution_evidence(response, &lower) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_STATUS_ONLY_SIGNALS) {
        return false;
    }
    false
}

pub(crate) fn contains_any_signal(haystack: &str, signals: &[&str]) -> bool {
    signals.iter().any(|signal| haystack.contains(signal))
}

pub(crate) fn contains_execution_evidence(response: &str, lower: &str) -> bool {
    if response.contains("```") || response.contains("~~~") {
        return true;
    }
    if IMPERATIVE_EVIDENCE_LABELS
        .iter()
        .any(|label| lower.contains(label))
    {
        return true;
    }
    if lower.contains("implemented and verified")
        || lower.contains("built and installed")
        || lower.contains("added regression coverage")
        || lower.contains("pushed to ")
    {
        return true;
    }
    response.lines().any(|line| {
        has_commandish_backticks(line)
            || has_code_path(line)
            || contains_commit_hash(line)
            || line.trim_start().starts_with("- `")
    })
}

pub(crate) fn has_commandish_backticks(line: &str) -> bool {
    if !line.contains('`') {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("cargo ")
        || lower.contains("git ")
        || lower.contains("make ")
        || lower.contains("npm ")
        || lower.contains("pnpm ")
        || lower.contains("yarn ")
        || lower.contains("pytest")
        || lower.contains("uv run")
        || lower.contains("agent-doc ")
        || line.contains('/')
}

pub(crate) fn has_code_path(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains("src/")
        || line.contains("tests/")
        || line.contains("specs/")
        || line.contains("runbooks/")
        || lower.contains(".rs")
        || lower.contains(".md")
        || lower.contains(".toml")
        || lower.contains(".json")
        || lower.contains(".sh")
        || lower.contains(".kt")
        || lower.contains(".ts")
}

pub(crate) fn contains_commit_hash(line: &str) -> bool {
    let mut run = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_hexdigit() {
            run += 1;
            if run >= 7 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
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

    let sidecar_exchange = component::parse(sidecar)
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

pub(crate) fn exchange_user_region(content: &str) -> &str {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut boundary_pos = content.len();
    let mut offset = 0;
    for line in content.lines() {
        if line.trim().starts_with(boundary_prefix) {
            boundary_pos = offset;
        }
        offset += line.len() + 1;
    }
    &content[..boundary_pos]
}

pub(crate) fn is_exchange_response_heading_for_prefix_repair(trimmed: &str) -> bool {
    let trimmed = trimmed.strip_prefix("❯ ").unwrap_or(trimmed);
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

pub(crate) fn is_prefixed_exchange_response_heading_for_prefix_repair(trimmed: &str) -> bool {
    let Some(stripped) = trimmed.strip_prefix("❯ ") else {
        return false;
    };
    is_exchange_response_heading_for_prefix_repair(stripped)
}

pub(crate) fn normalization_target_matches_line(
    line: &str,
    target_counts: &std::collections::HashMap<String, usize>,
) -> bool {
    let normalized = line.trim_end();
    target_counts.contains_key(normalized)
        || normalized
            .strip_prefix("❯ ")
            .is_some_and(|stripped| target_counts.contains_key(stripped))
}

pub(crate) fn starts_prompt_run_after_response(trimmed: &str, is_target: bool) -> bool {
    crate::diff::line_looks_like_prompt_prefix_repair_start(trimmed, is_target)
}

pub(crate) fn starts_targeted_prompt_repair_after_response(trimmed: &str, is_target: bool) -> bool {
    crate::diff::line_looks_like_targeted_prompt_prefix_repair_start(trimmed, is_target)
}

pub(crate) fn starts_targeted_or_prefixed_prompt_repair_after_response(
    trimmed: &str,
    is_target: bool,
) -> bool {
    starts_targeted_prompt_repair_after_response(
        trimmed,
        is_target || trimmed.trim_start().starts_with('❯'),
    )
}

pub(crate) fn exchange_prompt_prefix_eligible_lines<'a>(
    content: &'a str,
    target_counts: Option<&std::collections::HashMap<String, usize>>,
) -> Vec<&'a str> {
    let boundary_prefix = "<!-- agent:boundary:";
    let mut eligible = Vec::new();
    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;

    for line in exchange_user_region(content).lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(boundary_prefix) {
            in_response_block = false;
            response_heading_was_prefixed = false;
            continue;
        }
        if is_exchange_response_heading_for_prefix_repair(trimmed) {
            in_response_block = true;
            response_heading_was_prefixed =
                is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
            continue;
        }
        if crate::diff::line_looks_like_markdown_list_item(trimmed) {
            continue;
        }

        let is_target =
            target_counts.is_some_and(|counts| normalization_target_matches_line(line, counts));
        if in_response_block {
            let starts_prompt = if target_counts.is_some() {
                starts_targeted_or_prefixed_prompt_repair_after_response(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                )
            } else {
                starts_prompt_run_after_response(trimmed, false)
            };
            if starts_prompt {
                in_response_block = false;
                response_heading_was_prefixed = false;
            } else {
                continue;
            }
        }

        eligible.push(line);
    }

    eligible
}

/// Compare the committed/snapshot document against the working tree and return
/// exchange user-region lines that should regain a missing `❯ ` prefix.
pub fn extract_post_commit_normalization_targets(committed: &str, working: &str) -> Vec<String> {
    let committed_comps = component::parse(committed).unwrap_or_default();
    let working_comps = component::parse(working).unwrap_or_default();

    let committed_exc = committed_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(committed))
        .unwrap_or("");
    let working_exc = working_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(working))
        .unwrap_or("");

    if committed_exc == working_exc {
        return vec![];
    }

    let mut working_prefixed = std::collections::HashMap::<String, usize>::new();
    let mut working_unprefixed = std::collections::HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(working_exc, None) {
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(stripped) = trimmed.strip_prefix("❯ ") {
            *working_prefixed.entry(stripped.to_string()).or_default() += 1;
        } else {
            *working_unprefixed.entry(trimmed.to_string()).or_default() += 1;
        }
    }

    let mut committed_prefixed = std::collections::HashMap::<String, usize>::new();
    for line in exchange_prompt_prefix_eligible_lines(committed_exc, None) {
        let Some(stripped) = line.strip_prefix("❯ ") else {
            continue;
        };
        let normalized = stripped.trim_end();
        if normalized.is_empty() {
            continue;
        }
        *committed_prefixed
            .entry(normalized.to_string())
            .or_default() += 1;
    }

    let mut missing_counts = std::collections::HashMap::<String, usize>::new();
    for (line, committed_count) in committed_prefixed {
        let working_prefixed_count = working_prefixed.get(&line).copied().unwrap_or(0);
        let working_unprefixed_count = working_unprefixed.get(&line).copied().unwrap_or(0);
        let missing = committed_count.saturating_sub(working_prefixed_count);
        let repairable = missing.min(working_unprefixed_count);
        if repairable > 0 {
            missing_counts.insert(line, repairable);
        }
    }

    let mut targets = Vec::new();
    for line in exchange_prompt_prefix_eligible_lines(committed_exc, None) {
        let Some(stripped) = line.strip_prefix("❯ ") else {
            continue;
        };
        let normalized = stripped.trim_end();
        let Some(remaining) = missing_counts.get_mut(normalized) else {
            continue;
        };
        if *remaining == 0 {
            continue;
        }
        targets.push(stripped.to_string());
        *remaining -= 1;
    }

    targets
}

/// Apply `❯ ` prefix normalization to matching lines in the exchange user
/// region of a full document.
pub fn normalize_exchange_prefixes_for_targets(doc: &str, prefix_lines: &[String]) -> String {
    if prefix_lines.is_empty() {
        return doc.to_string();
    }

    let open_tag = "<!-- agent:exchange";
    let close_tag = "<!-- /agent:exchange -->";
    let boundary_prefix = "<!-- agent:boundary:";

    let Some(open_match) = doc.find(open_tag) else {
        return doc.to_string();
    };
    let Some(close_idx) = doc[open_match..]
        .find(close_tag)
        .map(|idx| open_match + idx)
    else {
        return doc.to_string();
    };
    let Some(open_end) = doc[open_match..]
        .find("-->")
        .map(|idx| open_match + idx + 3)
    else {
        return doc.to_string();
    };

    let before_exchange = &doc[..open_end];
    let exchange_content = &doc[open_end..close_idx];
    let after_exchange = &doc[close_idx..];

    let mut user_region_end = exchange_content.len();
    let mut offset = 0;
    for line in exchange_content.lines() {
        if line.trim().starts_with(boundary_prefix) {
            user_region_end = offset;
        }
        offset += line.len() + 1;
    }
    let user_region = &exchange_content[..user_region_end];
    let agent_region = &exchange_content[user_region_end..];

    let mut remaining = normalization_target_counts(prefix_lines);
    if remaining.is_empty() {
        return doc.to_string();
    }

    let mut in_response_block = false;
    let mut response_heading_was_prefixed = false;
    let normalized_user_region = user_region
        .split('\n')
        .map(|doc_line| {
            let trimmed = doc_line.trim();
            if trimmed.starts_with(boundary_prefix) {
                in_response_block = false;
                response_heading_was_prefixed = false;
                return doc_line.to_string();
            }
            if is_exchange_response_heading_for_prefix_repair(trimmed) {
                in_response_block = true;
                response_heading_was_prefixed =
                    is_prefixed_exchange_response_heading_for_prefix_repair(trimmed);
                return doc_line.to_string();
            }
            let normalized = doc_line.trim_end();
            let is_target = normalization_target_matches_line(doc_line, &remaining);
            if in_response_block {
                if starts_targeted_or_prefixed_prompt_repair_after_response(
                    trimmed,
                    is_target && !response_heading_was_prefixed,
                ) {
                    in_response_block = false;
                    response_heading_was_prefixed = false;
                } else {
                    return doc_line.to_string();
                }
            }
            if normalized.starts_with("❯ ")
                || crate::diff::line_looks_like_plain_response_after_prompt(normalized)
            {
                return doc_line.to_string();
            }
            let Some(remaining_count) = remaining.get_mut(normalized) else {
                return doc_line.to_string();
            };
            if *remaining_count == 0 {
                return doc_line.to_string();
            }
            *remaining_count -= 1;
            format!("❯ {doc_line}")
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!("{before_exchange}{normalized_user_region}{agent_region}{after_exchange}")
}
