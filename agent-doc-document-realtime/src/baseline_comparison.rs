//! Pure baseline comparisons for realtime document authority.
//!
//! Callers own IO, snapshots, git, and active-session lookup. This module owns
//! deterministic comparisons between authoritative document text and baseline
//! text so turn/session pipelines do not each rebuild their own comparison
//! policy. A baseline is not a competing document source; it is an immutable
//! turn/checkpoint fact used to reason about what changed in the current model.

use sha2::{Digest, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write;

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

    pub fn turn_state(&self) -> agent_doc_turn::cp_projection::TurnSteeringState {
        use agent_doc_turn::cp_projection::TurnSteeringState;
        match self {
            Self::None => TurnSteeringState::None,
            Self::PromptTarget { .. } => TurnSteeringState::PromptTarget,
            Self::ContentEdit { .. } => TurnSteeringState::ContentEdit,
            Self::PromptDeleted { .. } => TurnSteeringState::PromptDeleted,
            Self::PromptReduced { .. } => TurnSteeringState::PromptReduced,
        }
    }

    /// Stable semantic identity for one observable steering element.
    ///
    /// The complete normalized body participates in the hash, not only its
    /// first-line preview. This keeps distinct multi-line directives separate
    /// while deduplicating the same CRDT-visible directive across reconnects.
    pub fn identity(&self) -> Option<String> {
        let label = self.label()?;
        let verbatim = self.verbatim()?.replace("\r\n", "\n");
        let body = verbatim.trim();
        let digest = Sha256::digest(format!("{label}\0{body}").as_bytes());
        let mut identity = String::with_capacity(digest.len() * 2);
        for byte in digest {
            write!(&mut identity, "{byte:02x}").expect("writing to String cannot fail");
        }
        Some(identity)
    }
}

/// All realtime operator steering directives added since the baseline, in document
/// order (oldest-first).
///
/// `#realtime-steering-aggregate` (plan Phase 6): steering is **not** a FIFO
/// `QueueCell` drained one head at a time. The operator may add several prompts
/// while a turn is active, and every one must reach the agent **at once**, verbatim,
/// so the agent can process the concurrent directives together and find patterns
/// across them instead of answering them serially. This set is that aggregate view;
/// [`RealtimeSteering`] (single) remains the "is there any steering / what is the
/// primary one" summary used by the interrupt/label paths.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct RealtimeSteeringSet {
    directives: Vec<RealtimeSteering>,
}

impl RealtimeSteeringSet {
    pub fn new(directives: Vec<RealtimeSteering>) -> Self {
        let mut identities = BTreeSet::new();
        let directives = directives
            .into_iter()
            .filter(|directive| {
                directive
                    .identity()
                    .is_some_and(|identity| identities.insert(identity))
            })
            .collect();
        Self { directives }
    }

    /// Rehydrate the durable identity-keyed controller projection. During a
    /// rolling upgrade an aggregate-only payload is represented as one legacy
    /// directive so it remains visible, but only `elements` are set evidence.
    pub fn from_turn_projection(
        projection: &agent_doc_turn::cp_projection::TurnSteeringProjection,
    ) -> Self {
        let mut elements = projection.elements.values().collect::<Vec<_>>();
        elements.sort_by_key(|element| element.ordinal);
        if !elements.is_empty() {
            return Self::new(
                elements
                    .into_iter()
                    .map(|element| {
                        steering_from_turn_fields(
                            element.state,
                            element.preview.clone(),
                            element.verbatim.clone(),
                        )
                    })
                    .collect(),
            );
        }
        if !projection.is_present() {
            return Self::default();
        }
        Self::new(vec![steering_from_turn_fields(
            projection.state,
            projection.preview.clone(),
            projection.verbatim.clone().unwrap_or_default(),
        )])
    }

    pub fn is_present(&self) -> bool {
        !self.directives.is_empty()
    }

    pub fn len(&self) -> usize {
        self.directives.len()
    }

    pub fn is_empty(&self) -> bool {
        self.directives.is_empty()
    }

    pub fn directives(&self) -> &[RealtimeSteering] {
        &self.directives
    }

    /// The primary (first / oldest) directive, for callers that still need a single
    /// [`RealtimeSteering`] (labels, short markers). Falls back to `None` when empty.
    pub fn primary(&self) -> RealtimeSteering {
        self.directives
            .first()
            .cloned()
            .unwrap_or(RealtimeSteering::None)
    }

    /// Every directive's full verbatim text, concatenated oldest-first so the agent
    /// receives all concurrent steering at once. Directives are separated by a blank
    /// line; a leading count header is included when there is more than one so the
    /// agent knows to look for patterns across them. Returns `None` when empty.
    pub fn verbatim_aggregate(&self) -> Option<String> {
        let bodies: Vec<&str> = self
            .directives
            .iter()
            .filter_map(RealtimeSteering::verbatim)
            .filter(|v| !v.trim().is_empty())
            .collect();
        if bodies.is_empty() {
            return None;
        }
        if bodies.len() == 1 {
            return Some(bodies[0].to_string());
        }
        let mut out = format!(
            "{} concurrent operator steering directives (address ALL of them this turn; look for patterns across them):",
            bodies.len()
        );
        for (idx, body) in bodies.iter().enumerate() {
            out.push_str(&format!(
                "\n\n[steering {}/{}] {}",
                idx + 1,
                bodies.len(),
                body
            ));
        }
        Some(out)
    }

    /// Project the full operator-steering set into the wire-stable turn view.
    /// The primary directive supplies the compact state/preview while `count`
    /// and `verbatim` retain the complete aggregate for every consumer.
    pub fn turn_projection(&self) -> agent_doc_turn::cp_projection::TurnSteeringProjection {
        use agent_doc_turn::cp_projection::{
            TurnSteeringElementProjection, TurnSteeringProjection, TurnSteeringState,
        };

        let primary = self.primary();
        let preview = primary.preview().map(str::to_string);
        let state = primary.turn_state();
        if matches!(state, TurnSteeringState::None) {
            return TurnSteeringProjection::none();
        }
        let elements = self
            .directives
            .iter()
            .enumerate()
            .filter_map(|(ordinal, directive)| {
                Some((
                    directive.identity()?,
                    TurnSteeringElementProjection {
                        state: directive.turn_state(),
                        ordinal,
                        preview: directive.preview().map(str::to_string),
                        verbatim: directive.verbatim()?.to_string(),
                    },
                ))
            })
            .collect::<BTreeMap<_, _>>();
        TurnSteeringProjection::observed_identity_set(
            state,
            preview,
            self.verbatim_aggregate(),
            elements,
        )
    }
}

fn steering_from_turn_fields(
    state: agent_doc_turn::cp_projection::TurnSteeringState,
    preview: Option<String>,
    verbatim: String,
) -> RealtimeSteering {
    use agent_doc_turn::cp_projection::TurnSteeringState;
    let preview =
        preview.unwrap_or_else(|| verbatim.lines().next().unwrap_or_default().to_string());
    match state {
        TurnSteeringState::None => RealtimeSteering::None,
        TurnSteeringState::PromptTarget => RealtimeSteering::PromptTarget { preview, verbatim },
        TurnSteeringState::ContentEdit => RealtimeSteering::ContentEdit { preview, verbatim },
        TurnSteeringState::PromptDeleted => RealtimeSteering::PromptDeleted { preview, verbatim },
        TurnSteeringState::PromptReduced => RealtimeSteering::PromptReduced { preview, verbatim },
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

    /// All concurrent operator steering directives since the baseline
    /// (`#realtime-steering-aggregate`). See [`RealtimeSteeringSet`].
    pub fn realtime_steering_all(&self) -> RealtimeSteeringSet {
        realtime_steering_all_between(self.baseline, self.current)
    }
}

/// Aggregate form of [`realtime_steering_between`]: every unstarted prompt-bearing
/// directive the operator added since the baseline, oldest-first
/// (`#realtime-steering-aggregate`, plan Phase 6). A deleted/reduced prompt is a
/// single-directive state, so those short-circuit to a one-element set to match the
/// single-directive path exactly; the common "operator added N prompts mid-turn"
/// case yields all N so the agent addresses them together.
pub fn realtime_steering_all_between(baseline: &str, current: &str) -> RealtimeSteeringSet {
    let mut directives = exchange_steering_all_between(baseline, current).directives;
    directives.extend(queue_steering_between(baseline, current));
    RealtimeSteeringSet::new(directives)
}

/// Queue instructions have their own parser: the exchange guard deliberately
/// excludes managed queue syntax. Compare complete prompt revisions, ignoring
/// selection/priority decoration, completed rows, and reordering.
fn queue_steering_between(baseline: &str, current: &str) -> Vec<RealtimeSteering> {
    use agent_doc_queue::document_queue::{self, QueueEntry};
    let entries = |content: &str| {
        document_queue::parse(&agent_doc_queue::queue_prompt_drift::queue_component_text(
            content,
        ))
        .expect("queue parser is tolerant")
    };
    let normalize = |text: &str| {
        agent_doc_document::queue_projection::strip_priority_markers(text)
            .replace("\r\n", "\n")
            .trim()
            .to_string()
    };
    let mut baseline_counts = BTreeMap::<String, usize>::new();
    for entry in entries(baseline) {
        if let QueueEntry::Prompt(prompt) = entry {
            *baseline_counts.entry(normalize(&prompt.text)).or_default() += 1;
        }
    }
    let mut directives = Vec::new();
    for entry in entries(current) {
        if let QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) = &entry {
            let text = normalize(&prompt.text);
            let count = baseline_counts.entry(text).or_default();
            if *count > 0 {
                *count -= 1;
            } else if matches!(entry, QueueEntry::Prompt(_)) {
                let verbatim = prompt.text.trim().to_string();
                directives.push(RealtimeSteering::ContentEdit {
                    preview: prompt_bearing_preview(&verbatim),
                    verbatim,
                });
            }
        }
    }
    // Adding independent queued work does not change the active request. Only
    // replacement of an existing live revision belongs in this turn's steering
    // set; ordinary additions remain owned by the normal queue drain.
    if baseline_counts.values().any(|count| *count > 0) {
        directives
    } else {
        Vec::new()
    }
}

fn exchange_steering_all_between(baseline: &str, current: &str) -> RealtimeSteeringSet {
    match unresolved_prompt_delta(baseline, current) {
        UnresolvedPromptDelta::Deleted { .. } | UnresolvedPromptDelta::Reduced { .. } => {
            return RealtimeSteeringSet::new(vec![exchange_steering_between(baseline, current)]);
        }
        UnresolvedPromptDelta::None | UnresolvedPromptDelta::AddedOrExpanded => {}
    }

    let norm = |s: &str| {
        agent_doc_document::commit_normalization::normalize_committed_exchange_artifacts(s)
    };
    let base_norm = norm(&agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard(baseline));
    let cur_norm = norm(&agent_doc_diff::prompt_bearing_body_for_unstarted_prompt_guard(current));
    let Some(diff_text) = agent_doc_diff::unified_diff_from_contents(&base_norm, &cur_norm) else {
        return RealtimeSteeringSet::default();
    };
    let directives =
        agent_doc_diff::all_unstarted_prompt_bearing_changes_from_diff(&diff_text, current)
            .into_iter()
            .map(|change| {
                let preview = prompt_bearing_preview(&change.text);
                let verbatim = change.text.trim().to_string();
                // `#responsereplaysteering`: a verbatim replay of committed
                // response body is binary-owned content, not operator steering.
                // Without this the phantom directive can never be answered and
                // every later `session-check` INTERRUPTs, so the turn never
                // closes.
                if agent_doc_diff::prompt_target_replays_committed_response_body(
                    baseline,
                    &change.text,
                ) {
                    return RealtimeSteering::None;
                }
                match change.kind {
                    agent_doc_diff::PromptBearingChangeKind::PromptTarget => {
                        RealtimeSteering::PromptTarget { preview, verbatim }
                    }
                    agent_doc_diff::PromptBearingChangeKind::ContentEdit => {
                        RealtimeSteering::ContentEdit { preview, verbatim }
                    }
                    agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
                    | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact => {
                        RealtimeSteering::None
                    }
                }
            })
            .collect();
    RealtimeSteeringSet::new(directives)
}

pub fn realtime_steering_between(baseline: &str, current: &str) -> RealtimeSteering {
    realtime_steering_all_between(baseline, current).primary()
}

fn exchange_steering_between(baseline: &str, current: &str) -> RealtimeSteering {
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
    fn realtime_steering_detects_queue_revision_verbatim() {
        let queue = |body: &str| {
            format!(
                "{}<!-- agent:queue -->\n{body}\n<!-- /agent:queue -->\n",
                doc("")
            )
        };
        let baseline = queue("- do [#task] old option");
        let current = queue("- 🚧 do [#task] new option");
        let set = realtime_steering_all_between(&baseline, &current);
        assert_eq!(set.len(), 1);
        assert!(
            set.verbatim_aggregate()
                .unwrap()
                .contains("do [#task] new option")
        );
        assert_eq!(
            realtime_steering_between(&baseline, &current),
            set.primary()
        );
    }

    #[test]
    fn queue_steering_ignores_maintenance_and_retains_multiline_corrections() {
        let queue = |body: &str| {
            format!(
                "{}<!-- agent:queue -->\n{body}\n<!-- /agent:queue -->\n",
                doc("")
            )
        };
        let baseline = queue("- do [#task]\n- Another request");
        for body in [
            "- 🚧 do [#task]\n- Another request",
            "- Another request\n- 📌 do [#task]",
            "- ~~do [#task]~~\n- Another request",
            "- Another request",
            "- do [#task]\n- Another request\n- Independent new request",
        ] {
            assert!(
                !realtime_steering_all_between(&baseline, &queue(body)).is_present(),
                "maintenance: {body}"
            );
        }
        let current = queue(
            "---\nUse the new option.\nKeep every line of this detail.\n---\n- Another request",
        );
        let set = realtime_steering_all_between(&baseline, &current);
        assert_eq!(set.len(), 1);
        assert_eq!(
            set.primary().verbatim(),
            Some("Use the new option.\nKeep every line of this detail.")
        );
        assert_eq!(set, realtime_steering_all_between(&baseline, &current));
    }

    #[test]
    fn queue_and_exchange_steering_are_aggregated_together() {
        let baseline = format!(
            "{}<!-- agent:queue -->\n- Old request\n<!-- /agent:queue -->\n",
            doc("❯ Pending question\n")
        );
        let current = baseline
            .replace("❯ Pending question\n", "")
            .replace("Old request", "Updated request");
        let set = realtime_steering_all_between(&baseline, &current);
        assert_eq!(set.len(), 2);
        assert!(matches!(
            set.directives()[0],
            RealtimeSteering::PromptDeleted { .. }
        ));
        assert_eq!(set.directives()[1].verbatim(), Some("Updated request"));
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
        let current = doc(
            "❯ Fix the JB error:\n```\nRead access is allowed from inside read-action only\n```\n",
        );
        let steering = BaselineComparison::new(&baseline, &current).realtime_steering();
        assert_eq!(steering.label(), Some("prompt_target"));
        let verbatim = steering.verbatim().unwrap();
        assert!(verbatim.contains("Fix the JB error:"));
        assert!(verbatim.contains("Read access is allowed from inside read-action only"));
    }

    #[test]
    fn realtime_steering_all_aggregates_multiple_concurrent_prompt_targets() {
        // `#realtime-steering-aggregate` (plan Phase 6): the operator added TWO
        // prompts while the turn was active. Both must reach the agent at once so it
        // can address them together and find patterns — not just the first, and not
        // drained one head at a time.
        let baseline = doc("");
        let current = doc("❯ First steering directive\n\n❯ Second steering directive\n");

        let set = BaselineComparison::new(&baseline, &current).realtime_steering_all();
        assert!(set.is_present());
        assert_eq!(set.len(), 2, "both concurrent directives must be surfaced");

        let aggregate = set.verbatim_aggregate().unwrap();
        assert!(aggregate.contains("First steering directive"));
        assert!(aggregate.contains("Second steering directive"));
        // A count header tells the agent to look for patterns across them.
        assert!(aggregate.contains("2 concurrent operator steering directives"));

        // The single-directive summary still reports the primary (oldest) one, so
        // label/interrupt paths keep working.
        assert_eq!(set.primary().label(), Some("prompt_target"));

        let projection = set.turn_projection();
        assert_eq!(projection.count, 2);
        assert_eq!(projection.elements.len(), 2);
        assert_eq!(
            projection.preview.as_deref(),
            Some("❯ First steering directive")
        );
        assert_eq!(projection.verbatim.as_deref(), Some(aggregate.as_str()));
    }

    #[test]
    fn replayed_committed_response_cell_is_not_realtime_steering() {
        // `#responsereplaysteering`: a retained-capture replay spliced a committed
        // `### Re:` cell back into the live buffer a second time. Its paragraphs are
        // agent-authored bytes that the write pipeline resolves away, but the
        // steering detector used to read them as new operator prompts — and since
        // answering one only appends another response, the phantom directives never
        // cleared and every later `session-check` INTERRUPTed. The turn could never
        // close.
        let response_cell = concat!(
            "### Re: PR 20 — review addressed — fable\n",
            "\n",
            "Done. haiven-docs PR #20 head is now `283c17b`.\n",
            "\n",
            "Worked in a scratch worktree (`src/.haiven-docs-pr20`, removed after push).\n",
        );
        let baseline = doc(&format!("❯ Address all reviews in PR 20.\n\n{response_cell}"));
        let current = doc(&format!(
            "❯ Address all reviews in PR 20.\n\n{response_cell}\n{response_cell}"
        ));

        let set = BaselineComparison::new(&baseline, &current).realtime_steering_all();
        assert!(
            !set.is_present(),
            "a replayed committed response cell is not operator steering: {:?}",
            set.directives()
        );
    }

    #[test]
    fn replayed_response_cell_does_not_hide_a_concurrent_operator_prompt() {
        // The replay suppression is per-directive: a genuine prompt added in the
        // same edit still reaches the agent.
        let response_cell = concat!(
            "### Re: PR 20 — review addressed — fable\n",
            "\n",
            "Done. haiven-docs PR #20 head is now `283c17b`.\n",
        );
        let baseline = doc(&format!("❯ Address all reviews in PR 20.\n\n{response_cell}"));
        let current = doc(&format!(
            "❯ Address all reviews in PR 20.\n\n{response_cell}\n{response_cell}\n\
             ❯ Since we will be migrating away from Cognito, use the backend-signed shape.\n"
        ));

        let set = BaselineComparison::new(&baseline, &current).realtime_steering_all();
        assert_eq!(set.len(), 1, "{:?}", set.directives());
        assert!(
            set.verbatim_aggregate()
                .unwrap()
                .contains("backend-signed shape"),
            "the genuine concurrent directive must survive: {:?}",
            set.directives()
        );
    }

    #[test]
    fn realtime_steering_identity_set_deduplicates_and_retracts_by_body() {
        let first = RealtimeSteering::PromptTarget {
            preview: "first".into(),
            verbatim: "first\nbody".into(),
        };
        let second = RealtimeSteering::PromptTarget {
            preview: "second".into(),
            verbatim: "second\nbody".into(),
        };
        let duplicate_first = first.clone();
        let first_id = first.identity().unwrap();
        let second_id = second.identity().unwrap();

        let both = RealtimeSteeringSet::new(vec![first, second.clone(), duplicate_first])
            .turn_projection();
        assert_eq!(both.elements.len(), 2);
        assert!(both.elements.contains_key(&first_id));
        assert!(both.elements.contains_key(&second_id));

        let after_retraction = RealtimeSteeringSet::new(vec![second]).turn_projection();
        assert_eq!(after_retraction.elements.len(), 1);
        assert!(!after_retraction.elements.contains_key(&first_id));
        assert!(after_retraction.elements.contains_key(&second_id));
    }

    #[test]
    fn realtime_steering_all_single_directive_matches_single_path() {
        // One added prompt: the aggregate is exactly the single verbatim (no count
        // header), so the two paths agree when there is only one directive.
        let baseline = doc("");
        let current = doc("❯ Only one directive\n");

        let set = BaselineComparison::new(&baseline, &current).realtime_steering_all();
        assert_eq!(set.len(), 1);
        let single = BaselineComparison::new(&baseline, &current).realtime_steering();
        assert_eq!(set.verbatim_aggregate().as_deref(), single.verbatim());
        assert!(
            !set.verbatim_aggregate()
                .unwrap()
                .contains("concurrent operator")
        );
    }

    #[test]
    fn realtime_steering_all_empty_when_unchanged() {
        let baseline = doc("❯ Already here\n");
        let current = doc("❯ Already here\n");
        let set = BaselineComparison::new(&baseline, &current).realtime_steering_all();
        assert!(!set.is_present());
        assert!(set.verbatim_aggregate().is_none());
        assert!(!set.turn_projection().is_present());
    }
}
