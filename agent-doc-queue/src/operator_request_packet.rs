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
            if line_indent > level || (current_key == "choices" && is_legacy_choice_line(field)) {
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
                "queue_item" | "kind" | "blocking" | "request" | "context" | "choices"
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
}
