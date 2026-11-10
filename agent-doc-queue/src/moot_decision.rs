//! Remove a reported-closeout operator decision once it no longer needs an
//! answer (`#mootdecision`).
//!
//! A decision stays while its id is in the live set: a current `Reported`
//! claim whose closeout still asks the operator. Every other
//! `subagent-operator-request-reportedcloseout-` decision is moot. The open
//! todo, the queue-row request annotation, the `**Request**` row, the packet
//! opener, and a generated waiting item with no operator text all go. A
//! decision that still needs an answer, any other operator request, a local
//! answer, a checked todo, and the underlying work item stay. An operator
//! line under a waiting item stays with that item so the answer is not
//! destroyed.

use std::collections::HashSet;

use agent_doc_document::queue_projection::is_reserved_operator_action_id;
use agent_doc_element::element::find_code_ranges;
use agent_doc_element::todo::{parse_todo_line, remove_todo_item_from_document};

use crate::subagent_brief::subagent_operator_request_id;
use crate::waiting_archive::segment_has_operator_rows;

/// Id prefix of a reported-closeout operator request.
pub const REPORTED_CLOSEOUT_REQUEST_PREFIX: &str = "subagent-operator-request-reportedcloseout-";

const OPERATOR_ACTION_GLYPH: &str = "🙋";
const WAITING_ITEM_MARKER: &str = "<!-- agent-doc:waiting-item -->";

/// Prune moot reported-closeout decisions from `document`.
///
/// `live_request_ids` are the requests that still need an operator answer.
/// Returns `None` when the document is already quiet, so a second call is a
/// no-op.
pub fn prune_moot_reported_closeout_decisions(
    document: &str,
    live_request_ids: &HashSet<String>,
) -> Option<String> {
    let live = live_request_ids
        .iter()
        .map(|id| normalize_id(id))
        .collect::<HashSet<_>>();
    let without_todos = remove_open_moot_todos(document, &live);
    let rewritten = rewrite_decision_surfaces(&without_todos, &live);
    (rewritten != document).then_some(rewritten)
}

fn normalize_id(id: &str) -> String {
    id.trim()
        .trim_start_matches(['[', '#'])
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

fn is_reported_closeout(id: &str) -> bool {
    normalize_id(id).starts_with(REPORTED_CLOSEOUT_REQUEST_PREFIX)
}

fn is_moot(id: &str, live: &HashSet<String>) -> bool {
    let id = normalize_id(id);
    is_reported_closeout(&id) && !live.contains(&id)
}

fn remove_open_moot_todos(document: &str, live: &HashSet<String>) -> String {
    let mut document = document.to_string();
    while let Some(id) = next_open_moot_todo(&document, live) {
        let Ok(outcome) = remove_todo_item_from_document(&document, &id) else {
            break;
        };
        if outcome.content == document {
            break;
        }
        document = outcome.content;
    }
    document
}

fn next_open_moot_todo(document: &str, live: &HashSet<String>) -> Option<String> {
    let component = agent_doc_element::todo::find_todo_component(document).ok()??;
    for line in component.content(document).lines() {
        let Some(item) = parse_todo_line(line) else {
            continue;
        };
        let Some(id) = item.own_id else {
            continue;
        };
        if item.state.is_open() && is_moot(id, live) {
            return Some(id.to_string());
        }
    }
    None
}

fn rewrite_decision_surfaces(document: &str, live: &HashSet<String>) -> String {
    let lines: Vec<String> = document.split_inclusive('\n').map(str::to_string).collect();
    let mut drop = vec![false; lines.len()];
    let mut rewritten = vec![None; lines.len()];
    let mut in_fence: Option<char> = None;
    let mut index = 0;
    while index < lines.len() {
        let core = line_core(&lines[index]);
        if let Some(open) = in_fence {
            if closes_fence(core, open) {
                in_fence = None;
            }
            index += 1;
            continue;
        }
        if let Some(open) = opens_fence(core) {
            in_fence = Some(open);
            index += 1;
            continue;
        }
        if let Some(id) = packet_opener_id(core) {
            if indent_of(core) <= 3 && is_moot(id, live) {
                let newline = newline_suffix(&lines[index]);
                let indent = &core[..indent_of(core)];
                rewritten[index] = Some(format!(
                    "{indent}~~{} subagent operator request (moot)~~{newline}",
                    normalize_id(id)
                ));
            }
            index += 1;
            continue;
        }
        if let Some(id) = request_line_id(core) {
            if is_moot(id, live) {
                drop[index] = true;
            }
            index += 1;
            continue;
        }
        if core.contains(WAITING_ITEM_MARKER)
            && let Some(id) = waiting_head_id(core)
            && is_moot(&id, live)
        {
            let end = subtree_end(&lines, index);
            let raw = lines[index..end].concat();
            if !segment_has_operator_rows(&raw) {
                drop[index..end].fill(true);
                index = end;
                continue;
            }
        }
        if list_indent(core).is_some() && parse_todo_line(core).is_none() {
            let stripped = strip_trailing_moot_annotations(core, live);
            if stripped != core {
                rewritten[index] = Some(format!("{stripped}{}", newline_suffix(&lines[index])));
            }
        }
        index += 1;
    }
    let kept = lines
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !drop[*index])
        .map(|(index, line)| rewritten[index].take().unwrap_or(line))
        .collect::<Vec<_>>();
    drop_empty_choice_headers(kept).concat()
}

fn line_core(line: &str) -> &str {
    line.trim_end_matches(['\n', '\r'])
}

fn newline_suffix(line: &str) -> &str {
    &line[line_core(line).len()..]
}

fn indent_of(line: &str) -> usize {
    let core = line_core(line);
    core.len() - core.trim_start().len()
}

fn list_indent(line: &str) -> Option<usize> {
    let core = line_core(line);
    let trimmed = core.trim_start_matches([' ', '\t']);
    (trimmed.starts_with("- ") || trimmed.starts_with("* ") || trimmed.starts_with("+ "))
        .then(|| core.len() - trimmed.len())
}

fn bullet_body(line: &str) -> &str {
    let trimmed = line_core(line).trim_start();
    trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
        .unwrap_or(trimmed)
}

fn opens_fence(line: &str) -> Option<char> {
    let trimmed = line_core(line).trim_start();
    let marker = trimmed.chars().next()?;
    if marker != '`' && marker != '~' {
        return None;
    }
    let count = trimmed.chars().take_while(|ch| *ch == marker).count();
    (count >= 3).then_some(marker)
}

fn closes_fence(line: &str, open: char) -> bool {
    let trimmed = line_core(line).trim_start();
    trimmed.starts_with(open) && trimmed.chars().take_while(|ch| *ch == open).count() >= 3
}

fn packet_opener_id(line: &str) -> Option<&str> {
    let id = subagent_operator_request_id(line)?;
    is_reported_closeout(id).then_some(id)
}

fn request_line_id(line: &str) -> Option<&str> {
    let rest = bullet_body(line)
        .trim()
        .strip_prefix("**Request**:")?
        .trim();
    let id = rest.strip_prefix("[#")?.strip_suffix(']')?;
    valid_annotation_id(id).then_some(id)
}

fn waiting_head_id(line: &str) -> Option<String> {
    let body = bullet_body(line);
    let work = agent_doc_element_queue::struck_work_text(body);
    let work = crate::waiting_status::without_status_token(work);
    let work = work.trim().trim_start_matches("~~");
    let work = agent_doc_element_queue::strip_attempt_projection_prefix(work);
    crate::document_queue::queue_prompt_reference_ids(work)
        .into_iter()
        .find(|id| is_reported_closeout(id))
}

fn subtree_end(lines: &[String], head: usize) -> usize {
    let Some(head_indent) = list_indent(&lines[head]) else {
        return head + 1;
    };
    let mut end = head + 1;
    while end < lines.len() {
        let core = line_core(&lines[end]);
        if core.trim().is_empty() {
            end += 1;
            continue;
        }
        if opens_fence(core).is_some() {
            break;
        }
        if list_indent(core).is_some_and(|indent| indent <= head_indent) {
            break;
        }
        end += 1;
    }
    while end > head + 1 && line_core(&lines[end - 1]).trim().is_empty() {
        end -= 1;
    }
    end
}

fn valid_annotation_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    first.is_ascii_alphanumeric() && chars.all(|ch| ch.is_ascii_alphanumeric() || ch == '-')
}

fn trailing_annotation<'a>(
    text: &'a str,
    code: &[(usize, usize)],
) -> Option<(&'a str, &'a str, usize)> {
    let mut search_end = text.len();
    while let Some(rel) = text[..search_end].rfind("[#") {
        let in_code = code.iter().any(|(start, end)| rel >= *start && rel < *end);
        let preceded_by_space = rel == 0
            || text[..rel]
                .chars()
                .next_back()
                .is_some_and(char::is_whitespace);
        let parsed = (!in_code && preceded_by_space)
            .then(|| text[rel + 2..].split_once(']'))
            .flatten()
            .filter(|(id, _)| valid_annotation_id(id));
        if let Some((id, after)) = parsed {
            return Some((id, after, rel));
        }
        if rel == 0 {
            break;
        }
        search_end = rel;
    }
    None
}

fn strip_trailing_moot_annotations(line: &str, live: &HashSet<String>) -> String {
    let code = find_code_ranges(line);
    let trim_end = line.trim_end().len();
    let trailing_ws = &line[trim_end..];
    let mut scan_end = trim_end;
    let mut answer: Option<(usize, usize)> = None;
    if let Some((id, after, start)) = trailing_annotation(&line[..scan_end], &code)
        && is_reserved_operator_action_id(id)
        && after
            .trim_start()
            .strip_prefix(':')
            .is_some_and(|answer| !answer.trim().is_empty())
    {
        answer = Some((start, scan_end));
        scan_end = line[..start].trim_end().len();
    }
    let mut kept_reversed = Vec::new();
    let mut dropped = false;
    while let Some((id, after, start)) = trailing_annotation(&line[..scan_end], &code) {
        if !after.is_empty() {
            break;
        }
        if is_moot(id, live) {
            dropped = true;
        } else {
            kept_reversed.push(format!("[#{id}]"));
        }
        scan_end = line[..start].trim_end().len();
    }
    if !dropped {
        return line.to_string();
    }
    kept_reversed.reverse();
    let mut rebuilt = line[..scan_end].to_string();
    for token in kept_reversed {
        if !rebuilt.is_empty() && !rebuilt.ends_with([' ', '\t']) {
            rebuilt.push(' ');
        }
        rebuilt.push_str(&token);
    }
    if let Some((start, end)) = answer {
        if !rebuilt.is_empty() && !rebuilt.ends_with([' ', '\t']) {
            rebuilt.push(' ');
        }
        rebuilt.push_str(line[start..end].trim_start());
    }
    if !line_has_reserved_request(&rebuilt) {
        rebuilt = strip_operator_glyphs(&rebuilt);
    }
    rebuilt.push_str(trailing_ws);
    rebuilt
}

fn line_has_reserved_request(line: &str) -> bool {
    let code = find_code_ranges(line);
    let mut offset = 0;
    while let Some(rel) = line[offset..].find("[#") {
        let abs = offset + rel;
        let after = &line[abs + 2..];
        if let Some(end) = after.find(']') {
            let id = &after[..end];
            let in_code = code
                .iter()
                .any(|(start, stop)| abs >= *start && abs < *stop);
            if !in_code && is_reserved_operator_action_id(id) {
                return true;
            }
        }
        offset = abs + 2;
        if offset > line.len() {
            break;
        }
    }
    false
}

fn strip_operator_glyphs(line: &str) -> String {
    let code = find_code_ranges(line);
    let mut out = String::new();
    let mut rest = line;
    while let Some(at) = rest.find(OPERATOR_ACTION_GLYPH) {
        let abs = line.len() - rest.len() + at;
        let in_code = code.iter().any(|(start, end)| abs >= *start && abs < *end);
        if in_code {
            let next = at + OPERATOR_ACTION_GLYPH.len();
            out.push_str(&rest[..next]);
            rest = &rest[next..];
            continue;
        }
        out.push_str(&rest[..at]);
        rest = &rest[at + OPERATOR_ACTION_GLYPH.len()..];
        if out.ends_with([' ', '\t']) && rest.starts_with([' ', '\t']) {
            rest = rest.trim_start();
        }
    }
    out.push_str(rest);
    out
}

fn is_choices_header(line: &str) -> bool {
    bullet_body(line).trim() == "**Choices:**"
}

fn drop_empty_choice_headers(lines: Vec<String>) -> Vec<String> {
    let mut drop = vec![false; lines.len()];
    for (index, line) in lines.iter().enumerate() {
        if !is_choices_header(line) {
            continue;
        }
        let indent = indent_of(line);
        let has_child = lines[index + 1..]
            .iter()
            .map(|line| line_core(line))
            .find(|line| !line.trim().is_empty())
            .is_some_and(|next| indent_of(next) > indent && list_indent(next).is_some());
        if !has_child {
            drop[index] = true;
        }
    }
    lines
        .into_iter()
        .enumerate()
        .filter(|(index, _)| !drop[*index])
        .map(|(_, line)| line)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::operator_request_packet::blocking_operator_request_packets;

    const LIVE: &str = "subagent-operator-request-reportedcloseout-live111";
    const MOOT: &str = "subagent-operator-request-reportedcloseout-moot222";
    const PORT: &str = "subagent-operator-request-port";

    fn fixture() -> String {
        format!(
            "\
## Queue

<!-- agent:queue -->
- ### 🙋☑️ do [#livework] [#{LIVE}]
  - **State:** Waiting on operator input.
  - **Guidance:** More work remains. Keep this tree and answer the operator request below.
  - **Choices:**
    - **Request**: [#{LIVE}]
    - **Use the recorded sink**: keep the row until that ref is named.
- ### 🙋☑️ do [#mootwork] [#{MOOT}]
  - **State:** Waiting on operator input.
  - **Guidance:** More work remains. Keep this tree and answer the operator request below.
  - **Choices:**
    - **Request**: [#{MOOT}]
- ### 🙋☑️ do [#shared] [#{MOOT}] [#{LIVE}]
- ### 🙋☑️ do [#{MOOT}] [#latework]
- ### 🙋 [#{MOOT}]: keep the west cluster
- ### 🙋 [#{PORT}] Choose the port
- ### Waiting on you — answer by adding a child line under an item <!-- agent-doc:waiting-on-you -->
  - ⟦you: 1 · agent: 0 · done: 0⟧ [#{MOOT}] Name which lineage attempt is the report. <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item ▫️
  - ⟦you: 1 · agent: 0 · done: 0⟧ [#{LIVE}] Which sink ref should the attempt integrate into? <!-- agent-doc:waiting-item -->
    - **Next:** you — reply under this item ▫️
  - ⟦you: 1 · agent: 0 · done: 0⟧ [#{MOOT}] Name which lineage attempt is the report. <!-- agent-doc:waiting-item -->
    - ship the west cluster
<!-- /agent:queue -->

## Todo

<!-- agent:todo -->
- [ ] [#{LIVE}] Which sink ref should the attempt integrate into?
- [ ] [#{MOOT}] Name which lineage attempt is the report and which should be cancelled
- [ ] [#{PORT}] Choose the port
- [x] [#subagent-operator-request-reportedcloseout-mootdone] Already recorded
<!-- /agent:todo -->

## Exchange

<!-- agent:exchange -->
[#{MOOT}] subagent operator request
- *kind:* question
- *blocking:* true
- *request:*
  - Which attempt is this report, and which should be cancelled?

[#{LIVE}] subagent operator request
- *kind:* question
- *blocking:* true
- *request:*
  - Which sink ref should the attempt integrate into?

```
[#{MOOT}] subagent operator request
```
<!-- /agent:exchange -->
"
        )
    }

    fn live_ids() -> HashSet<String> {
        HashSet::from([LIVE.to_string()])
    }

    #[test]
    fn prunes_a_moot_reported_closeout_decision_and_keeps_a_live_one() {
        let pruned = prune_moot_reported_closeout_decisions(&fixture(), &live_ids())
            .expect("moot decision surfaces change the document");

        assert!(
            pruned.contains(&format!("- [ ] [#{LIVE}] Which sink ref")),
            "live decision todo stays:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("- ### 🙋☑️ do [#livework] [#{LIVE}]")),
            "live queue annotation and 🙋 stay:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("- **Request**: [#{LIVE}]")),
            "live request row stays:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("[#{LIVE}] subagent operator request")),
            "live packet opener stays:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!(
                "[#{LIVE}] Which sink ref should the attempt integrate into? {WAITING_ITEM_MARKER}"
            )),
            "live waiting item stays:\n{pruned}"
        );
        assert_eq!(
            pruned
                .matches(&format!(
                    "[#{MOOT}] Name which lineage attempt is the report. {WAITING_ITEM_MARKER}"
                ))
                .count(),
            1,
            "only the operator-authored moot waiting item stays:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("- [ ] [#{PORT}] Choose the port")),
            "a non-reported-closeout decision stays:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("- ### 🙋 [#{PORT}] Choose the port")),
            "the other operator request stays on its queue row:\n{pruned}"
        );
        assert!(
            !pruned
                .contains("Name which lineage attempt is the report and which should be cancelled"),
            "open moot todo is removed:\n{pruned}"
        );
        assert!(
            pruned.contains("- ### ☑️ do [#mootwork]"),
            "the work row stays, without the operator glyph:\n{pruned}"
        );
        assert!(
            !pruned.contains(&format!("do [#mootwork] [#{MOOT}]")),
            "moot trailing annotation is removed:\n{pruned}"
        );
        assert!(
            !pruned.contains(&format!("**Request**: [#{MOOT}]")),
            "moot request row is removed:\n{pruned}"
        );
        assert!(
            !pruned.contains("- **Choices:**\n- ### 🙋☑️ do [#shared]"),
            "an empty Choices header is removed:\n{pruned}"
        );
        assert!(
            pruned.contains("**Use the recorded sink**:"),
            "a live decision's other choice stays:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("- ### 🙋☑️ do [#shared] [#{LIVE}]")),
            "a live id after a moot id stays, with 🙋:\n{pruned}"
        );
        assert!(
            !pruned.contains(&format!("[#shared] [#{MOOT}]")),
            "the moot id is removed even when a live id follows it:\n{pruned}"
        );
        assert!(
            pruned.contains("- ### ☑️ do [#latework]"),
            "a work id after a moot id stays, and 🙋 drops with the last request:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("- ### 🙋 [#{MOOT}]: keep the west cluster")),
            "a local answer stays:\n{pruned}"
        );
        assert!(
            pruned.contains(
                "- [x] [#subagent-operator-request-reportedcloseout-mootdone] Already recorded"
            ),
            "a checked decision stays as history:\n{pruned}"
        );
        assert!(
            pruned.contains("ship the west cluster"),
            "operator text under a waiting item stays:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("~~{MOOT} subagent operator request (moot)~~")),
            "the packet opener is struck so it cannot be parsed again:\n{pruned}"
        );
        assert!(
            pruned.contains(&format!("```\n[#{MOOT}] subagent operator request\n```")),
            "a fenced copy of the opener stays:\n{pruned}"
        );
        assert!(
            !pruned.contains("**State:** Complete."),
            "the open work row is not marked complete:\n{pruned}"
        );
        assert!(pruned.contains("do [#mootwork]"), "the work item stays");
        let packets = blocking_operator_request_packets(&pruned);
        assert!(
            packets.iter().any(|packet| packet.id == LIVE),
            "the live packet still parses"
        );
        assert!(
            packets.iter().all(|packet| packet.id != MOOT),
            "the struck moot packet no longer parses: {packets:?}"
        );
        assert!(
            prune_moot_reported_closeout_decisions(&pruned, &live_ids()).is_none(),
            "a second pass changes nothing"
        );
    }
}
