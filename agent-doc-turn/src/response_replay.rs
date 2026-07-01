use std::collections::HashSet;

use agent_doc_frontmatter::frontmatter;
use agent_doc_prompt_lines::text_line_looks_like_prompt_target;

pub fn first_response_heading_line(response: &str) -> Option<&str> {
    response
        .lines()
        .map(str::trim)
        .find(|line| line.starts_with("### Re:"))
}

fn normalize_replay_topic(text: &str) -> String {
    let trimmed = text.trim();
    let trimmed = trimmed
        .strip_prefix("❯ ")
        .unwrap_or(trimmed)
        .strip_prefix("### Re:")
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed
        .split_once(" — ")
        .map(|(topic, _)| topic)
        .unwrap_or(trimmed)
        .trim();
    let trimmed = trimmed
        .strip_prefix("do ")
        .unwrap_or(trimmed)
        .trim_start_matches('#')
        .trim();

    let mut normalized = String::new();
    let mut last_was_space = false;
    for ch in trimmed.chars() {
        if ch.is_ascii_alphanumeric() {
            normalized.push(ch.to_ascii_lowercase());
            last_was_space = false;
        } else if !last_was_space {
            normalized.push(' ');
            last_was_space = true;
        }
    }
    normalized.trim().to_string()
}

fn line_matches_historical_prompt(line: &str, topic: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("### Re:")
        || trimmed.starts_with("## Assistant")
        || trimmed.starts_with("<!--")
    {
        return false;
    }
    if !(trimmed.starts_with("❯ ")
        || trimmed.starts_with('#')
        || trimmed.starts_with("do #")
        || trimmed.starts_with("preset #"))
    {
        return false;
    }

    let normalized_line = normalize_replay_topic(trimmed);
    !normalized_line.is_empty()
        && (normalized_line == topic
            || normalized_line.contains(topic)
            || topic.contains(&normalized_line))
}

pub fn has_matching_orphan_prompt_for_committed_capture(
    doc_content: &str,
    response_heading: &str,
) -> bool {
    let topic = normalize_replay_topic(response_heading);
    if topic.is_empty() {
        return false;
    }

    let body = frontmatter::parse(doc_content)
        .map(|(_, body)| body)
        .unwrap_or(doc_content);
    let exchange = if let Ok(components) = agent_doc_element::element::parse(body) {
        components
            .iter()
            .find(|component| component.name == "exchange")
            .map(|component| component.content(body).to_string())
            .unwrap_or_else(|| body.to_string())
    } else {
        body.to_string()
    };

    let mut saw_match = false;
    for line in exchange.lines() {
        let trimmed = line.trim();
        if trimmed == response_heading.trim() {
            return false;
        }
        if saw_match && trimmed.starts_with("### Re:") {
            return false;
        }
        if line_matches_historical_prompt(trimmed, &topic) {
            saw_match = true;
        }
    }

    saw_match
}

fn wrap_template_exchange_patch(body: &str) -> String {
    let mut patch = String::from("<!-- patch:exchange -->\n");
    patch.push_str(body);
    if !body.ends_with('\n') {
        patch.push('\n');
    }
    patch.push_str("<!-- /patch:exchange -->\n");
    patch
}

pub fn extract_visible_response_patch_between(
    snapshot_doc: &str,
    current_doc: &str,
    template_mode: bool,
) -> Option<String> {
    let norm =
        |s: &str| agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(s);
    let snapshot_norm = norm(snapshot_doc);
    let current_norm = norm(current_doc);
    if current_norm == snapshot_norm
        || crate::document_drift::detect_bypassed_response_write_between(
            &snapshot_norm,
            &current_norm,
        )
        .is_none()
    {
        return None;
    }

    let diff = similar::TextDiff::from_lines(&snapshot_norm, &current_norm);
    let mut collected = String::new();
    let mut collecting = false;
    for change in diff.iter_all_changes() {
        let line = change.value();
        let trimmed = line.trim_end_matches('\n').trim();
        match change.tag() {
            similar::ChangeTag::Insert => {
                if !collecting && !crate::closeout_signal::is_exchange_response_heading(trimmed) {
                    continue;
                }
                collecting = true;
                collected.push_str(line);
            }
            similar::ChangeTag::Equal if collecting => {
                if trimmed.is_empty() {
                    collected.push_str(line);
                    continue;
                }
                if trimmed.starts_with("<!-- agent:boundary:")
                    || trimmed == "<!-- /agent:exchange -->"
                    || trimmed == "<!-- /patch:exchange -->"
                    || text_line_looks_like_prompt_target(trimmed)
                    || crate::closeout_signal::is_exchange_response_heading(trimmed)
                {
                    break;
                }
                break;
            }
            _ => {}
        }
    }

    if collected.trim().is_empty() {
        return None;
    }

    Some(if template_mode {
        wrap_template_exchange_patch(&collected)
    } else {
        collected
    })
}

/// True when the live document's `agent:exchange` already contains a `### Re:`
/// response heading whose normalized topic matches `heading` — i.e. the prompt
/// the orphan answered is already answered by a landed response.
pub fn live_exchange_answers_heading(doc_content: &str, heading: &str) -> bool {
    let target = normalize_replay_topic(heading);
    if target.is_empty() {
        return false;
    }
    let Ok(components) = agent_doc_element::element::parse(doc_content) else {
        return false;
    };
    let Some(exchange) = components.iter().find(|c| c.name == "exchange") else {
        return false;
    };
    exchange
        .content(doc_content)
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with("### Re:"))
        .any(|line| normalize_replay_topic(line) == target)
}

pub fn prompt_change_is_known_response(change_text: &str, response: &str) -> bool {
    let response_lines: HashSet<String> = normalized_response_lines(response)
        .into_iter()
        .map(|line| line.trim().trim_start_matches('❯').trim().to_string())
        .filter(|line| !line.is_empty())
        .collect();
    if response_lines.is_empty() {
        return false;
    }
    change_text
        .lines()
        .map(|line| line.trim().trim_start_matches('❯').trim())
        .filter(|line| !line.is_empty())
        .all(|line| response_lines.contains(line))
}

/// Returns true if the pending response content appears to already be applied to the document.
///
/// Checks whether the document contains the response's normalized visible lines
/// as one contiguous block. This tolerates blank-line separation and transient
/// ` (HEAD)` suffixes on response headings without treating scattered matching
/// phrases elsewhere in the document as an already-applied replay.
pub fn response_already_applied(doc: &str, response: &str) -> bool {
    let response_lines = normalized_response_lines(response);
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

/// Accepts the response as applied when the captured `response` had spurious
/// leading `❯ ` markers that the user has since stripped from the document.
/// Compares the response's normalized lines against the document after also
/// stripping a single leading `❯ ` from response lines.
pub fn response_already_applied_after_prefix_strip(doc: &str, response: &str) -> bool {
    let response_lines: Vec<String> = response
        .lines()
        .filter_map(normalize_response_line)
        .map(|line| {
            let trimmed = line.trim_start();
            if let Some(stripped) = trimmed.strip_prefix("❯ ") {
                let indent_len = line.len() - trimmed.len();
                format!("{}{}", &line[..indent_len], stripped)
            } else {
                line
            }
        })
        .collect();
    if response_lines.is_empty() {
        return false;
    }
    let doc_lines = normalized_response_lines(doc);
    doc_lines
        .windows(response_lines.len())
        .any(|window| window == response_lines.as_slice())
}

pub fn response_materialized_in_content(response: &str, content: &str) -> bool {
    let probe =
        agent_doc_template::response_materialization::response_materialization_probe_from_response(
            response,
        );
    if probe.trim().is_empty()
        || response_already_applied(content, &probe)
        || response_already_applied_after_prefix_strip(content, &probe)
    {
        return true;
    }
    let normalized_content = agent_doc_document::transient_markers::strip_guard_markers(content);
    normalized_content != content
        && (response_already_applied(&normalized_content, &probe)
            || response_already_applied_after_prefix_strip(&normalized_content, &probe))
}

/// Remove consecutive duplicate `### Re:` blocks from document content.
pub fn dedupe_responses(content: &str) -> String {
    dedupe_responses_with_report(content).0
}

pub fn first_duplicate_response_heading(content: &str) -> Option<String> {
    dedupe_responses_with_report(content).1
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct JbCacheConflictAcceptDuplicateReplay {
    pub heading: String,
    pub deduped_content: String,
}

/// Detect the late JetBrains File Cache Conflict "accept" replay shape from
/// already-loaded document contents.
///
/// The stale editor/cache payload lands after the cycle already committed, so
/// the working tree contains an extra adjacent response block while `head`
/// still contains the correct single-response document. This is safe to repair
/// by replacing the working tree and snapshot with `dedupe(current)` when that
/// result matches `head` modulo transient editor markers.
pub fn classify_jb_cache_conflict_accept_duplicate_replay(
    current: &str,
    head: &str,
) -> Option<JbCacheConflictAcceptDuplicateReplay> {
    let heading = first_duplicate_response_heading(current)?;
    let deduped = dedupe_responses(current);
    if deduped == current {
        return None;
    }
    if agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&deduped)
        != agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(head)
    {
        return None;
    }

    Some(JbCacheConflictAcceptDuplicateReplay {
        heading,
        deduped_content: head.to_string(),
    })
}

/// A late-IPC reposition / stale-patch replay re-inserted the committed
/// response into the working tree after the cycle already reached `head`.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LateIpcResponseOverapplication {
    pub remediated_content: String,
}

/// Detect "late-IPC response over-application": the working tree (`current`)
/// contains the committed `head` content plus one or more extra copies of
/// already-committed response blocks, with identical surrounding scaffold and
/// no new distinct response content.
pub fn is_committed_response_overapplication(current: &str, head: &str) -> bool {
    let (cur_scaffold, cur_responses) = split_scaffold_and_responses(current);
    let (head_scaffold, head_responses) = split_scaffold_and_responses(head);

    if cur_scaffold != head_scaffold || cur_responses == head_responses {
        return false;
    }
    let cur_set: HashSet<&String> = cur_responses.iter().collect();
    let head_set: HashSet<&String> = head_responses.iter().collect();
    cur_set == head_set && cur_responses.len() > head_responses.len()
}

/// Heading-topic-tolerant superset of [`is_committed_response_overapplication`].
pub fn is_committed_response_replay_including_stale(current: &str, head: &str) -> bool {
    let (cur_scaffold, cur_responses) = split_scaffold_and_responses(current);
    let (head_scaffold, head_responses) = split_scaffold_and_responses(head);

    if cur_scaffold != head_scaffold || cur_responses.len() <= head_responses.len() {
        return false;
    }

    let mut remaining: Vec<&String> = cur_responses.iter().collect();
    for head_block in &head_responses {
        if let Some(pos) = remaining.iter().position(|cur| *cur == head_block) {
            remaining.remove(pos);
        } else {
            return false;
        }
    }

    let head_topics: HashSet<&str> = head_responses
        .iter()
        .filter_map(|block| block.lines().next())
        .collect();
    if remaining.is_empty()
        || !remaining.iter().all(|surplus| {
            surplus
                .lines()
                .next()
                .is_some_and(|heading| head_topics.contains(heading))
        })
    {
        return false;
    }

    let head_lines: HashSet<&str> = head.lines().map(str::trim).collect();
    !current
        .lines()
        .map(str::trim)
        .any(|line| line_carries_user_directive(line) && !head_lines.contains(line))
}

/// Classify a late-IPC committed-response over-application and return the
/// content that should be restored.
pub fn classify_late_ipc_response_overapplication(
    current: &str,
    head: &str,
) -> Option<LateIpcResponseOverapplication> {
    if is_committed_response_overapplication(current, head)
        || is_committed_response_replay_including_stale(current, head)
    {
        return Some(LateIpcResponseOverapplication {
            remediated_content: head.to_string(),
        });
    }
    None
}

fn line_carries_user_directive(line: &str) -> bool {
    let t = line.trim();
    if t.starts_with('❯') || t.starts_with("preset ") || t.starts_with("dispatch ") {
        return true;
    }
    for kw in ["do ", "re "] {
        if let Some(rest) = t.strip_prefix(kw) {
            let rest = rest.trim_start();
            if rest.starts_with("[#") || rest.starts_with('#') {
                return true;
            }
        }
    }
    let bare = t.strip_prefix("- ").unwrap_or(t).trim_start();
    bare.starts_with("[#")
}

fn is_response_heading(trimmed: &str) -> bool {
    trimmed.starts_with("### Re:")
        || trimmed.starts_with("#### Re:")
        || trimmed.starts_with("##### Re:")
        || trimmed.starts_with("###### Re:")
        || trimmed == "## Assistant"
}

fn split_scaffold_and_responses(content: &str) -> (Vec<String>, Vec<String>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut scaffold = Vec::new();
    let mut responses = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        if is_response_heading(trimmed) {
            let start = i;
            i += 1;
            while i < lines.len()
                && !is_response_heading(lines[i].trim())
                && !lines[i].trim().starts_with("<!-- /agent:")
            {
                i += 1;
            }
            let block = lines[start..i]
                .iter()
                .filter_map(|line| normalize_response_block_line(line))
                .collect::<Vec<_>>()
                .join("\n");
            responses.push(block);
        } else {
            if !trimmed.is_empty() && !trimmed.starts_with("<!-- agent:boundary:") {
                scaffold.push(trimmed.to_string());
            }
            i += 1;
        }
    }
    (scaffold, responses)
}

fn dedupe_responses_with_report(content: &str) -> (String, Option<String>) {
    let lines: Vec<&str> = content.lines().collect();
    let mut result_lines: Vec<&str> = Vec::new();

    let mut blocks: Vec<(usize, usize)> = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if lines[i].starts_with("### Re:") {
            let start = i;
            i += 1;
            while i < lines.len()
                && !lines[i].starts_with("### Re:")
                && !lines[i].starts_with("<!-- /agent:")
            {
                i += 1;
            }
            blocks.push((start, i));
        } else {
            i += 1;
        }
    }

    if blocks.len() < 2 {
        return (content.to_string(), None);
    }

    let mut skip_ranges = Vec::new();
    let mut first_duplicate = None;
    for pair in blocks.windows(2) {
        let (s1, e1) = pair[0];
        let (s2, e2) = pair[1];
        let block1 = lines[s1..e1]
            .iter()
            .filter_map(|line| normalize_response_block_line(line))
            .collect::<Vec<_>>()
            .join("\n");
        let block2 = lines[s2..e2]
            .iter()
            .filter_map(|line| normalize_response_block_line(line))
            .collect::<Vec<_>>()
            .join("\n");
        if block1 == block2 {
            let b1_corrupt = block_has_prompt_prefixed_body(&lines[s1..e1]);
            let b2_corrupt = block_has_prompt_prefixed_body(&lines[s2..e2]);
            let (skip_s, skip_e) = if b1_corrupt && !b2_corrupt {
                (s1, e1)
            } else {
                (s2, e2)
            };
            if first_duplicate.is_none() {
                first_duplicate = Some(lines[skip_s].trim().to_string());
            }
            skip_ranges.push((skip_s, skip_e));
        }
    }

    if skip_ranges.is_empty() {
        return (content.to_string(), None);
    }

    for (i, line) in lines.iter().enumerate() {
        let in_skip = skip_ranges.iter().any(|(s, e)| i >= *s && i < *e);
        if !in_skip {
            result_lines.push(line);
        }
    }

    let mut result = result_lines.join("\n");
    if content.ends_with('\n') && !result.ends_with('\n') {
        result.push('\n');
    }
    (result, first_duplicate)
}

fn normalize_response_block_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.starts_with("<!-- agent:boundary:") {
        return None;
    }
    if trimmed.starts_with("### Re:") {
        return Some(
            trimmed
                .strip_suffix(" (HEAD)")
                .unwrap_or(trimmed)
                .to_string(),
        );
    }
    let unprefixed = trimmed
        .strip_prefix('❯')
        .map(str::trim_start)
        .unwrap_or(trimmed);
    Some(unprefixed.to_string())
}

fn block_has_prompt_prefixed_body(block_lines: &[&str]) -> bool {
    block_lines.iter().any(|line| {
        let trimmed = line.trim();
        !trimmed.starts_with("### Re:")
            && !trimmed.starts_with("<!-- agent:boundary:")
            && trimmed.starts_with('❯')
    })
}

fn normalized_response_lines(content: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = content.lines().peekable();
    while let Some(line) = lines.next() {
        if line.trim() == "> **Queue prompt:**" {
            while let Some(next) = lines.peek() {
                if next.trim_start().starts_with('>') {
                    lines.next();
                } else {
                    break;
                }
            }
            continue;
        }
        if let Some(normalized) = normalize_response_line(line) {
            out.push(normalized);
        }
    }
    out
}

fn normalize_response_line(line: &str) -> Option<String> {
    let raw = line.trim_end_matches('\r');
    let trimmed = raw.trim();
    if trimmed.is_empty()
        || trimmed.starts_with("<!-- patch:")
        || trimmed.starts_with("<!-- /patch:")
        || trimmed.starts_with("<!-- agent:")
        || trimmed.starts_with("<!-- /agent:")
    {
        return None;
    }
    Some(strip_transient_response_head_marker(raw))
}

fn strip_transient_response_head_marker(line: &str) -> String {
    if let Some(stripped) = line.strip_suffix(" (HEAD)") {
        let trimmed = stripped.trim_start();
        let is_re_heading = trimmed.starts_with("### Re:");
        let is_bold_re_heading = trimmed.starts_with("**Re:") && trimmed.ends_with("**");
        if is_re_heading || is_bold_re_heading {
            return stripped.to_string();
        }
    }
    line.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_already_applied_tolerates_queue_prompt_echo_between_heading_and_body() {
        let captured_response = "### Re: do [#thing] — opus-4-8\n\nShipped the fix.\n";
        let materialized_doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do [#thing] — opus-4-8\n\n",
            "> **Queue prompt:**\n",
            ">\n",
            "> do [#thing]\n\n",
            "Shipped the fix.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(response_already_applied(
            materialized_doc,
            captured_response
        ));

        let other_doc = concat!(
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: do [#other] — opus-4-8\n\nUnrelated.\n",
            "<!-- /agent:exchange -->\n",
        );
        assert!(!response_already_applied(other_doc, captured_response));
    }

    #[test]
    fn dedup_requires_contiguous_normalized_response_block() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: topic — opus-4-6\n",
            "Implemented in `src/agent-doc`.\n",
            "- `cargo test`\n",
            "<!-- /patch:exchange -->\n"
        );
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: topic — opus-4-6 (HEAD)\n",
            "Earlier answer.\n\n",
            "Implemented in `src/agent-doc`.\n\n",
            "Unrelated text.\n",
            "- `cargo test`\n",
            "<!-- /agent:exchange -->\n"
        );

        assert!(!response_already_applied(doc, response));
    }

    #[test]
    fn dedup_short_response_still_requires_contiguous_match() {
        let response = "Implemented.\nDone.\n";
        let doc = "Implemented.\nOther line.\nDone.\n";

        assert!(!response_already_applied(doc, response));
    }

    #[test]
    fn patch_wrapped_response_is_materialized_by_visible_patch_body() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "<!-- /patch:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_materialized_in_content(response, content));
    }

    #[test]
    fn patch_wrapped_response_matches_visible_body_with_transient_markers() {
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: visible body — gpt-5\n\n",
            "The document contains the applied body only.\n",
            "No follow-up. <!-- no-pending-capture -->\n",
            "<!-- /patch:exchange -->\n",
        );
        let content = concat!(
            "<!-- agent:exchange -->\n",
            "### Re: visible body — gpt-5 (HEAD)\n\n",
            "The document contains the applied body only.\n",
            "No follow-up. <!-- no-pending-capture -->\n",
            "<!-- agent:boundary:test -->\n",
            "<!-- /agent:exchange -->\n",
        );

        assert!(response_materialized_in_content(response, content));
    }

    #[test]
    fn visible_response_patch_extracts_inserted_response_body() {
        let snapshot = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do #ship\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do #ship\n",
            "### Re: do #ship — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );

        assert_eq!(
            extract_visible_response_patch_between(snapshot, current, false),
            Some("### Re: do #ship — gpt-5\n\nDone.\n".to_string())
        );
    }

    #[test]
    fn visible_response_patch_wraps_template_exchange_patch() {
        let snapshot = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do #ship\n",
            "<!-- /agent:exchange -->\n"
        );
        let current = concat!(
            "<!-- agent:exchange -->\n",
            "❯ do #ship\n",
            "### Re: do #ship — gpt-5\n\n",
            "Done.\n",
            "<!-- /agent:exchange -->\n"
        );

        assert_eq!(
            extract_visible_response_patch_between(snapshot, current, true),
            Some(
                concat!(
                    "<!-- patch:exchange -->\n",
                    "### Re: do #ship — gpt-5\n\n",
                    "Done.\n",
                    "<!-- /patch:exchange -->\n"
                )
                .to_string()
            )
        );
    }

    #[test]
    fn dedupe_removes_consecutive_duplicate() {
        let content = "### Re: Foo\nContent A.\n### Re: Foo\nContent A.\n### Re: Bar\nContent B.\n";

        assert_eq!(
            dedupe_responses(content),
            "### Re: Foo\nContent A.\n### Re: Bar\nContent B.\n"
        );
        assert_eq!(
            first_duplicate_response_heading(content).as_deref(),
            Some("### Re: Foo")
        );
    }

    #[test]
    fn dedupe_preserves_non_consecutive_duplicates() {
        let content = "### Re: Foo\nContent.\n### Re: Bar\nOther.\n### Re: Foo\nContent.\n";

        assert_eq!(dedupe_responses(content), content);
    }

    #[test]
    fn dedupe_treats_head_marker_as_transient() {
        let content = "\
### Re: Foo — gpt-5
Content.
<!-- agent:boundary:old -->
### Re: Foo — gpt-5 (HEAD)
Content.
<!-- agent:boundary:new -->
";

        assert_eq!(
            dedupe_responses(content),
            "### Re: Foo — gpt-5\nContent.\n<!-- agent:boundary:old -->\n"
        );
        assert_eq!(
            first_duplicate_response_heading(content).as_deref(),
            Some("### Re: Foo — gpt-5 (HEAD)")
        );
    }

    #[test]
    fn classify_duplicate_replay_returns_committed_head_repair() {
        let current = "\
### Re: Foo — gpt-5
Content.
<!-- agent:boundary:old -->
### Re: Foo — gpt-5 (HEAD)
Content.
<!-- agent:boundary:late -->
";
        let head = "\
### Re: Foo — gpt-5
Content.
<!-- agent:boundary:committed -->
";

        let replay = classify_jb_cache_conflict_accept_duplicate_replay(current, head)
            .expect("duplicate replay should be classified");

        assert_eq!(replay.heading, "### Re: Foo — gpt-5 (HEAD)");
        assert_eq!(replay.deduped_content, head);
    }

    const HEAD_DOC: &str = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
<!-- /agent:exchange -->
";

    #[test]
    fn overapplication_detects_boundary_wrapped_duplicate() {
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
<!-- /agent:exchange -->
";

        assert!(is_committed_response_overapplication(current, HEAD_DOC));
    }

    #[test]
    fn classify_late_ipc_overapplication_returns_committed_head_repair() {
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:replayed -->
<!-- /agent:exchange -->
";

        let overapplication = classify_late_ipc_response_overapplication(current, HEAD_DOC)
            .expect("late IPC replay should be classified");

        assert_eq!(overapplication.remediated_content, HEAD_DOC);
    }

    #[test]
    fn overapplication_rejects_new_response_content() {
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
### Re: a different answer — opus-4-8
New content.
<!-- agent:boundary:newone -->
<!-- /agent:exchange -->
";

        assert!(!is_committed_response_overapplication(current, HEAD_DOC));
    }

    #[test]
    fn stale_replay_detects_drifted_body_duplicate_of_committed_topic() {
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
### Re: fix thing — opus-4-8 (HEAD)
Fixed it.
Note: stale draft paragraph the committed copy dropped.
<!-- agent:boundary:stale -->
<!-- /agent:exchange -->
";

        assert!(!is_committed_response_overapplication(current, HEAD_DOC));
        assert!(is_committed_response_replay_including_stale(
            current, HEAD_DOC
        ));
    }

    #[test]
    fn stale_replay_rejects_new_topic() {
        let current = "\
<!-- agent:exchange -->
do [#fix-thing]
### Re: fix thing — opus-4-8
Fixed it.
<!-- agent:boundary:88409761 -->
### Re: a brand new topic — opus-4-8
Genuinely new answer.
<!-- agent:boundary:newone -->
<!-- /agent:exchange -->
";

        assert!(!is_committed_response_replay_including_stale(
            current, HEAD_DOC
        ));
    }

    #[test]
    fn dedupe_collapses_prompt_prefixed_corrupted_duplicate() {
        let content = "\
### Re: fix thing — opus-4-8
❯ **Scope:** narrow.
❯ **Commits:** abc123.
### Re: fix thing — opus-4-8 (HEAD)
**Scope:** narrow.
**Commits:** abc123.
";

        let result = dedupe_responses(content);

        assert_eq!(result.matches("### Re: fix thing").count(), 1);
        assert!(!result.contains('❯'));
        assert_eq!(dedupe_responses(&result), result);
    }

    #[test]
    fn overapplication_rejects_lost_response() {
        let head = "\
<!-- agent:exchange -->
### Re: one — opus-4-8
First.
### Re: two — opus-4-8
Second.
<!-- agent:boundary:x -->
<!-- /agent:exchange -->
";
        let current = "\
<!-- agent:exchange -->
### Re: one — opus-4-8
First.
<!-- agent:boundary:x -->
<!-- /agent:exchange -->
";

        assert!(!is_committed_response_overapplication(current, head));
    }
}
