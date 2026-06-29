//! Extracted from `write.rs` (large-module split). See parent module for context.

use super::*;

pub(crate) fn is_safe_out_of_band_exchange_growth(
    snapshot_content: &str,
    file_content: &str,
) -> bool {
    if !file_content.starts_with(snapshot_content) {
        return false;
    }
    let suffix = file_content[snapshot_content.len()..].trim();
    !suffix.is_empty() && suffix.starts_with("### Re:")
}

pub(crate) fn is_safe_exchange_user_prompt_insert(
    snapshot_exchange: &str,
    file_exchange: &str,
) -> bool {
    let snap_lines: Vec<&str> = snapshot_exchange.lines().collect();
    let file_lines: Vec<&str> = file_exchange.lines().collect();

    if snap_lines.len() >= file_lines.len() {
        return false;
    }

    let prefix_len = snap_lines
        .iter()
        .zip(file_lines.iter())
        .take_while(|(s, f)| s.trim() == f.trim())
        .count();

    let suffix_len = snap_lines
        .iter()
        .rev()
        .zip(file_lines.iter().rev())
        .take_while(|(s, f)| s.trim() == f.trim())
        .count();

    if suffix_len == 0 {
        return false;
    }

    let suffix_start_in_snap = snap_lines.len().saturating_sub(suffix_len);
    let suffix_has_response = snap_lines[suffix_start_in_snap..]
        .iter()
        .any(|line| line.trim().starts_with("### Re:"));

    if !suffix_has_response {
        return false;
    }

    let insert_start = prefix_len;
    let insert_end = file_lines.len().saturating_sub(suffix_len);

    if insert_start >= insert_end {
        return false;
    }

    let inserted_lines = &file_lines[insert_start..insert_end];

    for line in inserted_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with("### Re:")
            || trimmed.starts_with("#### Re:")
            || trimmed.starts_with("<!-- agent:")
            || trimmed.starts_with("<!-- /agent:")
        {
            return false;
        }
    }

    true
}

pub(crate) fn flush_exchange_insert_block(block: &mut String) -> bool {
    let trimmed = block.trim();
    if trimmed.is_empty() {
        block.clear();
        return true;
    }
    let ok = is_safe_historical_exchange_insert_block(trimmed);
    block.clear();
    ok
}

pub(crate) fn is_safe_historical_exchange_insert_block(block: &str) -> bool {
    let non_blank: Vec<&str> = block
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .collect();
    if non_blank.is_empty() {
        return true;
    }

    let Some(first_response_idx) = non_blank.iter().position(|line| {
        line.starts_with("### Re:") || line.starts_with("#### Re:") || line.starts_with("##### Re:")
    }) else {
        return false;
    };
    if first_response_idx == 0 {
        return true;
    }

    non_blank[..first_response_idx]
        .iter()
        .all(|line| historical_exchange_prelude_looks_like_prompt_target(line))
}

pub(crate) fn historical_exchange_prelude_looks_like_prompt_target(line: &str) -> bool {
    let trimmed = line.trim();
    !trimmed.is_empty()
        && !trimmed.starts_with("<!--")
        && !trimmed.starts_with("```")
        && !trimmed.starts_with("~~~")
        && !trimmed.starts_with("### Re:")
        && !trimmed.starts_with("#### Re:")
        && !trimmed.starts_with("##### Re:")
        && (trimmed.starts_with('❯')
            || trimmed.ends_with('?')
            || historical_exchange_prelude_looks_like_imperative(trimmed))
}

pub(crate) fn historical_exchange_prelude_looks_like_imperative(line: &str) -> bool {
    let compact = line.trim_start_matches('>').trim().to_ascii_lowercase();
    compact == "go"
        || compact == "continue"
        || compact.starts_with("do #")
        || compact.starts_with("run ")
        || compact.starts_with("rerun ")
        || compact.starts_with("build ")
        || compact.starts_with("test ")
        || compact.starts_with("commit ")
        || compact.starts_with("push ")
        || compact.starts_with("fix ")
        || compact.starts_with("complete ")
}

pub(crate) fn is_safe_historical_exchange_growth(
    snapshot_content: &str,
    file_content: &str,
) -> bool {
    let diff = similar::TextDiff::from_lines(snapshot_content, file_content);
    let mut insert_block = String::new();
    let mut saw_insert = false;

    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Equal => {
                if !flush_exchange_insert_block(&mut insert_block) {
                    return false;
                }
            }
            similar::ChangeTag::Delete => return false,
            similar::ChangeTag::Insert => {
                saw_insert = true;
                insert_block.push_str(change.value());
            }
        }
    }

    saw_insert && flush_exchange_insert_block(&mut insert_block)
}

pub(crate) fn is_safe_user_follow_up_exchange_growth(
    head_content: &str,
    current_content: &str,
) -> bool {
    if head_content == current_content || !current_content.starts_with(head_content) {
        return false;
    }

    let suffix = current_content[head_content.len()..].trim();
    !suffix.is_empty()
        && suffix != "## Assistant"
        && !suffix.starts_with("### Re:")
        && !suffix.starts_with("#### Re:")
}

pub(crate) fn is_safe_out_of_band_pending_mutation(
    snapshot_content: &str,
    file_content: &str,
) -> bool {
    let (snap_prelude, snap_items, snap_postlude) =
        agent_doc_element_backlog::backlog::parse_items(snapshot_content);
    let (file_prelude, file_items, file_postlude) =
        agent_doc_element_backlog::backlog::parse_items(file_content);

    if snap_prelude.trim() != file_prelude.trim() || snap_postlude.trim() != file_postlude.trim() {
        return false;
    }
    if file_items.is_empty() {
        return false;
    }

    let file_ids: HashSet<&str> = file_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .map(|item| item.id.as_str())
        .collect();
    if file_ids.is_empty() {
        return false;
    }

    snap_items
        .iter()
        .filter(|item| !item.id.is_empty())
        .all(|item| file_ids.contains(item.id.as_str()))
}

pub(crate) fn detect_reintroduced_reaped_pending_ids(
    doc: &str,
    reaped_ids: &HashSet<String>,
) -> Result<Vec<String>> {
    if reaped_ids.is_empty() {
        return Ok(Vec::new());
    }

    let components = agent_doc_element::element::parse(doc)?;
    let mut seen = HashSet::new();
    let mut reintroduced = Vec::new();
    for component in components
        .iter()
        .filter(|component| agent_doc_element::element::is_tracked_work_component(&component.name))
    {
        let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(component.content(doc));
        for item in items {
            if !item.id.is_empty() && reaped_ids.contains(&item.id) && seen.insert(item.id.clone())
            {
                reintroduced.push(item.id);
            }
        }
    }

    reintroduced.sort();
    Ok(reintroduced)
}

pub(crate) fn strip_promptish_list_prefix(line: &str) -> &str {
    let mut trimmed = line.trim();

    if let Some(rest) = trimmed.strip_prefix('❯') {
        trimmed = rest.trim_start();
    }

    if let Some(rest) = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))
    {
        return rest.trim_start();
    }

    let digits = trimmed.chars().take_while(|c| c.is_ascii_digit()).count();
    if digits > 0 {
        let rest = &trimmed[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return rest.trim_start();
        }
    }

    trimmed
}

pub(crate) fn starts_with_prompt_preset_reference(line: &str) -> bool {
    let trimmed = strip_promptish_list_prefix(line);
    let Some(rest) = trimmed.strip_prefix('#') else {
        return false;
    };
    let token_len = rest
        .char_indices()
        .take_while(|(_, ch)| ch.is_ascii_alphanumeric() || *ch == '-' || *ch == '_')
        .map(|(idx, ch)| idx + ch.len_utf8())
        .last()
        .unwrap_or(0);
    if token_len == 0 {
        return false;
    }
    let remainder = &rest[token_len..];
    remainder.is_empty()
        || remainder
            .chars()
            .next()
            .is_some_and(|ch| ch.is_whitespace())
}

pub(crate) fn status_mutation_introduces_prompt_work(
    snapshot_content: &str,
    file_content: &str,
) -> bool {
    let diff = similar::TextDiff::from_lines(snapshot_content, file_content);
    let mut added = String::new();

    for change in diff.iter_all_changes() {
        if change.tag() == similar::ChangeTag::Insert {
            added.push_str(change.value());
        }
    }

    if added.trim().is_empty() {
        return false;
    }

    if !agent_doc_core::diff::extract_prompt_preset_requests_from_text(&added).is_empty() {
        return true;
    }

    added.lines().any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty()
            && (agent_doc_core::diff::text_line_looks_like_prompt_target(trimmed)
                || starts_with_prompt_preset_reference(trimmed))
    })
}

pub(crate) fn is_safe_out_of_band_status_mutation(
    snapshot_content: &str,
    file_content: &str,
) -> bool {
    snapshot_content.trim() != file_content.trim()
        && !status_mutation_introduces_prompt_work(snapshot_content, file_content)
}

pub(crate) fn is_empty_template_scaffold_snapshot(snapshot_doc: &str) -> bool {
    let body = agent_doc_core::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let Ok(components) = agent_doc_element::element::parse(body) else {
        return false;
    };

    let has_status = components.iter().any(|c| c.name == "status");
    let has_exchange = components.iter().any(|c| c.name == "exchange");
    let has_pending = components.iter().any(|c| is_backlog_component(&c.name));
    if !(has_status && has_exchange && has_pending) {
        return false;
    }

    components.iter().all(|component| {
        (matches!(component.name.as_str(), "status" | "exchange" | "queue")
            || is_backlog_component(&component.name))
            && normalize_component_content_for_absorb(component.content(body)).is_empty()
    })
}

pub(crate) fn classify_safe_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
    allow_historical_exchange_growth: bool,
) -> Option<&'static str> {
    if snapshot_doc == file_doc {
        return None;
    }

    let snap_body = agent_doc_core::frontmatter::parse(snapshot_doc)
        .map(|(_, body)| body)
        .unwrap_or(snapshot_doc);
    let file_body = agent_doc_core::frontmatter::parse(file_doc)
        .map(|(_, body)| body)
        .unwrap_or(file_doc);

    if redact_component_contents_for_absorb(snap_body)?
        != redact_component_contents_for_absorb(file_body)?
    {
        return None;
    }

    let snap_components = agent_doc_element::element::parse(snap_body).ok()?;
    let file_components = agent_doc_element::element::parse(file_body).ok()?;
    if snap_components.len() != file_components.len() {
        return None;
    }

    let mut saw_exchange = false;
    let mut saw_pending = false;
    let mut saw_status = false;

    for (snap_comp, file_comp) in snap_components.iter().zip(file_components.iter()) {
        if snap_comp.name != file_comp.name {
            return None;
        }
        // Backlog/pending components tolerate patch attr differences (deprecated attr being stripped)
        if !is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != file_comp.patch_mode()
        {
            return None;
        }

        let snap_content = normalize_component_content_for_absorb(snap_comp.content(snap_body));
        let file_content = normalize_component_content_for_absorb(file_comp.content(file_body));
        if snap_content == file_content {
            continue;
        }

        match snap_comp.name.as_str() {
            "exchange" => {
                let safe_exchange =
                    is_safe_out_of_band_exchange_growth(&snap_content, &file_content)
                        || (allow_historical_exchange_growth
                            && is_safe_historical_exchange_growth(&snap_content, &file_content))
                        || is_safe_exchange_user_prompt_insert(&snap_content, &file_content);
                if !safe_exchange {
                    return None;
                }
                saw_exchange = true;
            }
            name if is_backlog_component(name) => {
                if !is_safe_out_of_band_pending_mutation(&snap_content, &file_content) {
                    return None;
                }
                saw_pending = true;
            }
            "status" => {
                if !is_safe_out_of_band_status_mutation(&snap_content, &file_content) {
                    return None;
                }
                saw_status = true;
            }
            _ => return None,
        }
    }

    match (saw_status, saw_exchange, saw_pending) {
        (true, true, true) => Some("status+exchange+pending"),
        (true, true, false) => Some("status+exchange"),
        (true, false, true) => Some("status+pending"),
        (true, false, false) => Some("status"),
        (false, true, true) => Some("exchange+pending"),
        (false, true, false) => Some("exchange"),
        (false, false, true) => Some("pending"),
        (false, false, false) => None,
    }
}

pub fn classify_safe_out_of_band_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    classify_safe_agent_doc_mutation(snapshot_doc, file_doc, false)
}

pub(crate) fn classify_committed_historical_agent_doc_mutation(
    snapshot_doc: &str,
    file_doc: &str,
) -> Option<&'static str> {
    classify_safe_agent_doc_mutation(snapshot_doc, file_doc, true)
}

#[cfg(test)]
mod tests {
    #![allow(unused_imports)]
    use super::*;
    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_exchange_and_pending() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";
        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n\
            <!-- agent:pending -->\n\
            - [ ] [#c3d4] new pending\n\
            - [ ] [#a1b2] existing\n\
            <!-- /agent:pending -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            Some("exchange+pending")
        );
    }
    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_user_prompt_append() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ❯ follow-up question\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }
    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_status_and_exchange() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Older status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            <!-- agent:boundary:oldid -->\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent: codex\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Newer status\n\
            <!-- /agent:status -->\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:newid -->\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            Some("status+exchange")
        );
    }
    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps\n\
            <!-- /agent:status -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }
    #[test]
    fn classify_safe_out_of_band_agent_doc_mutation_rejects_status_prompt_preset_reference_with_guidance()
     {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            Compacted.\n\
            <!-- /agent:status -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:status patch=replace -->\n\
            #next-steps for calibrating session benchmarks with expected scores\n\
            <!-- /agent:status -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }
    #[test]
    fn is_safe_historical_exchange_growth_allows_prompt_target_before_response() {
        let snapshot = "### Re: older\nold body\n";
        let head = "### Re: older\nold body\n\ndo #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` — codex\nCompleted.\n";

        assert!(is_safe_historical_exchange_insert_block(
            "do #7mqc. spec-test-news-commit-push\n### Re: do `#7mqc` — codex\nCompleted."
        ));
        assert!(is_safe_historical_exchange_growth(snapshot, head));
    }
    #[test]
    fn classify_safe_committed_historical_agent_doc_mutation_exchange() {
        let snapshot = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";
        let file = "---\nagent_doc_session: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: historical\n\
            repaired body\n\
            #### #next-steps\n\
            Follow up.\n\
            ### Re: newer\n\
            new body\n\
            <!-- /agent:exchange -->\n";

        assert_eq!(
            classify_safe_committed_historical_agent_doc_mutation(snapshot, file),
            Some("exchange")
        );
        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot, file),
            None
        );
    }
    #[test]
    fn safe_exchange_user_prompt_insert_basic() {
        let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        let file = "### Re: prev — model\nprev response\nUSER PROMPT\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }
    #[test]
    fn safe_exchange_user_prompt_insert_rejects_after_response() {
        let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        let file = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response\nEXTRA TEXT";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }
    #[test]
    fn safe_exchange_user_prompt_insert_rejects_deletions() {
        let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        let file =
            "### Re: prev — model\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }
    #[test]
    fn safe_exchange_user_prompt_insert_rejects_agent_markers() {
        let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        let file = "### Re: prev — model\nprev response\n### Re: injected — model\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, file));
    }
    #[test]
    fn safe_exchange_user_prompt_insert_no_boundary() {
        let snapshot = "### Re: new — model\nnew response";
        let file = "USER PROMPT\n### Re: new — model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }
    #[test]
    fn safe_exchange_user_prompt_insert_identical() {
        let snapshot = "### Re: prev — model\nprev response\n### Re: new — model\nnew response";
        assert!(!is_safe_exchange_user_prompt_insert(snapshot, snapshot));
    }
    #[test]
    fn safe_exchange_user_prompt_insert_multiline_prompts() {
        let snapshot = "### Re: prev — model\nprev response\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        let file = "### Re: prev — model\nprev response\nline one\nline two\nline three\n<!-- agent:boundary:abc -->\n### Re: new — model\nnew response";
        assert!(is_safe_exchange_user_prompt_insert(snapshot, file));
    }
    #[test]
    fn safe_exchange_user_prompt_insert_classify_integration() {
        let snapshot_doc = "---\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prev — model\nprev response\n\
            <!-- agent:boundary:abc -->\n\
            ### Re: new — model\nnew response\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] item\n\
            <!-- /agent:backlog -->\n";

        let file_doc = "---\nagent_doc_format: template\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: prev — model\nprev response\n\
            USER PROMPT\n\
            <!-- agent:boundary:abc -->\n\
            ### Re: new — model\nnew response\n\
            <!-- /agent:exchange -->\n\n\
            <!-- agent:backlog -->\n\
            - [ ] item\n\
            <!-- /agent:backlog -->\n";

        assert_eq!(
            classify_safe_out_of_band_agent_doc_mutation(snapshot_doc, file_doc),
            Some("exchange")
        );
    }
}
