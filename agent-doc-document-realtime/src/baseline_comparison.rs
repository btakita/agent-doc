//! Pure baseline comparisons for realtime document authority.
//!
//! Callers own IO, snapshots, git, and active-session lookup. This module owns
//! deterministic comparisons between authoritative document text and baseline
//! text so turn/session pipelines do not each rebuild their own comparison
//! policy. A baseline is not a competing document source; it is an immutable
//! turn/checkpoint fact used to reason about what changed in the current model.

use agent_doc_prompt_lines::text_line_looks_like_prompt_target;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtimeSteering {
    None,
    PromptTarget { preview: String },
    ContentEdit { preview: String },
    PromptDeleted { preview: String },
    PromptReduced { preview: String },
}

impl RealtimeSteering {
    pub fn is_present(&self) -> bool {
        !matches!(self, Self::None)
    }

    pub fn label(&self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::PromptTarget { .. } => Some("prompt_target"),
            Self::ContentEdit { .. } => Some("content_edit"),
            Self::PromptDeleted { .. } => Some("prompt_deleted"),
            Self::PromptReduced { .. } => Some("prompt_reduced"),
        }
    }

    pub fn preview(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::PromptTarget { preview }
            | Self::ContentEdit { preview }
            | Self::PromptDeleted { preview }
            | Self::PromptReduced { preview } => Some(preview),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BaselineComparison<'a> {
    pub baseline: &'a str,
    pub current: &'a str,
}

impl<'a> BaselineComparison<'a> {
    pub fn new(baseline: &'a str, current: &'a str) -> Self {
        Self { baseline, current }
    }

    pub fn is_equal(&self) -> bool {
        self.current == self.baseline
    }

    pub fn normalized_exchange_equal(&self) -> bool {
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
            self.current,
        ) == agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
            self.baseline,
        )
    }

    pub fn active_session_delta_is_only_exchange_or_backlog_metadata(&self) -> bool {
        active_session_delta_is_only_exchange_or_backlog_metadata(self.baseline, self.current)
    }

    pub fn promptless_comment_only_delta(&self) -> bool {
        promptless_comment_only_delta(self.baseline, self.current)
    }

    pub fn exchange_only_promptless_content_delta(&self) -> bool {
        exchange_only_promptless_content_delta(self.baseline, self.current)
    }

    pub fn exchange_has_new_appended_content(&self) -> bool {
        let baseline =
            agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
                self.baseline,
            );
        let current =
            agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(
                self.current,
            );
        exchange_has_new_appended_content(&baseline, &current)
    }

    pub fn bypassed_response_write_marker(&self) -> Option<String> {
        detect_bypassed_response_write_between(self.baseline, self.current)
    }

    pub fn realtime_steering(&self) -> RealtimeSteering {
        realtime_steering_between(self.baseline, self.current)
    }
}

pub fn realtime_steering_between(baseline: &str, current: &str) -> RealtimeSteering {
    match unresolved_prompt_delta(baseline, current) {
        UnresolvedPromptDelta::Deleted { baseline_prompt } => {
            return RealtimeSteering::PromptDeleted {
                preview: prompt_bearing_preview(&baseline_prompt),
            };
        }
        UnresolvedPromptDelta::Reduced { current_prompt } => {
            return RealtimeSteering::PromptReduced {
                preview: prompt_bearing_preview(&current_prompt),
            };
        }
        UnresolvedPromptDelta::None | UnresolvedPromptDelta::AddedOrExpanded => {}
    }

    let norm = |s: &str| {
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(s)
    };
    let base_norm = norm(&agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard(baseline));
    let cur_norm = norm(&agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard(current));
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(&base_norm, &cur_norm) else {
        return RealtimeSteering::None;
    };
    let Some(change) =
        agent_doc_diff::first_unstarted_prompt_bearing_change_from_diff(&diff_text, current)
    else {
        return RealtimeSteering::None;
    };
    let preview = prompt_bearing_preview(&change.text);
    match change.kind {
        agent_doc_diff::PromptBearingChangeKind::PromptTarget => {
            RealtimeSteering::PromptTarget { preview }
        }
        agent_doc_diff::PromptBearingChangeKind::ContentEdit => {
            RealtimeSteering::ContentEdit { preview }
        }
        agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
        | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact => RealtimeSteering::None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum UnresolvedPromptDelta {
    None,
    AddedOrExpanded,
    Deleted { baseline_prompt: String },
    Reduced { current_prompt: String },
}

fn unresolved_prompt_delta(baseline: &str, current: &str) -> UnresolvedPromptDelta {
    let baseline_prompt = unresolved_exchange_prompt_in_content(baseline);
    let current_prompt = unresolved_exchange_prompt_in_content(current);
    match (baseline_prompt, current_prompt) {
        (None, _) => UnresolvedPromptDelta::None,
        (Some(baseline_prompt), None) => UnresolvedPromptDelta::Deleted { baseline_prompt },
        (Some(baseline_prompt), Some(current_prompt)) => {
            let baseline_norm = normalize_prompt_for_delta(&baseline_prompt);
            let current_norm = normalize_prompt_for_delta(&current_prompt);
            if baseline_norm == current_norm {
                UnresolvedPromptDelta::None
            } else if baseline_norm.contains(&current_norm)
                && current_norm.len() < baseline_norm.len()
            {
                UnresolvedPromptDelta::Reduced { current_prompt }
            } else {
                UnresolvedPromptDelta::AddedOrExpanded
            }
        }
    }
}

fn normalize_prompt_for_delta(prompt: &str) -> String {
    prompt
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn unresolved_exchange_prompt_in_content(content: &str) -> Option<String> {
    let body = exchange_body(content)?;
    let lines: Vec<&str> = body.lines().collect();
    let tail_start = boundary_tail_start(&lines);
    let tail = &lines[tail_start..];

    let first_response_idx = tail
        .iter()
        .position(|line| is_exchange_response_heading(line.trim()));
    if first_response_idx.is_some() {
        return None;
    }
    let prompt_lines: Vec<String> = tail
        .iter()
        .map(|line| line.trim())
        .filter(|line| {
            !line.is_empty()
                && !line.starts_with("<!--")
                && !line.starts_with("-->")
                && !is_exchange_response_heading(line)
                && !agent_doc_diff::line_is_binary_authored_ipc_proof_diagnostic(line)
                && !agent_doc_diff::line_is_binary_authored_compact_summary(line)
        })
        .map(normalized_prompt_for_match)
        .filter(|line| !line.is_empty())
        .collect();
    if prompt_lines.is_empty() {
        return None;
    }
    Some(prompt_lines.join("\n"))
}

fn exchange_body(doc: &str) -> Option<String> {
    let body = agent_doc_frontmatter::frontmatter::parse(doc)
        .map(|(_, body)| body.to_string())
        .unwrap_or_else(|_| doc.to_string());
    let components = agent_doc_element::element::parse(&body).ok()?;
    let exchange = components
        .iter()
        .find(|component| component.name == "exchange")?;
    Some(exchange.content(&body).to_string())
}

fn boundary_tail_start(lines: &[&str]) -> usize {
    lines
        .iter()
        .rposition(|line| line.trim().starts_with("<!-- agent:boundary:"))
        .map(|idx| idx + 1)
        .unwrap_or(0)
}

fn normalized_prompt_for_match(line: &str) -> String {
    line.trim()
        .trim_start_matches('❯')
        .trim()
        .trim_start_matches("- ")
        .trim()
        .to_string()
}

fn prompt_bearing_preview(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(text)
        .trim()
        .to_string()
}

pub fn exchange_has_new_appended_content(baseline: &str, current: &str) -> bool {
    let Some(baseline_exchange) = extract_normalized_exchange_body(baseline) else {
        return false;
    };
    let Some(current_exchange) = extract_normalized_exchange_body(current) else {
        return false;
    };
    if current_exchange == baseline_exchange {
        return false;
    }
    let baseline_lines: Vec<&str> = baseline_exchange.lines().collect();
    let current_lines: Vec<&str> = current_exchange.lines().collect();
    if current_lines.len() <= baseline_lines.len() {
        return false;
    }
    for (i, line) in baseline_lines.iter().enumerate() {
        if current_lines.get(i) != Some(line) {
            return false;
        }
    }
    let appended: String = current_lines[baseline_lines.len()..].join("\n");
    if appended
        .lines()
        .map(str::trim)
        .any(is_exchange_response_heading)
    {
        return true;
    }
    if appended.lines().any(text_line_looks_like_prompt_target) {
        return false;
    }
    true
}

pub fn extract_normalized_exchange_body(doc: &str) -> Option<String> {
    let (_, body) = agent_doc_frontmatter::frontmatter::parse(doc).ok()?;
    let components = agent_doc_element::element::parse(body).ok()?;
    for component in &components {
        if component.name == "exchange" {
            return Some(component.content(body).to_string());
        }
    }
    None
}

pub fn exchange_only_promptless_content_delta(baseline: &str, current: &str) -> bool {
    if baseline == current {
        return true;
    }
    let Some(baseline_masked) = mask_exchange_component_content(baseline) else {
        return false;
    };
    let Some(current_masked) = mask_exchange_component_content(current) else {
        return false;
    };
    normalize_transient_markers(&baseline_masked) == normalize_transient_markers(&current_masked)
}

pub fn active_session_delta_is_only_exchange_or_backlog_metadata(
    baseline: &str,
    current: &str,
) -> bool {
    let Some(baseline_masked) = mask_components_by_name(baseline, &["exchange", "backlog"]) else {
        return false;
    };
    let Some(current_masked) = mask_components_by_name(current, &["exchange", "backlog"]) else {
        return false;
    };
    normalize_transient_markers(&baseline_masked) == normalize_transient_markers(&current_masked)
}

pub fn promptless_comment_only_delta(baseline: &str, current: &str) -> bool {
    if baseline == current {
        return true;
    }
    normalize_transient_markers(&agent_doc_diff::strip_comments(baseline))
        == normalize_transient_markers(&agent_doc_diff::strip_comments(current))
}

pub fn detect_bypassed_response_write_between(
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<String> {
    let snap_norm = normalize_transient_markers(snapshot_doc);
    let cur_norm = normalize_transient_markers(current_doc);
    if cur_norm == snap_norm {
        return None;
    }
    if !has_new_response_heading_marker(&snap_norm, &cur_norm) {
        return None;
    }

    let diff_text = agent_doc_diff::unified_diff_from_contents(&snap_norm, &cur_norm)?;

    let diff = similar::TextDiff::from_lines(&snap_norm, &cur_norm);
    for change in diff.iter_all_changes() {
        if change.tag() != similar::ChangeTag::Insert {
            continue;
        }
        let trimmed = change.value().trim();
        if is_binary_authored_recovery_diagnostic_heading(trimmed) {
            continue;
        }
        if is_direct_response_patchback_heading(trimmed) {
            if let Some(bare_target) =
                agent_doc_diff::first_bare_prompt_prefix_target_before_marker(&diff_text, trimmed)
            {
                return Some(format!(
                    "{} (bare prompt target missing `❯ `: {})",
                    trimmed, bare_target
                ));
            }
            return Some(trimmed.to_string());
        }
    }
    None
}

pub fn is_exchange_response_heading(trimmed: &str) -> bool {
    trimmed == "## Assistant"
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
}

pub fn is_direct_response_patchback_heading(trimmed: &str) -> bool {
    trimmed.starts_with("### Re:") || trimmed == "## Assistant"
}

pub fn has_new_response_heading_marker(snapshot_doc: &str, current_doc: &str) -> bool {
    let snapshot_counts = response_heading_marker_counts(snapshot_doc);
    let current_counts = response_heading_marker_counts(current_doc);
    current_counts
        .into_iter()
        .any(|(marker, count)| count > snapshot_counts.get(&marker).copied().unwrap_or(0))
}

fn response_heading_marker_counts(doc: &str) -> std::collections::BTreeMap<String, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for line in doc.lines() {
        let trimmed = line.trim();
        if is_direct_response_patchback_heading(trimmed) {
            *counts.entry(trimmed.to_string()).or_insert(0) += 1;
        }
    }
    counts
}

pub fn is_binary_authored_recovery_diagnostic_heading(trimmed: &str) -> bool {
    (trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:"))
        && trimmed.contains("interrupted-cycle recovery")
}

pub fn mask_exchange_component_content(doc: &str) -> Option<String> {
    mask_components_by_name(doc, &["exchange"])
}

pub fn mask_components_by_name(doc: &str, names: &[&str]) -> Option<String> {
    let components = agent_doc_element::element::parse(doc).ok()?;
    let mut masked = doc.to_string();
    let mut saw_target = false;
    for component in components.iter().rev() {
        if !names.contains(&component.name.as_str()) {
            continue;
        }
        saw_target = true;
        masked.replace_range(component.open_end..component.close_start, "\n");
    }
    saw_target.then_some(masked)
}

fn normalize_transient_markers(doc: &str) -> String {
    agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(doc)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(prompt_tail: &str) -> String {
        format!(
            concat!(
                "---\n",
                "agent_doc_format: template\n",
                "---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "<!-- agent:boundary:base -->\n",
                "{}",
                "<!-- /agent:exchange -->\n"
            ),
            prompt_tail
        )
    }

    #[test]
    fn realtime_steering_detects_prompt_deleted() {
        let baseline = doc("❯ Original prompt\nmore detail\n");
        let current = doc("");

        assert_eq!(
            BaselineComparison::new(&baseline, &current).realtime_steering(),
            RealtimeSteering::PromptDeleted {
                preview: "Original prompt".to_string()
            }
        );
    }

    #[test]
    fn realtime_steering_detects_prompt_reduced() {
        let baseline = doc("❯ Original prompt\nmore detail\n");
        let current = doc("❯ Original prompt\n");

        assert_eq!(
            BaselineComparison::new(&baseline, &current).realtime_steering(),
            RealtimeSteering::PromptReduced {
                preview: "Original prompt".to_string()
            }
        );
    }

    #[test]
    fn realtime_steering_detects_prompt_target_added() {
        let baseline = doc("");
        let current = doc("❯ New prompt\n");

        assert_eq!(
            BaselineComparison::new(&baseline, &current).realtime_steering(),
            RealtimeSteering::PromptTarget {
                preview: "❯ New prompt".to_string()
            }
        );
    }
}
