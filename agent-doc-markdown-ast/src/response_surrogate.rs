//! Stable surrogate ids for newly materialized assistant responses
//! (`#responsesurrogate`).
//!
//! The id is 12 lowercase hex, the leading SHA-256 prefix of the response's
//! token-stripped identity digest. It is appended to the `### Re:` heading
//! (before a transient ` (HEAD)`), to the exchange prompt that heading answers,
//! and — once the answered queue row is struck — to that row outside `~~`.
//! Historical responses that have no token are left unchanged.

use std::collections::{HashMap, HashSet};

use agent_doc_element::element::{self, Component};
use agent_doc_element_queue::{
    response_surrogate_token, split_trailing_response_surrogate, strip_priority_markers,
    strip_response_surrogate_annotation,
};
use agent_doc_hash::short_content_hash;

use crate::exchange_tree::{
    ExchangeNode, ExchangeNodeKind, parse_exchange_nodes, render_exchange_nodes,
    response_identity_digest,
};

/// Heading comparison key for `(HEAD)` baseline membership.
///
/// Strips a transient ` (HEAD)` suffix and one trailing response-surrogate
/// token. The visible line keeps both; only the key drops them.
pub fn response_heading_baseline_key(heading_line: &str) -> String {
    let line = heading_line.trim();
    let line = line.strip_suffix(" (HEAD)").unwrap_or(line).trim_end();
    split_trailing_response_surrogate(line).0.trim().to_string()
}

/// Give each newly materialized `### Re:` response in `after` one stable
/// surrogate id, and append that id to the prompt it answers.
///
/// `before` is the document before this closeout edit. Responses present there
/// without a token are not backfilled. An already-written heading token is kept.
pub fn stamp_new_response_surrogates(before: &str, after: &str) -> String {
    if before == after {
        return after.to_string();
    }
    let Ok(after_components) = element::parse(after) else {
        return after.to_string();
    };
    let before_components = element::parse(before).unwrap_or_default();

    let before_exchange = component_bodies(before, &before_components, "exchange");
    let mut historical = HashSet::new();
    for body in &before_exchange {
        for node in re_responses(&parse_exchange_nodes(body)) {
            historical.insert(response_identity_digest(&node.lines));
            historical.insert(historical_digest(&node.lines));
        }
    }

    let after_exchange = component_bodies(after, &after_components, "exchange");
    let mut used: HashMap<String, String> = HashMap::new();
    for body in before_exchange.iter().chain(after_exchange.iter()) {
        for node in re_responses(&parse_exchange_nodes(body)) {
            if let Some(id) = heading_token_id(node.lines.first().map(String::as_str).unwrap_or(""))
            {
                let digest = response_identity_digest(&node.lines);
                used.entry(id.to_string()).or_insert(digest);
            }
        }
    }

    let before_struck = struck_work_keys(&component_bodies(before, &before_components, "queue"));
    let mut links = Vec::new();
    let mut doc = after.to_string();
    let mut exchange_edits = Vec::new();
    for component in after_components
        .iter()
        .filter(|component| component.name == "exchange")
    {
        let body = component.content(after);
        let (stamped, mut found) = stamp_exchange_body(body, &historical, &mut used);
        if stamped != body {
            exchange_edits.push(BodyEdit {
                open_end: component.open_end,
                close_start: component.close_start,
                body: stamped,
            });
        }
        links.append(&mut found);
    }
    apply_body_edits(&mut doc, exchange_edits);

    if links.is_empty() {
        return doc;
    }
    let Ok(components) = element::parse(&doc) else {
        return doc;
    };
    let mut queue_edits = Vec::new();
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        let body = component.content(&doc);
        let stamped = stamp_queue_body(body, &before_struck, &links);
        if stamped != body {
            queue_edits.push(BodyEdit {
                open_end: component.open_end,
                close_start: component.close_start,
                body: stamped,
            });
        }
    }
    apply_body_edits(&mut doc, queue_edits);
    doc
}

struct BodyEdit {
    open_end: usize,
    close_start: usize,
    body: String,
}

struct LinkedResponse {
    id: String,
    topic_key: String,
    echo_keys: Vec<String>,
}

fn component_bodies<'a>(doc: &'a str, components: &'a [Component], name: &str) -> Vec<&'a str> {
    components
        .iter()
        .filter(|component| component.name == name)
        .map(|component| component.content(doc))
        .collect()
}

fn apply_body_edits(doc: &mut String, mut edits: Vec<BodyEdit>) {
    edits.sort_by_key(|edit| edit.open_end);
    for edit in edits.into_iter().rev() {
        doc.replace_range(edit.open_end..edit.close_start, &edit.body);
    }
}

fn re_responses(nodes: &[ExchangeNode]) -> Vec<&ExchangeNode> {
    nodes.iter().filter(|node| is_re_response(node)).collect()
}

fn is_re_response(node: &ExchangeNode) -> bool {
    matches!(node.kind, ExchangeNodeKind::Response { .. })
        && node
            .lines
            .first()
            .is_some_and(|line| line.trim().starts_with("### Re:"))
}

fn stamp_exchange_body(
    body: &str,
    historical: &HashSet<String>,
    used: &mut HashMap<String, String>,
) -> (String, Vec<LinkedResponse>) {
    let mut nodes = parse_exchange_nodes(body);
    let code_ranges = element::find_code_ranges(body);
    let offsets = line_offsets(&nodes);
    let mut links = Vec::new();
    let response_indexes: Vec<usize> = nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| matches!(node.kind, ExchangeNodeKind::Response { .. }))
        .map(|(index, _)| index)
        .collect();

    for (ordinal, &index) in response_indexes.iter().enumerate() {
        if !is_re_response(&nodes[index]) {
            continue;
        }
        let heading_at = offsets.get(index).and_then(|lines| lines.first()).copied();
        if heading_at.is_some_and(|offset| offset_in_code(offset, &code_ranges)) {
            continue;
        }
        let digest = response_identity_digest(&nodes[index].lines);
        let prior_digest = historical_digest(&nodes[index].lines);
        let echo_keys = echo_work_keys(&nodes[index].lines);
        let topic_key =
            topic_key_from_heading(nodes[index].lines.first().map(String::as_str).unwrap_or(""));
        let existing =
            heading_token_id(nodes[index].lines.first().map(String::as_str).unwrap_or(""));
        let (id, minted_now) = if let Some(id) = existing {
            (id.to_string(), false)
        } else if historical.contains(&digest) || historical.contains(&prior_digest) {
            continue;
        } else {
            (mint_id(&digest, used), true)
        };
        let token = response_surrogate_token(&id);
        if minted_now {
            if let Some(line) = nodes[index].lines.first_mut() {
                *line = append_response_token(line, &token);
            }
            let start = if ordinal == 0 {
                0
            } else {
                response_indexes[ordinal - 1] + 1
            };
            stamp_prompt_tail(&mut nodes, start, index, &token, &offsets, &code_ranges);
        }
        stamp_echo_lines(
            &mut nodes[index].lines,
            &token,
            offsets.get(index),
            &code_ranges,
        );
        links.push(LinkedResponse {
            id,
            topic_key,
            echo_keys,
        });
    }
    (render_exchange_nodes(&nodes), links)
}

fn mint_id(digest: &str, used: &mut HashMap<String, String>) -> String {
    let mut salt = 0u32;
    loop {
        let material = if salt == 0 {
            digest.to_string()
        } else {
            format!("{digest}\0{salt}")
        };
        let id = short_content_hash(&material);
        match used.get(&id) {
            Some(existing) if existing == digest => return id,
            Some(_) => salt = salt.saturating_add(1),
            None => {
                used.insert(id.clone(), digest.to_string());
                return id;
            }
        }
    }
}

fn heading_token_id(line: &str) -> Option<&str> {
    let core = line.trim_end_matches(['\n', '\r']).trim_end();
    let core = core.strip_suffix(" (HEAD)").unwrap_or(core).trim_end();
    let (_, token) = split_trailing_response_surrogate(core);
    let token = token?;
    token
        .strip_prefix(agent_doc_element_queue::RESPONSE_SURROGATE_PREFIX)?
        .strip_suffix('⟧')
}

fn append_response_token(line: &str, token: &str) -> String {
    let (core, newline) = match line.strip_suffix('\n') {
        Some(rest) => (rest, "\n"),
        None => (line, ""),
    };
    let (core, cr) = match core.strip_suffix('\r') {
        Some(rest) => (rest, "\r"),
        None => (core, ""),
    };
    let head = if core.ends_with(" (HEAD)") {
        " (HEAD)"
    } else {
        ""
    };
    let body = if head.is_empty() {
        core
    } else {
        &core[..core.len() - head.len()]
    };
    if split_trailing_response_surrogate(body).1.is_some() {
        return line.to_string();
    }
    format!("{body} {token}{head}{cr}{newline}")
}

fn line_offsets(nodes: &[ExchangeNode]) -> Vec<Vec<usize>> {
    let mut offsets = Vec::with_capacity(nodes.len());
    let mut pos = 0usize;
    for node in nodes {
        let mut line_offs = Vec::with_capacity(node.lines.len());
        for line in &node.lines {
            line_offs.push(pos);
            pos += line.len();
        }
        offsets.push(line_offs);
    }
    offsets
}

fn offset_in_code(offset: usize, ranges: &[(usize, usize)]) -> bool {
    ranges
        .iter()
        .any(|&(start, end)| offset >= start && offset < end)
}

fn stamp_prompt_tail(
    nodes: &mut [ExchangeNode],
    start: usize,
    end: usize,
    token: &str,
    offsets: &[Vec<usize>],
    code_ranges: &[(usize, usize)],
) {
    let mut carets = Vec::new();
    for node_index in start..end {
        if !matches!(nodes[node_index].kind, ExchangeNodeKind::Prompt) {
            continue;
        }
        for (line_index, line) in nodes[node_index].lines.iter().enumerate() {
            if !line.trim().starts_with('❯') {
                continue;
            }
            let offset = offsets
                .get(node_index)
                .and_then(|lines| lines.get(line_index))
                .copied();
            if offset.is_some_and(|offset| offset_in_code(offset, code_ranges)) {
                continue;
            }
            carets.push((node_index, line_index));
        }
    }
    if !carets.is_empty() {
        for (node_index, line_index) in carets {
            let line = &nodes[node_index].lines[line_index];
            nodes[node_index].lines[line_index] = append_response_token(line, token);
        }
        return;
    }
    for node_index in (start..end).rev() {
        if !matches!(nodes[node_index].kind, ExchangeNodeKind::Prompt) {
            continue;
        }
        let Some(line_index) =
            nodes[node_index]
                .lines
                .iter()
                .enumerate()
                .rev()
                .find_map(|(line_index, line)| {
                    let trimmed = line.trim();
                    if trimmed.is_empty() || is_boundary_line(trimmed) {
                        return None;
                    }
                    let offset = offsets
                        .get(node_index)
                        .and_then(|lines| lines.get(line_index))
                        .copied();
                    if offset.is_some_and(|offset| offset_in_code(offset, code_ranges)) {
                        return None;
                    }
                    Some(line_index)
                })
        else {
            continue;
        };
        let line = &nodes[node_index].lines[line_index];
        nodes[node_index].lines[line_index] = append_response_token(line, token);
        return;
    }
}

fn is_boundary_line(trimmed: &str) -> bool {
    trimmed.starts_with("<!-- agent:boundary:") && trimmed.ends_with("-->")
}

fn stamp_echo_lines(
    lines: &mut [String],
    token: &str,
    offsets: Option<&Vec<usize>>,
    code_ranges: &[(usize, usize)],
) {
    let mut index = 0usize;
    while index < lines.len() {
        if !is_echo_label(&lines[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < lines.len() && is_quote_line(&lines[index]) && !is_echo_label(&lines[index]) {
            index += 1;
        }
        let last = last_echo_content_index(lines, start, index);
        let in_code = offsets
            .and_then(|offsets| offsets.get(last))
            .is_some_and(|offset| offset_in_code(*offset, code_ranges));
        if !in_code {
            lines[last] = append_response_token(&lines[last], token);
        }
    }
}

fn last_echo_content_index(lines: &[String], start: usize, end: usize) -> usize {
    let mut last = end - 1;
    while last > start {
        if !echo_line_text(&lines[last]).trim().is_empty() {
            return last;
        }
        last -= 1;
    }
    start
}

fn is_quote_line(line: &str) -> bool {
    line.trim().starts_with('>')
}

fn is_echo_label(line: &str) -> bool {
    let mut text = line.trim();
    if !text.starts_with('>') {
        return false;
    }
    while let Some(rest) = text.strip_prefix('>') {
        text = rest.trim_start();
    }
    let text = split_trailing_response_surrogate(text.trim()).0.trim();
    let lower = text.to_ascii_lowercase();
    lower == "**queue prompt:**"
        || lower == "**queue prompts:**"
        || lower.starts_with("**queue prompt:**")
        || lower.starts_with("**queue prompts:**")
}

fn echo_line_text(line: &str) -> String {
    let mut text = line.trim();
    while let Some(rest) = text.strip_prefix('>') {
        text = rest.trim_start();
    }
    let lower = text.to_ascii_lowercase();
    if let Some(rest) = lower.strip_prefix("**queue prompts:**") {
        text = &text[text.len() - rest.len()..];
    } else if let Some(rest) = lower.strip_prefix("**queue prompt:**") {
        text = &text[text.len() - rest.len()..];
    }
    strip_bullet(text.trim()).trim().to_string()
}

/// Identity used only to decide that a response was already in `before`.
///
/// Echo insertion adds a labeled blockquote and the blank lines around it.
/// Those blanks are not a new response, so the historical digest drops the
/// echo and every blank line. The minted id still uses the full digest.
fn historical_digest(lines: &[String]) -> String {
    let kept = echo_stripped_lines(lines)
        .into_iter()
        .filter(|line| !line.trim().is_empty())
        .collect::<Vec<_>>();
    response_identity_digest(&kept)
}

fn echo_stripped_lines(lines: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        if is_echo_label(&lines[index]) {
            index += 1;
            while index < lines.len()
                && is_quote_line(&lines[index])
                && !is_echo_label(&lines[index])
            {
                index += 1;
            }
            continue;
        }
        out.push(lines[index].clone());
        index += 1;
    }
    out
}

fn echo_work_keys(lines: &[String]) -> Vec<String> {
    let mut keys = Vec::new();
    let mut index = 0usize;
    while index < lines.len() {
        if !is_echo_label(&lines[index]) {
            index += 1;
            continue;
        }
        let start = index;
        index += 1;
        while index < lines.len() && is_quote_line(&lines[index]) && !is_echo_label(&lines[index]) {
            index += 1;
        }
        let mut parts = Vec::new();
        for line in &lines[start..index] {
            let extracted = echo_line_text(line);
            if extracted.trim().is_empty() {
                continue;
            }
            let key = normalize_work_key(&extracted);
            if !key.is_empty() {
                keys.push(key);
                parts.push(extracted);
            }
        }
        if !parts.is_empty() {
            let joined = normalize_work_key(&parts.join(" "));
            if !joined.is_empty() {
                keys.push(joined);
            }
        }
    }
    keys
}

fn topic_key_from_heading(line: &str) -> String {
    let rest = line
        .trim()
        .strip_prefix("### Re:")
        .unwrap_or(line.trim())
        .trim();
    let rest = rest.strip_suffix(" (HEAD)").unwrap_or(rest).trim_end();
    let rest = split_trailing_response_surrogate(rest).0;
    let topic = rest
        .split_once(" — ")
        .map(|(topic, _)| topic)
        .unwrap_or(rest);
    normalize_work_key(split_trailing_response_surrogate(topic.trim()).0)
}

fn normalize_work_key(text: &str) -> String {
    strip_response_surrogate_annotation(text)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn strip_bullet(line: &str) -> &str {
    let trimmed = line.trim_start();
    if let Some(rest) = ["- ", "* ", "+ "]
        .iter()
        .find_map(|prefix| trimmed.strip_prefix(prefix))
    {
        return rest;
    }
    let digits = trimmed
        .bytes()
        .take_while(|byte| byte.is_ascii_digit())
        .count();
    if digits > 0 && trimmed[digits..].starts_with(". ") {
        return &trimmed[digits + 2..];
    }
    trimmed
}

fn struck_work_keys(bodies: &[&str]) -> HashSet<String> {
    let mut keys = HashSet::new();
    for body in bodies {
        for (key, _) in queue_strike_lines(body) {
            keys.insert(key);
        }
    }
    keys
}

fn queue_strike_lines(body: &str) -> Vec<(String, bool)> {
    let ranges = element::find_code_ranges(body);
    let mut found = Vec::new();
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if offset_in_code(start, &ranges) || line.trim_start().starts_with("~~~") {
            continue;
        }
        if let Some(parsed) = queue_line_strike(line) {
            found.push(parsed);
        }
    }
    found
}

fn queue_line_strike(line: &str) -> Option<(String, bool)> {
    let core = line.trim_end_matches(['\n', '\r']);
    let body = strip_bullet(core.trim());
    let (body, token) = split_trailing_response_surrogate(body);
    let key = struck_work_key(body)?;
    Some((key, token.is_some()))
}

fn struck_work_key(body: &str) -> Option<String> {
    let rest = body.strip_prefix("~~")?;
    let inner = if let Some(close) = rest.find("~~ — auto-struck: ") {
        rest[..close].trim()
    } else {
        rest.strip_suffix("~~")?.trim()
    };
    if inner.is_empty() {
        return None;
    }
    let key = normalize_work_key(&strip_priority_markers(inner));
    (!key.is_empty()).then_some(key)
}

fn stamp_queue_body(
    body: &str,
    before_struck: &HashSet<String>,
    links: &[LinkedResponse],
) -> String {
    if links.is_empty() {
        return body.to_string();
    }
    let ranges = element::find_code_ranges(body);
    let mut out = String::with_capacity(body.len());
    let mut offset = 0usize;
    for line in body.split_inclusive('\n') {
        let start = offset;
        offset += line.len();
        if offset_in_code(start, &ranges) || line.trim_start().starts_with("~~~") {
            out.push_str(line);
            continue;
        }
        let Some((key, has_token)) = queue_line_strike(line) else {
            out.push_str(line);
            continue;
        };
        if has_token || before_struck.contains(&key) {
            out.push_str(line);
            continue;
        }
        let Some(link) = links.iter().rev().find(|link| {
            !key.is_empty()
                && (link.topic_key == key || link.echo_keys.iter().any(|echo| echo == &key))
        }) else {
            out.push_str(line);
            continue;
        };
        out.push_str(&append_response_token(
            line,
            &response_surrogate_token(&link.id),
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::exchange_tree::parse_exchange_nodes;

    fn doc(exchange: &str, queue: &str) -> String {
        format!(
            "<!-- agent:exchange -->\n{exchange}<!-- /agent:exchange -->\n<!-- agent:run -->\n{queue}<!-- /agent:run -->\n"
        )
    }

    fn token_ids(text: &str) -> Vec<String> {
        let mut ids = Vec::new();
        let mut rest = text;
        while let Some(start) = rest.find("⟦response:") {
            rest = &rest[start + "⟦response:".len()..];
            let Some(end) = rest.find('⟧') else {
                break;
            };
            ids.push(rest[..end].to_string());
            rest = &rest[end + '⟧'.len_utf8()..];
        }
        ids
    }

    fn assert_one_id(text: &str) -> String {
        let ids = token_ids(text);
        assert!(!ids.is_empty(), "missing surrogate:\n{text}");
        assert!(
            ids.iter().all(|id| id == &ids[0] && is_hex12(id)),
            "ids diverged: {ids:?}\n{text}"
        );
        ids[0].clone()
    }

    fn is_hex12(id: &str) -> bool {
        id.len() == 12
            && id
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
    }

    #[test]
    fn new_response_shares_one_id_across_heading_prompt_and_struck_row() {
        let before = doc("❯ do the thing\n", "- do the thing\n");
        let after = doc(
            "❯ do the thing\n### Re: do the thing — model\nanswered\n",
            "- ~~do the thing~~\n",
        );
        let stamped = stamp_new_response_surrogates(&before, &after);
        let id = assert_one_id(&stamped);
        assert!(stamped.contains(&format!("### Re: do the thing — model ⟦response:{id}⟧")));
        assert!(stamped.contains(&format!("❯ do the thing ⟦response:{id}⟧")));
        assert!(stamped.contains(&format!("- ~~do the thing~~ ⟦response:{id}⟧")));
        assert_eq!(stamp_new_response_surrogates(&stamped, &stamped), stamped);
    }

    #[test]
    fn historical_response_is_not_backfilled_when_only_the_echo_or_strike_changes() {
        let before = doc(
            "### Re: do the thing — model\nanswered\n",
            "- do the thing\n",
        );
        let after = doc(
            "### Re: do the thing — model\n> **Queue prompt:**\n> do the thing\nanswered\n",
            "- ~~do the thing~~\n",
        );
        let stamped = stamp_new_response_surrogates(&before, &after);
        assert!(
            !stamped.contains("⟦response:"),
            "historical response was rewritten:\n{stamped}"
        );
    }

    #[test]
    fn live_queue_row_is_not_stamped() {
        let before = doc("❯ do the thing\n", "- do the thing\n");
        let after = doc(
            "❯ do the thing\n### Re: do the thing — model\nanswered\n",
            "- do the thing\n",
        );
        let stamped = stamp_new_response_surrogates(&before, &after);
        assert!(stamped.contains("### Re: do the thing — model ⟦response:"));
        assert!(stamped.contains("- do the thing\n"));
        assert!(!stamped.contains("- do the thing ⟦response:"));
    }

    #[test]
    fn two_new_responses_get_two_ids_and_the_token_precedes_head() {
        let before = doc("", "");
        let after = doc(
            "❯ one\n### Re: one — model (HEAD)\na\n❯ two\n### Re: two — model\nb\n",
            "- ~~one~~\n- ~~two~~ — auto-struck: answered this cycle\n",
        );
        let stamped = stamp_new_response_surrogates(&before, &after);
        let ids = token_ids(&stamped);
        let unique: HashSet<_> = ids.iter().cloned().collect();
        assert!(unique.len() >= 2, "{ids:?}\n{stamped}");
        assert!(
            stamped.contains("### Re: one — model ⟦response:") && stamped.contains("⟧ (HEAD)"),
            "{stamped}"
        );
        assert!(
            stamped.contains("~~two~~ — auto-struck: answered this cycle ⟦response:"),
            "{stamped}"
        );
        assert!(!stamped.contains("~~two~~ ⟦response:"));
    }

    #[test]
    fn caret_prompt_is_stamped_and_a_session_summary_line_is_not() {
        let before = doc("", "");
        let after = doc(
            "Summary of the session\n❯ real prompt\n### Re: real prompt — model\nbody\n",
            "",
        );
        let stamped = stamp_new_response_surrogates(&before, &after);
        assert!(stamped.contains("❯ real prompt ⟦response:"));
        assert!(!stamped.contains("Summary of the session ⟦response:"));
    }

    #[test]
    fn prompt_tail_without_a_caret_stamps_its_last_content_line_not_a_boundary() {
        let before = doc("", "");
        let after = doc(
            "real prompt line\n<!-- agent:boundary:abc -->\n### Re: real prompt line — model\nbody\n",
            "",
        );
        let stamped = stamp_new_response_surrogates(&before, &after);
        assert!(stamped.contains("real prompt line ⟦response:"));
        assert!(!stamped.contains("<!-- agent:boundary:abc --> ⟦response:"));
        assert!(stamped.contains("<!-- agent:boundary:abc -->\n"));
    }

    #[test]
    fn echo_last_content_line_receives_the_heading_id_and_node_ids_ignore_it() {
        let before = doc("❯ do the thing\n", "");
        let after = doc(
            "❯ do the thing\n### Re: do the thing — model\n> **Queue prompt:**\n> do the thing\nanswered\n",
            "- ~~do the thing~~\n",
        );
        let stamped = stamp_new_response_surrogates(&before, &after);
        let id = assert_one_id(&stamped);
        assert!(stamped.contains(&format!("> do the thing ⟦response:{id}⟧")));
        let bare_nodes = parse_exchange_nodes(
            "❯ do the thing\n### Re: do the thing — model\n> **Queue prompt:**\n> do the thing\nanswered\n",
        );
        let exchange = stamped
            .split_once("<!-- agent:exchange -->\n")
            .unwrap()
            .1
            .split_once("<!-- /agent:exchange -->")
            .unwrap()
            .0;
        let stamped_nodes = parse_exchange_nodes(exchange);
        assert_eq!(bare_nodes.len(), stamped_nodes.len());
        for (bare, stamped_node) in bare_nodes.iter().zip(stamped_nodes.iter()) {
            assert_eq!(bare.node_id(), stamped_node.node_id());
        }
    }

    #[test]
    fn baseline_key_ignores_the_token_and_head_suffix() {
        assert_eq!(
            response_heading_baseline_key("### Re: old — model ⟦response:0123456789ab⟧ (HEAD)"),
            "### Re: old — model"
        );
        assert_eq!(
            response_heading_baseline_key("### Re: old — model"),
            "### Re: old — model"
        );
    }

    #[test]
    fn kept_heading_token_still_links_a_newly_struck_row() {
        let id = "0123456789ab";
        let before = doc(
            &format!(
                "❯ do the thing ⟦response:{id}⟧\n### Re: do the thing — model ⟦response:{id}⟧\nanswered\n"
            ),
            "- do the thing\n",
        );
        let after = doc(
            &format!(
                "❯ do the thing ⟦response:{id}⟧\n### Re: do the thing — model ⟦response:{id}⟧\n> **Queue prompt:**\n> do the thing\nanswered\n"
            ),
            "- ~~do the thing~~\n",
        );
        let stamped = stamp_new_response_surrogates(&before, &after);
        assert!(stamped.contains(&format!("- ~~do the thing~~ ⟦response:{id}⟧")));
        assert!(stamped.contains(&format!("> do the thing ⟦response:{id}⟧")));
        assert_eq!(
            token_ids(&stamped)
                .iter()
                .filter(|found| *found == id)
                .count()
                >= 3,
            true
        );
    }

    #[test]
    fn agent_loop_row_is_a_queue_component() {
        let before = "<!-- agent:exchange -->\n❯ loop work\n<!-- /agent:exchange -->\n<!-- agent:loop -->\n- loop work\n<!-- /agent:loop -->\n";
        let after = "<!-- agent:exchange -->\n❯ loop work\n### Re: loop work — model\ndone\n<!-- /agent:exchange -->\n<!-- agent:loop -->\n- ~~loop work~~\n<!-- /agent:loop -->\n";
        let stamped = stamp_new_response_surrogates(before, after);
        assert!(stamped.contains("- ~~loop work~~ ⟦response:"), "{stamped}");
    }
}
