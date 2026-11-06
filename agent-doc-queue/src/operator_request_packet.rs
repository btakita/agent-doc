//! Structural parser for blocking subagent operator-request packets.
//!
//! Queue response projection and turn closeout share this parser so a summary
//! cannot accidentally omit choices the operator must see.

/// A blocking `[#subagent-operator-request-<unique-id>]` packet a response
/// presents to the operator (`#packettodo`, `#wayposttodo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingOperatorRequestPacket {
    pub id: String,
    pub kind: String,
    pub request: String,
    pub context: Option<String>,
    pub queue_item: Option<String>,
    pub choice_labels: Vec<String>,
    /// Canonical choice content without its source list marker, preserving the
    /// exact emphasized label and explanatory text for response projection.
    pub choice_rows: Vec<String>,
    /// The packet's raw `recommendation:` row, as written. Read it through
    /// [`BlockingOperatorRequestPacket::choice_recommendation`].
    pub recommendation: Option<String>,
}

/// One offered choice a response recommends, and why (`#recommendationinrow`).
///
/// This is read from a specified form and never from prose, so a sentence such
/// as "my recommendation: leave them out" recommends nothing here:
/// - a packet's `recommendation:` row, `**<offered label>**: <reason>`;
/// - a decision record's `**Recommendation:**` field, in the same shape;
/// - the one choice labelled `(Recommended)`, whose description starts
///   `Recommended because <reason>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChoiceRecommendation {
    /// The offered label, without any `(Recommended)` suffix.
    pub label: String,
    pub rationale: String,
}

/// Appended to the recommended row of a `**Choices:**` subtree. A label that
/// already ends `(Recommended)` is marked by its label and gets no second mark.
pub const RECOMMENDED_CHOICE_MARK: &str = " — **Recommended**";

/// The reserved response row that states a recommendation and its reason.
pub const RECOMMENDATION_ROW_LABEL: &str = "**Recommendation:** ";

/// The reserved response row that says what a request's choices refer to.
pub const CONTEXT_ROW_LABEL: &str = "**Context:** ";

impl ChoiceRecommendation {
    /// The dedicated row rendered directly above its `**Choices:**` subtree.
    /// The separator is deliberately not `**: `: that spelling is an offered
    /// choice, and this row must never read as one.
    pub fn row(&self) -> String {
        format!(
            "{RECOMMENDATION_ROW_LABEL}**{}** — {}",
            self.label, self.rationale
        )
    }

    /// Mark the row that offers this recommendation.
    pub fn mark_choice_row(&self, rows: &mut [String]) {
        for row in rows {
            let Some(label) = choice_row_label(row) else {
                continue;
            };
            if same_choice_label(label, &self.label) && !is_recommended_choice_label(label) {
                row.push_str(RECOMMENDED_CHOICE_MARK);
            }
        }
    }
}

impl BlockingOperatorRequestPacket {
    /// The choice this packet recommends. `None` for a packet that recommends
    /// nothing, and for one whose recommendation names no offered choice or
    /// states no reason; those render exactly as they did before.
    pub fn choice_recommendation(&self) -> Option<ChoiceRecommendation> {
        resolve_choice_recommendation(
            self.recommendation.as_deref(),
            &self.choice_labels,
            &self.choice_rows,
        )
    }

    /// The row carrying this packet's context bullets (`#choicecontextinrow`).
    /// It renders above the choices, so a label such as `Use #4` can be
    /// answered from the row. `None` for a packet that states no context.
    pub fn context_row(&self) -> Option<String> {
        self.context
            .as_deref()
            .map(|context| format!("{CONTEXT_ROW_LABEL}{context}"))
    }
}

/// A final, operator-visible answer to a structured choice prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StructuredChoiceDecision {
    pub request_id: Option<String>,
    pub context: Option<String>,
    pub question: Option<String>,
    pub choice_labels: Vec<String>,
    pub selected: String,
    pub selected_label: Option<String>,
    /// The record's raw `**Recommendation:**` field, as written.
    pub recommendation: Option<String>,
}

impl StructuredChoiceDecision {
    /// The recommendation this record states about the choices it presented.
    pub fn choice_recommendation(&self) -> Option<ChoiceRecommendation> {
        resolve_choice_recommendation(self.recommendation.as_deref(), &self.choice_labels, &[])
    }
}

/// A choice label with any `(Recommended)` suffix removed, comparable across
/// the spellings a label takes between a packet and its decision record.
pub(crate) fn normalized_choice_label(label: &str) -> String {
    bare_choice_label(label.trim().trim_end_matches('.')).to_ascii_lowercase()
}

fn bare_choice_label(label: &str) -> &str {
    label
        .trim_end_matches("(recommended)")
        .trim_end_matches("(Recommended)")
        .trim()
}

fn same_choice_label(left: &str, right: &str) -> bool {
    normalized_choice_label(left) == normalized_choice_label(right)
}

fn is_recommended_choice_label(label: &str) -> bool {
    label.trim().to_ascii_lowercase().ends_with("(recommended)")
}

/// The label of a rendered choice row: `**label**: effect` or `label: effect`.
fn choice_row_label(row: &str) -> Option<&str> {
    let row = row.trim();
    let label = match row.strip_prefix("**") {
        Some(bold) => bold.split_once("**:")?.0,
        None => row.split_once(':').map_or(row, |(label, _)| label),
    };
    Some(label.trim()).filter(|label| !label.is_empty())
}

/// `**<label>**: <reason>`, the one spelling a declared recommendation takes.
fn parse_declared_recommendation(row: &str) -> Option<(&str, &str)> {
    let row = row.trim();
    let row = row.strip_prefix("- ").unwrap_or(row).trim_start();
    let (label, rationale) = row.strip_prefix("**")?.split_once("**:")?;
    let (label, rationale) = (label.trim(), rationale.trim());
    (!label.is_empty() && !rationale.is_empty()).then_some((label, rationale))
}

fn resolve_choice_recommendation(
    declared: Option<&str>,
    labels: &[String],
    rows: &[String],
) -> Option<ChoiceRecommendation> {
    if let Some(declared) = declared {
        let (label, rationale) = parse_declared_recommendation(declared)?;
        let offered = labels
            .iter()
            .find(|offered| same_choice_label(offered, label))?;
        return Some(ChoiceRecommendation {
            label: bare_choice_label(offered.trim()).to_string(),
            rationale: rationale.to_string(),
        });
    }
    let mut suffixed = labels
        .iter()
        .filter(|label| is_recommended_choice_label(label));
    let label = suffixed.next()?;
    if suffixed.next().is_some() {
        return None;
    }
    let description = rows
        .iter()
        .find_map(|row| row.strip_prefix(&format!("**{label}**:")))?
        .trim();
    const PREFIX: &str = "recommended because ";
    let rationale = description
        .get(..PREFIX.len())
        .filter(|prefix| prefix.eq_ignore_ascii_case(PREFIX))
        .map(|prefix| description[prefix.len()..].trim())
        .filter(|rationale| !rationale.is_empty())?;
    Some(ChoiceRecommendation {
        label: bare_choice_label(label.trim()).to_string(),
        rationale: rationale.to_string(),
    })
}

/// A `(Recommended)` marker is an operator-facing judgment, not decoration.
/// New choice prompts must state the evidence for that judgment explicitly so
/// the recommendation survives projection without relying on hidden reasoning.
pub fn recommended_choice_rationale_error(label: &str, description: &str) -> Option<String> {
    let label = label.trim();
    let lowercase_label = label.to_ascii_lowercase();
    if !lowercase_label.ends_with("(recommended)") {
        return None;
    }

    let description = description.trim();
    const PREFIX: &str = "recommended because ";
    let lowercase = description.to_ascii_lowercase();
    let Some(rationale) = lowercase.strip_prefix(PREFIX) else {
        return Some(format!(
            "recommended choice `{label}` must start its description with `Recommended because <contextual reason>`"
        ));
    };
    if !rationale_states_a_reason(&lowercase_label, rationale) {
        return Some(format!(
            "recommended choice `{label}` needs a concrete contextual reason, not a tautological recommendation"
        ));
    }
    None
}

/// Whether a lowercased rationale says something beyond its own lowercased
/// label: at least two words that are neither the label nor recommendation
/// filler.
fn rationale_states_a_reason(lowercase_label: &str, rationale: &str) -> bool {
    let choice_words = lowercase_label
        .trim_end_matches("(recommended)")
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        .collect::<std::collections::HashSet<_>>();

    let substantive_words = rationale
        .split(|character: char| !character.is_ascii_alphanumeric())
        .filter(|word| !word.is_empty())
        // Restating the action label is not evidence for preferring it.  A
        // rationale must name an independent tradeoff or current fact.
        .filter(|word| !choice_words.contains(word))
        .filter(|word| {
            !matches!(
                *word,
                "a" | "an"
                    | "and"
                    | "be"
                    | "best"
                    | "better"
                    | "because"
                    | "choice"
                    | "choose"
                    | "chosen"
                    | "it"
                    | "is"
                    | "option"
                    | "prefer"
                    | "preferred"
                    | "recommend"
                    | "recommendation"
                    | "recommended"
                    | "select"
                    | "selected"
                    | "should"
                    | "that"
                    | "the"
                    | "this"
                    | "to"
            )
        })
        .take(2)
        .count();
    substantive_words >= 2
}

/// Validate recommendation rationale in a newly emitted blocking packet.
/// Parsing remains backward-compatible for committed legacy packets; callers
/// use this gate only while accepting a new response.
pub fn blocking_operator_request_recommendation_error(
    packet: &BlockingOperatorRequestPacket,
) -> Option<String> {
    if let Some(error) = declared_recommendation_error(packet) {
        return Some(error);
    }
    packet.choice_labels.iter().find_map(|label| {
        if !label.trim().to_ascii_lowercase().ends_with("(recommended)") {
            return None;
        }
        let prefix = format!("**{label}**:");
        let description = packet
            .choice_rows
            .iter()
            .find_map(|row| row.strip_prefix(&prefix))
            .map(str::trim)
            .unwrap_or_default();
        recommended_choice_rationale_error(label, description)
    })
}

/// A declared recommendation renders only when it is exact, so a malformed one
/// is refused while it is being written instead of vanishing from the row.
fn declared_recommendation_error(packet: &BlockingOperatorRequestPacket) -> Option<String> {
    let declared = packet.recommendation.as_deref()?;
    let Some((label, rationale)) = parse_declared_recommendation(declared) else {
        return Some(format!(
            "`recommendation:` must be one row `**<offered label>**: <contextual reason>`, not `{declared}`"
        ));
    };
    if !packet
        .choice_labels
        .iter()
        .any(|offered| same_choice_label(offered, label))
    {
        return Some(format!(
            "recommendation `{label}` must name an offered choice, spelled as its label"
        ));
    }
    if let Some(other) = packet
        .choice_labels
        .iter()
        .find(|offered| is_recommended_choice_label(offered) && !same_choice_label(offered, label))
    {
        return Some(format!(
            "recommendation `{label}` contradicts the choice labelled `{other}`"
        ));
    }
    let lowercase_label = label.to_ascii_lowercase();
    (!rationale_states_a_reason(&lowercase_label, &rationale.to_ascii_lowercase())).then(|| {
        format!(
            "recommendation `{label}` needs a concrete contextual reason, not a tautological recommendation"
        )
    })
}

/// Parse final `**Decision record**` blocks emitted after a terminal or
/// sequential operator choice. Examples inside fenced code are ignored.
pub fn structured_choice_decisions(response: &str) -> Vec<StructuredChoiceDecision> {
    let lines = response.lines().collect::<Vec<_>>();
    let mut decisions = Vec::new();
    let mut fence = None;
    let mut index = 0usize;
    while index < lines.len() {
        let line = lines[index];
        let trimmed = line.trim();
        if let Some(open) = fence {
            if is_closing_fence(trimmed, open) {
                fence = None;
            }
            index += 1;
            continue;
        }
        if let Some(open) = opening_fence(trimmed) {
            fence = Some(open);
            index += 1;
            continue;
        }
        if !trimmed.starts_with("**Decision record**") {
            index += 1;
            continue;
        }

        let request_id = decision_request_id(trimmed).map(str::to_string);
        let mut context = None;
        let mut question = None;
        let mut choice_labels = Vec::new();
        let mut selected = None;
        let mut recommendation = None;
        index += 1;
        while index < lines.len() {
            let field = lines[index].trim();
            if field.is_empty() {
                index += 1;
                if selected.is_some() {
                    break;
                }
                continue;
            }
            if field.starts_with('#') || field.starts_with("**Decision record**") {
                break;
            }
            let Some((key, value)) = operator_request_field(field) else {
                index += 1;
                continue;
            };
            match key.to_ascii_lowercase().as_str() {
                "context" => context = (!value.is_empty()).then(|| value.to_string()),
                "question" => question = (!value.is_empty()).then(|| value.to_string()),
                "choices presented" => {
                    choice_labels = value
                        .trim_end_matches('.')
                        .split(',')
                        .map(str::trim)
                        .filter(|label| !label.is_empty())
                        .map(ToString::to_string)
                        .collect();
                }
                "selected" => selected = (!value.is_empty()).then(|| value.to_string()),
                "recommendation" => {
                    recommendation = (!value.is_empty()).then(|| value.to_string());
                }
                _ => {}
            }
            index += 1;
        }
        let Some(selected) = selected else {
            continue;
        };
        let selected_label = selected
            .strip_prefix("**")
            .and_then(|value| value.split_once("**"))
            .map(|(label, _)| label.trim().to_string())
            .filter(|label| !label.is_empty());
        decisions.push(StructuredChoiceDecision {
            request_id,
            context,
            question,
            choice_labels,
            selected,
            selected_label,
            recommendation,
        });
    }
    decisions
}

fn decision_request_id(line: &str) -> Option<&str> {
    let start = line.find("[#subagent-operator-request-")? + 2;
    let tail = &line[start..];
    let end = tail.find(']')?;
    let id = &tail[..end];
    let suffix = id.strip_prefix("subagent-operator-request-")?;
    (!suffix.is_empty()
        && suffix
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '-'))
    .then_some(id)
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ParsedBlockingOperatorRequestPacket {
    packet: BlockingOperatorRequestPacket,
    start_line: usize,
    end_line: usize,
}

/// Every structurally valid blocking operator-request packet in `response`.
pub fn blocking_operator_request_packets(response: &str) -> Vec<BlockingOperatorRequestPacket> {
    parsed_blocking_operator_request_packets(response)
        .into_iter()
        .map(|parsed| parsed.packet)
        .collect()
}

/// Exact source blocks for the packets accepted by
/// [`blocking_operator_request_packets`].
///
/// The complete packet is retained so its request and every offered choice are
/// copied byte-for-byte into bounded response summaries.
pub fn blocking_operator_request_blocks(response: &str) -> Vec<&str> {
    let mut line_starts = vec![0usize];
    for line in response.split_inclusive('\n') {
        line_starts.push(line_starts.last().copied().unwrap_or_default() + line.len());
    }
    let response_len = response.len();
    parsed_blocking_operator_request_packets(response)
        .into_iter()
        .map(|parsed| {
            let start = line_starts
                .get(parsed.start_line)
                .copied()
                .unwrap_or(response_len);
            let end = line_starts
                .get(parsed.end_line)
                .copied()
                .unwrap_or(response_len);
            response[start..end].trim_end_matches(['\r', '\n'])
        })
        .collect()
}

fn parsed_blocking_operator_request_packets(
    response: &str,
) -> Vec<ParsedBlockingOperatorRequestPacket> {
    let lines: Vec<&str> = response.lines().collect();
    let mut packets = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut index = 0;
    while index < lines.len() {
        let marker_index = index;
        let line = lines[index];
        index += 1;
        let trimmed_start = line.trim_start();
        if let Some(open) = fence {
            if is_closing_fence(trimmed_start, open) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = opening_fence(trimmed_start) {
            fence = Some(open);
            continue;
        }
        let indent = line.len() - trimmed_start.len();
        let Some(packet_id) = (indent <= 3)
            .then(|| crate::subagent_brief::subagent_operator_request_id(line))
            .flatten()
        else {
            continue;
        };
        let mut kind = None;
        let mut blocking = None;
        let mut request: Option<String> = None;
        let mut context: Option<String> = None;
        let mut queue_item = None;
        let mut choice_labels = Vec::new();
        let mut choice_rows = Vec::new();
        let mut recommendation = None;
        let mut field_indent: Option<usize> = None;
        let mut current_key = String::new();
        let mut choice_indent: Option<usize> = None;
        while index < lines.len() {
            let raw = lines[index];
            let field = raw.trim();
            if field.is_empty()
                || crate::subagent_brief::subagent_operator_request_id(field).is_some()
            {
                break;
            }
            let line_indent = raw.len() - raw.trim_start().len();
            let level = *field_indent.get_or_insert(line_indent);
            if line_indent < level {
                break;
            }
            index += 1;
            if line_indent > level
                || (matches!(current_key.as_str(), "choices" | "recommendation")
                    && is_legacy_choice_line(field))
            {
                match current_key.as_str() {
                    "choices" => {
                        let shallowest = *choice_indent.get_or_insert(line_indent);
                        if line_indent <= shallowest
                            && let Some(label) = operator_request_choice_label(field)
                        {
                            choice_labels.push(label.to_string());
                            if let Some(row) = canonical_operator_request_choice_row(field, label) {
                                choice_rows.push(row);
                            }
                        }
                    }
                    "request" => append_nested_value(request.as_mut(), field),
                    "context" => append_nested_value(context.as_mut(), field),
                    "queue_item" => {
                        append_nested_value(Some(queue_item.get_or_insert_with(String::new)), field)
                    }
                    "recommendation" => append_nested_value(
                        Some(recommendation.get_or_insert_with(String::new)),
                        field,
                    ),
                    _ => {}
                }
                continue;
            }
            current_key.clear();
            choice_indent = None;
            let Some((key, value)) = operator_request_field(field) else {
                continue;
            };
            let key = key.to_ascii_lowercase();
            match key.as_str() {
                "kind" => kind = Some(value.to_ascii_lowercase()),
                "blocking" => blocking = Some(value.eq_ignore_ascii_case("true")),
                "request" => request = Some(value.to_string()),
                "context" => context = Some(value.to_string()),
                "queue_item" => queue_item = (!value.is_empty()).then(|| value.to_string()),
                "recommendation" => {
                    recommendation = (!value.is_empty()).then(|| value.to_string());
                }
                _ => {}
            }
            if value.is_empty() {
                current_key = key;
            }
        }
        let (Some(kind), Some(true), Some(request)) = (kind, blocking, request) else {
            continue;
        };
        if !matches!(kind.as_str(), "question" | "permission")
            || request.is_empty()
            || (request.starts_with('<') && request.ends_with('>'))
        {
            continue;
        }
        packets.push(ParsedBlockingOperatorRequestPacket {
            packet: BlockingOperatorRequestPacket {
                id: packet_id.to_string(),
                kind,
                request,
                context: context.filter(|value| !value.is_empty()),
                queue_item: queue_item.filter(|item| !item.is_empty()),
                choice_labels,
                choice_rows,
                recommendation,
            },
            start_line: marker_index,
            end_line: index,
        });
    }
    packets
}

fn append_nested_value(value: Option<&mut String>, field: &str) {
    let Some(value) = value else {
        return;
    };
    let text = field.strip_prefix("- ").unwrap_or(field).trim();
    if text.is_empty() {
        return;
    }
    if !value.is_empty() {
        value.push(' ');
    }
    value.push_str(text);
}

fn is_legacy_choice_line(field: &str) -> bool {
    field.starts_with("- ")
        && !operator_request_field(field).is_some_and(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "queue_item"
                    | "kind"
                    | "blocking"
                    | "request"
                    | "context"
                    | "choices"
                    | "recommendation"
            )
        })
}

fn operator_request_choice_label(field: &str) -> Option<&str> {
    let choice = field.strip_prefix("- ")?;
    let label = choice
        .split_once(':')
        .map_or(choice, |(label, _)| label)
        .trim();
    let label = label
        .strip_prefix("**")
        .and_then(|label| label.strip_suffix("**"))
        .unwrap_or(label)
        .trim();
    (!label.is_empty()).then_some(label)
}

fn canonical_operator_request_choice_row(field: &str, label: &str) -> Option<String> {
    let choice = field.strip_prefix("- ")?.trim();
    let (_, effect) = choice.split_once(':')?;
    let effect = effect.trim();
    (!effect.is_empty()).then(|| format!("**{label}**: {effect}"))
}

fn operator_request_field(field: &str) -> Option<(&str, &str)> {
    let field = field.trim().strip_prefix("- ").unwrap_or(field.trim());
    for (prefix, delimiter) in [("**", ":**"), ("**", "**:"), ("*", ":*"), ("*", "*:")] {
        if let Some(emphasized) = field.strip_prefix(prefix)
            && let Some((key, value)) = emphasized.split_once(delimiter)
        {
            return Some((key.trim(), value.trim()));
        }
    }
    let (key, value) = field.split_once(':')?;
    Some((key.trim(), value.trim()))
}

fn opening_fence(trimmed_start: &str) -> Option<(char, usize)> {
    let ch = trimmed_start.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let run = trimmed_start.chars().take_while(|c| *c == ch).count();
    (run >= 3).then_some((ch, run))
}

fn is_closing_fence(trimmed_start: &str, (ch, run): (char, usize)) -> bool {
    let closing = trimmed_start.chars().take_while(|c| *c == ch).count();
    closing >= run && trimmed_start[closing * ch.len_utf8()..].trim().is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_blocks_share_the_structural_packet_contract() {
        let response = concat!(
            "Intro.\n\n",
            "[#subagent-operator-request-release] subagent operator request\n",
            "- *queue_item:*\n  - deploy\n",
            "- *kind:* question\n",
            "- *blocking:* true\n",
            "- *request:*\n  - Which release?\n",
            "- *context:*\n  - Need a target.\n",
            "- *choices:*\n  - **staging**: Release staging.\n  - **production**: Release production.\n",
            "\nRecommendation: staging.\n",
        );
        let packets = blocking_operator_request_packets(response);
        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0].choice_labels, ["staging", "production"]);
        assert_eq!(packets[0].context.as_deref(), Some("Need a target."));
        assert_eq!(
            packets[0].choice_rows,
            [
                "**staging**: Release staging.",
                "**production**: Release production."
            ]
        );
        let blocks = blocking_operator_request_blocks(response);
        assert_eq!(blocks.len(), 1);
        assert!(blocks[0].starts_with("[#subagent-operator-request-release]"));
        assert!(blocks[0].ends_with("**production**: Release production."));
    }

    #[test]
    fn parses_terminal_choice_decision_with_selected_label() {
        let response = concat!(
            "**Decision record** for `[#subagent-operator-request-release-target]`:\n",
            "- **Context:** A release target is required.\n",
            "- **Question:** where should it be published?\n",
            "- **Choices presented:** staging (Recommended), production.\n",
            "- **Selected:** **staging**. Publish the verified build there.\n",
        );

        let decisions = structured_choice_decisions(response);
        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].request_id.as_deref(),
            Some("subagent-operator-request-release-target")
        );
        assert_eq!(decisions[0].selected_label.as_deref(), Some("staging"));
        assert_eq!(
            decisions[0].choice_labels,
            ["staging (Recommended)", "production"]
        );
    }

    #[test]
    fn ignores_decision_record_examples_in_fences() {
        let response = "```md\n**Decision record**:\n- **Selected:** **staging**.\n```\n";
        assert!(structured_choice_decisions(response).is_empty());
    }

    #[test]
    fn recommended_choice_requires_an_explicit_contextual_reason() {
        assert!(recommended_choice_rationale_error(
            "Leave uncommitted (Recommended)",
            "Recommended because the integration regression remains unresolved; keeping the worktree intact preserves the pending fix."
        )
        .is_none());
        assert!(
            recommended_choice_rationale_error(
                "Leave uncommitted (Recommended)",
                "Keep the worktree available before committing."
            )
            .unwrap()
            .contains("must start")
        );
        assert!(
            recommended_choice_rationale_error(
                "Leave uncommitted (Recommended)",
                "Recommended because it is the recommended choice."
            )
            .unwrap()
            .contains("tautological")
        );
        assert!(
            recommended_choice_rationale_error(
                "Leave uncommitted (Recommended)",
                "Recommended because leave uncommitted."
            )
            .unwrap()
            .contains("tautological")
        );
        assert!(
            recommended_choice_rationale_error("Commit now", "Commit the verified patch.")
                .is_none()
        );
    }

    #[test]
    fn blocking_packet_exposes_missing_recommendation_rationale_without_rejecting_parse() {
        let response = concat!(
            "[#subagent-operator-request-disposition] subagent operator request\n",
            "- *kind:* question\n",
            "- *blocking:* true\n",
            "- *request:*\n  - How should the worktree be handled?\n",
            "- *choices:*\n",
            "  - **Leave uncommitted (Recommended)**: Keep the worktree available.\n",
            "  - **Commit now**: Commit the verified patch.\n",
        );
        let packets = blocking_operator_request_packets(response);
        assert_eq!(packets.len(), 1, "legacy packets remain readable");
        assert!(
            blocking_operator_request_recommendation_error(&packets[0])
                .unwrap()
                .contains("must start")
        );
    }

    /// The `#completioncellrestore` packet, with its recommendation in the
    /// specified form instead of in prose beside the packet.
    const RECOMMENDED_PACKET: &str = concat!(
        "[#subagent-operator-request-closedcellkeys] subagent operator request\n",
        "- *queue_item:*\n  - do [#completioncellrestore]\n",
        "- *kind:* question\n",
        "- *blocking:* true\n",
        "- *request:*\n  - Should the 22 closed attempts be restored?\n",
        "- *recommendation:*\n",
        "  - **Leave them out**: The worker responses stay in the saved receipts, and the next repair removes restored cells again.\n",
        "- *choices:*\n",
        "  - **Restore the 22**: after install, run `--apply` with those keys.\n",
        "  - **Leave them out**: apply nothing; the responses stay in the receipts.\n",
    );

    fn only_packet(response: &str) -> BlockingOperatorRequestPacket {
        let mut packets = blocking_operator_request_packets(response);
        assert_eq!(packets.len(), 1, "{response}");
        packets.remove(0)
    }

    #[test]
    fn recommendation_row_names_its_offered_label_and_reason() {
        let packet = only_packet(RECOMMENDED_PACKET);
        assert_eq!(packet.choice_labels, ["Restore the 22", "Leave them out"]);
        let recommendation = packet.choice_recommendation().unwrap();
        assert_eq!(recommendation.label, "Leave them out");
        assert_eq!(
            recommendation.row(),
            "**Recommendation:** **Leave them out** — The worker responses stay in the saved receipts, and the next repair removes restored cells again."
        );
        assert_eq!(
            blocking_operator_request_recommendation_error(&packet),
            None
        );

        let mut rows = packet.choice_rows.clone();
        recommendation.mark_choice_row(&mut rows);
        assert_eq!(
            rows,
            [
                "**Restore the 22**: after install, run `--apply` with those keys.",
                "**Leave them out**: apply nothing; the responses stay in the receipts. — **Recommended**",
            ]
        );
    }

    #[test]
    fn a_recommendation_written_after_choices_is_never_an_offered_choice() {
        let row = "- *recommendation:*\n  - **Leave them out**: The saved receipts keep every response.\n";
        let response = RECOMMENDED_PACKET.replace(
            "- *recommendation:*\n  - **Leave them out**: The worker responses stay in the saved receipts, and the next repair removes restored cells again.\n",
            "",
        ) + row;
        let packet = only_packet(&response);
        assert_eq!(packet.choice_labels, ["Restore the 22", "Leave them out"]);
        assert_eq!(
            packet.choice_recommendation().unwrap().rationale,
            "The saved receipts keep every response."
        );

        // The flush legacy choice layout reaches the same answer.
        let flush = response.replace("\n  - **", "\n- **");
        let packet = only_packet(&flush);
        assert_eq!(
            packet.choice_recommendation().map(|found| found.label),
            Some("Leave them out".to_string()),
            "{flush}"
        );
    }

    #[test]
    fn prose_never_recommends_a_choice() {
        // The defect's own shape: a judgment beside the packet is not a form.
        let response = format!(
            "**My recommendation: leave them out.** The receipts keep them.\n\n{}\n**Recommendation:** Leave them out.\n",
            RECOMMENDED_PACKET.replace(
                "- *recommendation:*\n  - **Leave them out**: The worker responses stay in the saved receipts, and the next repair removes restored cells again.\n",
                "",
            )
        );
        let packet = only_packet(&response);
        assert_eq!(packet.recommendation, None);
        assert_eq!(packet.choice_recommendation(), None);
        assert_eq!(
            blocking_operator_request_recommendation_error(&packet),
            None
        );
    }

    #[test]
    fn recommended_label_yields_the_same_recommendation() {
        let packet = only_packet(concat!(
            "[#subagent-operator-request-a] subagent operator request\n",
            "- *kind:* question\n- *blocking:* true\n- *request:*\n  - Which?\n",
            "- *choices:*\n",
            "  - **Keep compatibility (Recommended)**: Recommended because existing clients still send the old format.\n",
            "  - **Switch now**: Require the new format.\n",
        ));
        let recommendation = packet.choice_recommendation().unwrap();
        assert_eq!(recommendation.label, "Keep compatibility");
        assert_eq!(
            recommendation.rationale,
            "existing clients still send the old format."
        );

        // The label already marks the row; a second mark would only repeat it.
        let mut rows = packet.choice_rows.clone();
        recommendation.mark_choice_row(&mut rows);
        assert_eq!(rows, packet.choice_rows);
    }

    #[test]
    fn a_recommended_label_without_a_stated_reason_has_nothing_to_render() {
        for choices in [
            // A legacy packet: the label stays as its own mark, with no row.
            "  - **staging (Recommended)**: Publish to staging.\n  - **production**: Publish.\n",
            // Two recommended labels name no single recommendation.
            "  - **a (Recommended)**: Recommended because the first evidence holds.\n  - **b (Recommended)**: Recommended because the second evidence holds.\n",
        ] {
            let packet = only_packet(&format!(
                "[#subagent-operator-request-a] subagent operator request\n\
                 - *kind:* question\n- *blocking:* true\n- *request:*\n  - Which?\n\
                 - *choices:*\n{choices}"
            ));
            assert_eq!(packet.choice_recommendation(), None, "{choices}");
        }
    }

    #[test]
    fn invalid_recommendation_is_refused_while_written_and_silent_when_replayed() {
        let declared = "  - **Leave them out**: The worker responses stay in the saved receipts, and the next repair removes restored cells again.\n";
        for (row, expected) in [
            (
                "  - **Restore everything**: The saved receipts would be complete.\n",
                "must name an offered choice",
            ),
            (
                "  - Leave them out, the receipts keep them.\n",
                "must be one row",
            ),
            (
                "  - **Leave them out**: it is the recommended choice.\n",
                "concrete contextual reason",
            ),
            (
                "  - **Leave them out**: leave them out.\n",
                "concrete contextual reason",
            ),
        ] {
            let packet = only_packet(&RECOMMENDED_PACKET.replace(declared, row));
            let error = blocking_operator_request_recommendation_error(&packet)
                .unwrap_or_else(|| panic!("{row} must be refused"));
            assert!(error.contains(expected), "{error}");
            assert_eq!(
                packet.choice_recommendation().is_some(),
                row.contains("it is the") || row.contains("leave them out."),
                "{row}"
            );
        }

        let contradiction = RECOMMENDED_PACKET.replace(
            "**Restore the 22**: after install",
            "**Restore the 22 (Recommended)**: Recommended because the operator asked for every cell; after install",
        );
        let error =
            blocking_operator_request_recommendation_error(&only_packet(&contradiction)).unwrap();
        assert!(error.contains("contradicts"), "{error}");
    }

    #[test]
    fn decision_record_carries_a_recommendation_of_its_own() {
        let decisions = structured_choice_decisions(concat!(
            "**Decision record**:\n",
            "- **Question:** where should it be published?\n",
            "- **Choices presented:** staging, production.\n",
            "- **Recommendation:** **staging**: The production window is closed until Monday.\n",
            "- **Selected:** **production**. Ship it.\n",
        ));
        assert_eq!(decisions.len(), 1);
        assert_eq!(
            decisions[0].choice_recommendation().unwrap().row(),
            "**Recommendation:** **staging** — The production window is closed until Monday."
        );
        assert_eq!(decisions[0].selected_label.as_deref(), Some("production"));
    }
}
