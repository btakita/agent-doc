//! Pure baseline comparisons for realtime document authority.
//!
//! Callers own IO, snapshots, git, and active-session lookup. This module owns
//! deterministic comparisons between authoritative document text and baseline
//! text so turn/session pipelines do not each rebuild their own comparison
//! policy. A baseline is not a competing document source; it is an immutable
//! turn/checkpoint fact used to reason about what changed in the current model.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RealtimeSteering {
    None,
    PromptTarget { preview: String, verbatim: String },
    ContentEdit { preview: String, verbatim: String },
    PromptDeleted { preview: String, verbatim: String },
    PromptReduced { preview: String, verbatim: String },
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

    /// First non-empty line of the steering change — for logs / short markers.
    pub fn preview(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::PromptTarget { preview, .. }
            | Self::ContentEdit { preview, .. }
            | Self::PromptDeleted { preview, .. }
            | Self::PromptReduced { preview, .. } => Some(preview),
        }
    }

    /// The full, verbatim operator steering text. `#realtime-steering-verbatim`:
    /// the simplest robust way to make the agent address a prompt the operator
    /// added mid-turn is to hand it the operator's prompt **verbatim**, not a
    /// first-line preview, so no steering intent is silently truncated away.
    pub fn verbatim(&self) -> Option<&str> {
        match self {
            Self::None => None,
            Self::PromptTarget { verbatim, .. }
            | Self::ContentEdit { verbatim, .. }
            | Self::PromptDeleted { verbatim, .. }
            | Self::PromptReduced { verbatim, .. } => Some(verbatim),
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
                verbatim: baseline_prompt.trim().to_string(),
            };
        }
        UnresolvedPromptDelta::Reduced { current_prompt } => {
            return RealtimeSteering::PromptReduced {
                preview: prompt_bearing_preview(&current_prompt),
                verbatim: current_prompt.trim().to_string(),
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
    let verbatim = change.text.trim().to_string();
    match change.kind {
        agent_doc_diff::PromptBearingChangeKind::PromptTarget => {
            RealtimeSteering::PromptTarget { preview, verbatim }
        }
        agent_doc_diff::PromptBearingChangeKind::ContentEdit => {
            RealtimeSteering::ContentEdit { preview, verbatim }
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
    agent_doc_turn::document_drift::exchange_has_new_appended_content(baseline, current)
}

pub fn extract_normalized_exchange_body(doc: &str) -> Option<String> {
    agent_doc_turn::document_drift::extract_normalized_exchange_body(doc)
}

pub fn exchange_only_promptless_content_delta(baseline: &str, current: &str) -> bool {
    agent_doc_turn::document_drift::exchange_only_promptless_content_drift(baseline, current)
}

pub fn active_session_delta_is_only_exchange_or_backlog_metadata(
    baseline: &str,
    current: &str,
) -> bool {
    agent_doc_turn::document_drift::active_session_drift_is_only_exchange_or_backlog_metadata(
        baseline, current,
    )
}

pub fn promptless_comment_only_delta(baseline: &str, current: &str) -> bool {
    agent_doc_turn::document_drift::promptless_comment_only_drift(baseline, current)
}

pub fn detect_bypassed_response_write_between(
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<String> {
    agent_doc_turn::document_drift::detect_bypassed_response_write_between(
        snapshot_doc,
        current_doc,
    )
}

pub fn is_exchange_response_heading(trimmed: &str) -> bool {
    agent_doc_turn::closeout_signal::is_exchange_response_heading(trimmed)
}

pub fn is_direct_response_patchback_heading(trimmed: &str) -> bool {
    agent_doc_turn::closeout_signal::is_direct_response_patchback_heading(trimmed)
}

pub fn has_new_response_heading_marker(snapshot_doc: &str, current_doc: &str) -> bool {
    agent_doc_turn::closeout_signal::has_new_response_heading_marker(snapshot_doc, current_doc)
}

pub fn is_binary_authored_recovery_diagnostic_heading(trimmed: &str) -> bool {
    agent_doc_turn::closeout_signal::is_binary_authored_recovery_diagnostic_heading(trimmed)
}

pub fn mask_exchange_component_content(doc: &str) -> Option<String> {
    agent_doc_turn::document_drift::mask_exchange_component_content(doc)
}

pub fn mask_components_by_name(doc: &str, names: &[&str]) -> Option<String> {
    agent_doc_turn::document_drift::mask_components_by_name(doc, names)
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

        let steering = BaselineComparison::new(&baseline, &current).realtime_steering();
        assert_eq!(steering.label(), Some("prompt_deleted"));
        assert_eq!(steering.preview(), Some("Original prompt"));
        // `#realtime-steering-verbatim`: the full operator text is retained, not
        // just the first-line preview.
        assert!(steering.verbatim().unwrap().contains("more detail"));
    }

    #[test]
    fn realtime_steering_detects_prompt_reduced() {
        let baseline = doc("❯ Original prompt\nmore detail\n");
        let current = doc("❯ Original prompt\n");

        let steering = BaselineComparison::new(&baseline, &current).realtime_steering();
        assert_eq!(steering.label(), Some("prompt_reduced"));
        assert_eq!(steering.preview(), Some("Original prompt"));
        assert!(steering.verbatim().unwrap().contains("Original prompt"));
    }

    #[test]
    fn realtime_steering_detects_prompt_target_added() {
        let baseline = doc("");
        let current = doc("❯ New prompt\n");

        let steering = BaselineComparison::new(&baseline, &current).realtime_steering();
        assert_eq!(steering.label(), Some("prompt_target"));
        assert_eq!(steering.preview(), Some("❯ New prompt"));
        assert_eq!(steering.verbatim(), Some("❯ New prompt"));
    }

    #[test]
    fn realtime_steering_verbatim_retains_multiline_operator_prompt() {
        // `#realtime-steering-verbatim`: a multi-line operator prompt added
        // mid-turn is surfaced in full so the agent can address the whole intent,
        // not just the first line.
        let baseline = doc("");
        let current = doc("❯ Fix the JB error:\n```\nRead access is allowed from inside read-action only\n```\n");
        let steering = BaselineComparison::new(&baseline, &current).realtime_steering();
        assert_eq!(steering.label(), Some("prompt_target"));
        let verbatim = steering.verbatim().unwrap();
        assert!(verbatim.contains("Fix the JB error:"));
        assert!(verbatim.contains("Read access is allowed from inside read-action only"));
    }
}
