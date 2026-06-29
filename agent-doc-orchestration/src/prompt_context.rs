use agent_doc_element::element;

use crate::{diff, frontmatter, pending, session_accretion};
use std::path::Path;

const BACKLOG_HEAD_LIMIT: usize = 3;
const RECENT_EXCHANGE_TURNS_LIMIT: usize = 2;

#[derive(Debug, Clone)]
struct ResponseSection {
    start_line: usize,
    end_line: usize,
    text: String,
}

pub fn build_document_section(
    file: &Path,
    diff_text: &str,
    doc: &str,
    report: Option<&session_accretion::SessionAccretionReport>,
) -> String {
    let prompt_targets = extract_prompt_targets(diff_text);
    let remote_host_scope = render_remote_host_scope(file, doc);
    let Some(report) = report else {
        return full_document_section(doc, &remote_host_scope);
    };
    if prompt_targets.is_empty() || report.is_healthy() {
        return full_document_section(doc, &remote_host_scope);
    }

    let Ok(components) = element::parse(doc) else {
        return full_document_section(doc, &remote_host_scope);
    };

    let session_summary = extract_session_summary(&components, doc)
        .unwrap_or_else(|| "No explicit `### Session Summary` block is present yet.".to_string());
    let backlog_head = render_backlog_head(&components, doc)
        .unwrap_or_else(|| "No active backlog items found.".to_string());
    let response_toc = crate::response_toc::render_prompt_toc(file, doc, &prompt_targets)
        .unwrap_or_else(|| {
            "No live or archived response TOC entries are available yet.".to_string()
        });
    let recent_exchange_turns = render_recent_exchange_turns(&components, doc, &prompt_targets)
        .unwrap_or_else(|| {
            "No earlier `### Re:` turns are available in the current exchange.".to_string()
        });
    let available_components = render_available_components(&components);

    let mut rendered = format!(
        "The session is showing {}-level context accretion, so the on-disk document stays full length while this prompt uses a bounded recent-context pack instead. If older history becomes necessary, ask for more previous turns instead of assuming hidden context.\n\n\
         {}\
         <response_context level=\"{}\">\n\
         <prompt_targets oldest_first=\"true\">\n{}\n\
         </prompt_targets>\n\n\
         <session_summary>\n{}\n\
         </session_summary>\n\n\
         <backlog_head>\n{}\n\
         </backlog_head>\n\n\
         <response_toc retrieval_hint=\"agent-doc response-fetch <FILE> --locator <LOCATOR>\">\n{}\n\
         </response_toc>\n\n\
         <recent_exchange_turns limit=\"{}\">\n{}\n\
         </recent_exchange_turns>\n\n\
         <available_components>\n{}\n\
         </available_components>\n\
         </response_context>\n\n",
        level_name(report.level),
        remote_host_scope,
        level_name(report.level),
        render_prompt_targets(&prompt_targets),
        session_summary.trim_end(),
        backlog_head.trim_end(),
        response_toc.trim_end(),
        RECENT_EXCHANGE_TURNS_LIMIT,
        recent_exchange_turns.trim_end(),
        available_components.trim_end(),
    );
    if let Some(reason) = report.reasons.first() {
        rendered.insert_str(
            0,
            &format!("Accretion reason: {}.\n\n", reason.trim_end_matches('.')),
        );
    }
    rendered
}

fn full_document_section(doc: &str, remote_host_scope: &str) -> String {
    format!(
        "{}The full document is now:\n\n<document>\n{}\n</document>\n\n",
        remote_host_scope, doc
    )
}

fn render_remote_host_scope(file: &Path, doc: &str) -> String {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let declared_targets = frontmatter::parse_for_file_with_context(doc, file, &rc)
        .or_else(|_| frontmatter::parse(doc))
        .ok()
        .map(|(fm, _)| fm.required_ssh_targets)
        .unwrap_or_default();
    let declared = if declared_targets.is_empty() {
        "No required SSH targets are declared for this document.".to_string()
    } else {
        format!(
            "Declared required SSH targets for this document: {}.",
            declared_targets.join(", ")
        )
    };

    format!(
        "<remote_host_scope>\n\
         {declared}\n\
         Globally approved SSH commands, ambient SSH config, and unrelated project history are not evidence that a named remote host belongs to this document's project. Use a named remote host only when the current user prompt, this session document/frontmatter, project-local `.agent-doc/config.toml`, or project-local runbooks explicitly identify it; otherwise ask or record a follow-up to confirm the intended host.\n\
         </remote_host_scope>\n\n",
    )
}

fn extract_prompt_targets(diff_text: &str) -> Vec<String> {
    let mut targets: Vec<String> = diff::classify_prompt_bearing_changes(diff_text)
        .into_iter()
        .filter(|change| change.kind == diff::PromptBearingChangeKind::PromptTarget)
        .map(|change| change.text.trim().to_string())
        .filter(|text| !text.is_empty())
        .collect();

    if targets.is_empty() {
        for directive in diff::extract_imperative_directives(diff_text) {
            if !targets.iter().any(|existing| existing == &directive) {
                targets.push(directive);
            }
        }
    }

    targets
}

fn render_prompt_targets(prompt_targets: &[String]) -> String {
    prompt_targets
        .iter()
        .enumerate()
        .map(|(idx, text)| {
            format!(
                "<prompt_target index=\"{}\">\n{}\n</prompt_target>",
                idx + 1,
                text
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn extract_session_summary(components: &[element::Component], doc: &str) -> Option<String> {
    let exchange = components.iter().find(|comp| comp.name == "exchange")?;
    let exchange_body = exchange.content(doc);
    let lines: Vec<&str> = exchange_body.lines().collect();
    let start = lines
        .iter()
        .position(|line| line.trim_end() == "### Session Summary")?;
    let mut end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start + 1) {
        let trimmed = line.trim_start();
        if trimmed.starts_with("### Re:") || trimmed.starts_with("<!-- agent:boundary:") {
            end = idx;
            break;
        }
    }
    let summary = lines[start..end].join("\n").trim().to_string();
    (!summary.is_empty()).then_some(summary)
}

fn render_backlog_head(components: &[element::Component], doc: &str) -> Option<String> {
    let backlog = components
        .iter()
        .find(|comp| element::is_backlog_component(&comp.name))?;
    let (_, items, _) = pending::parse_items(backlog.content(doc));
    let active: Vec<&pending::PendingItem> = items.iter().filter(|item| !item.is_done()).collect();
    if active.is_empty() {
        return None;
    }

    let mut lines = active
        .iter()
        .take(BACKLOG_HEAD_LIMIT)
        .map(|item| format_pending_head(item))
        .collect::<Vec<_>>();
    if active.len() > BACKLOG_HEAD_LIMIT {
        lines.push(format!(
            "- {} more active item(s)",
            active.len() - BACKLOG_HEAD_LIMIT
        ));
    }
    Some(lines.join("\n"))
}

fn format_pending_head(item: &pending::PendingItem) -> String {
    let checkbox = match (&item.state, &item.gate_type) {
        (pending::PendingState::Gated, Some(gt)) => format!("[/{}]", gt),
        _ => format!("[{}]", item.state.box_char()),
    };
    format!("- {} [#{}] {}", checkbox, item.id, item.text)
}

fn render_available_components(components: &[element::Component]) -> String {
    components
        .iter()
        .map(|comp| comp.name.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_recent_exchange_turns(
    components: &[element::Component],
    doc: &str,
    prompt_targets: &[String],
) -> Option<String> {
    let exchange = components.iter().find(|comp| comp.name == "exchange")?;
    let sections = collect_recent_exchange_turn_sections(exchange.content(doc), prompt_targets);
    if sections.is_empty() {
        return None;
    }
    Some(sections.join("\n\n"))
}

fn collect_recent_exchange_turn_sections(
    exchange_body: &str,
    prompt_targets: &[String],
) -> Vec<String> {
    let lines: Vec<&str> = exchange_body.lines().collect();
    let sections = collect_response_sections(&lines);
    if sections.is_empty() {
        return Vec::new();
    }

    let anchored = collect_prompt_anchored_sections(&lines, &sections, prompt_targets);
    if !anchored.is_empty() {
        return anchored;
    }

    collect_recent_sections(&sections)
}

fn collect_response_sections(lines: &[&str]) -> Vec<ResponseSection> {
    let mut sections = Vec::new();
    let mut current = Vec::new();
    let mut current_start = None;

    for (idx, line) in lines.iter().enumerate() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<!-- agent:boundary:") {
            break;
        }
        if trimmed.starts_with("### Re:") && !current.is_empty() {
            sections.push(ResponseSection {
                start_line: current_start.expect("response sections always start with a heading"),
                end_line: idx,
                text: current.join("\n").trim().to_string(),
            });
            current.clear();
            current_start = None;
        }
        if !current.is_empty() || trimmed.starts_with("### Re:") {
            if current_start.is_none() {
                current_start = Some(idx);
            }
            current.push(*line);
        }
    }

    if !current.is_empty() {
        sections.push(ResponseSection {
            start_line: current_start.expect("response sections always start with a heading"),
            end_line: lines.len(),
            text: current.join("\n").trim().to_string(),
        });
    }

    sections
}

fn collect_prompt_anchored_sections(
    lines: &[&str],
    sections: &[ResponseSection],
    prompt_targets: &[String],
) -> Vec<String> {
    let mut selected_indexes = Vec::new();

    for prompt_target in prompt_targets {
        let Some(prompt_line) = find_prompt_target_line(lines, prompt_target) else {
            continue;
        };
        let Some(section_idx) = find_context_section_for_prompt_line(sections, prompt_line) else {
            continue;
        };
        if !selected_indexes.contains(&section_idx) {
            selected_indexes.push(section_idx);
        }
    }

    selected_indexes
        .into_iter()
        .filter_map(|idx| sections.get(idx))
        .map(|section| section.text.clone())
        .filter(|section| !section.is_empty())
        .collect()
}

fn collect_recent_sections(sections: &[ResponseSection]) -> Vec<String> {
    let keep_from = sections.len().saturating_sub(RECENT_EXCHANGE_TURNS_LIMIT);
    sections
        .iter()
        .skip(keep_from)
        .map(|section| section.text.clone())
        .filter(|section| !section.is_empty())
        .collect()
}

fn find_prompt_target_line(lines: &[&str], prompt_target: &str) -> Option<usize> {
    let target_lines = normalize_prompt_target_lines(prompt_target);
    if target_lines.is_empty() {
        return None;
    }

    let normalized_lines: Vec<String> = lines
        .iter()
        .map(|line| normalize_prompt_match_text(line))
        .collect();

    normalized_lines
        .windows(target_lines.len())
        .enumerate()
        .filter_map(|(idx, window)| (window == target_lines.as_slice()).then_some(idx))
        .next_back()
}

fn normalize_prompt_target_lines(prompt_target: &str) -> Vec<String> {
    prompt_target
        .lines()
        .map(normalize_prompt_match_text)
        .filter(|line| !line.is_empty())
        .collect()
}

fn normalize_prompt_match_text(line: &str) -> String {
    line.trim()
        .strip_prefix("❯ ")
        .or_else(|| line.trim().strip_prefix('❯').map(str::trim_start))
        .unwrap_or(line.trim())
        .trim()
        .to_string()
}

fn find_context_section_for_prompt_line(
    sections: &[ResponseSection],
    prompt_line: usize,
) -> Option<usize> {
    if let Some((idx, _)) = sections
        .iter()
        .enumerate()
        .find(|(_, section)| prompt_line >= section.start_line && prompt_line < section.end_line)
    {
        return Some(idx);
    }

    sections
        .iter()
        .enumerate()
        .take_while(|(_, section)| section.end_line <= prompt_line)
        .map(|(idx, _)| idx)
        .last()
}

fn level_name(level: session_accretion::SessionAccretionLevel) -> &'static str {
    match level {
        session_accretion::SessionAccretionLevel::Healthy => "healthy",
        session_accretion::SessionAccretionLevel::Warn => "warn",
        session_accretion::SessionAccretionLevel::Block => "block",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn warn_report() -> session_accretion::SessionAccretionReport {
        session_accretion::SessionAccretionReport {
            level: session_accretion::SessionAccretionLevel::Warn,
            reasons: vec!["document hit 2 no-op closeouts in the last 30 minutes".to_string()],
            ..Default::default()
        }
    }

    #[test]
    fn build_document_section_falls_back_to_full_document_when_healthy() {
        let section = build_document_section(
            Path::new("session.md"),
            "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n+❯ Hello\n",
            "doc body",
            Some(&session_accretion::SessionAccretionReport::default()),
        );
        assert!(section.contains("The full document is now:"));
        assert!(section.contains("<document>\ndoc body\n</document>"));
        assert!(section.contains("<remote_host_scope>"));
        assert!(section.contains("No required SSH targets are declared"));
        assert!(section.contains("Globally approved SSH commands"));
    }

    #[test]
    fn build_document_section_uses_bounded_context_pack_for_warn_prompt_targets() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            Done.\n\
            +do [#ctxpack]. spec-test-build-install-commit-push\n\
            <!-- /agent:exchange -->\n";
        let doc = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: current topic — gpt-5\n\n",
            "Older response body.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ctxpack] Add bounded context pack\n",
            "- [ ] [#noopcap] Collapse noop churn\n",
            "- [ ] [#chkptcap] Capture checkpoints\n",
            "- [ ] [#later] Fourth item\n",
            "<!-- /agent:backlog -->\n",
        );

        let section =
            build_document_section(Path::new("session.md"), diff, doc, Some(&warn_report()));
        assert!(section.contains("warn-level context accretion"));
        assert!(section.contains("<response_context level=\"warn\">"));
        assert!(section.contains("do [#ctxpack]. spec-test-build-install-commit-push"));
        assert!(section.contains("### Session Summary\n\nCompacted earlier turns."));
        assert!(section.contains("- [ ] [#ctxpack] Add bounded context pack"));
        assert!(section.contains("- 1 more active item(s)"));
        assert!(section.contains("<response_toc"));
        assert!(section.contains("response-fetch <FILE> --locator <LOCATOR>"));
        assert!(section.contains("<recent_exchange_turns limit=\"2\">"));
        assert!(section.contains("### Re: current topic — gpt-5"));
        assert!(section.contains("Older response body."));
        assert!(section.contains("ask for more previous turns"));
        assert!(section.contains("available_components"));
        assert!(section.contains("<remote_host_scope>"));
        assert!(section.contains("ambient SSH config"));
    }

    #[test]
    fn build_document_section_lists_declared_required_ssh_targets() {
        let doc = "---\nrequired_ssh_targets:\n  - buildparty-worker\n---\nBody\n";
        let section = build_document_section(
            Path::new("session.md"),
            "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n+❯ Hello\n",
            doc,
            Some(&session_accretion::SessionAccretionReport::default()),
        );

        assert!(
            section.contains("Declared required SSH targets for this document: buildparty-worker.")
        );
        assert!(section.contains("unrelated project history"));
    }

    #[test]
    fn build_document_section_anchors_tail_prompt_to_immediately_previous_response() {
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
            Done.\n\
            +do [#tailctx]. spec-test-build-install-commit-push\n\
            <!-- /agent:exchange -->\n";
        let doc = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: older topic — gpt-5\n\n",
            "Old response body.\n\n",
            "### Re: latest topic — gpt-5\n\n",
            "Latest response body.\n",
            "do [#tailctx]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#tailctx] Tail follow-up\n",
            "<!-- /agent:backlog -->\n",
        );

        let section =
            build_document_section(Path::new("session.md"), diff, doc, Some(&warn_report()));
        assert!(section.contains("### Re: latest topic — gpt-5"));
        assert!(section.contains("Latest response body."));
        assert!(!section.contains("Old response body."));
    }

    #[test]
    fn collect_recent_exchange_turn_sections_anchors_inline_prompt_edit_to_enclosing_response() {
        let exchange = concat!(
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: earlier topic — gpt-5\n\n",
            "Earlier response body.\n",
            "do [#inlinectx]. spec-test-build-install-commit-push\n\n",
            "### Re: latest topic — gpt-5\n\n",
            "Latest response body.\n",
        );

        let sections = collect_recent_exchange_turn_sections(
            exchange,
            &["do [#inlinectx]. spec-test-build-install-commit-push".to_string()],
        );
        assert_eq!(sections.len(), 1);
        assert!(sections[0].contains("### Re: earlier topic — gpt-5"));
        assert!(sections[0].contains("Earlier response body."));
        assert!(!sections[0].contains("Latest response body."));
    }

    #[test]
    fn build_document_section_keeps_full_document_for_warn_content_edits_without_prompt_targets() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1 @@\n-Old\n+Updated wording.\n";
        let section = build_document_section(
            Path::new("session.md"),
            diff,
            "doc body",
            Some(&warn_report()),
        );
        assert!(section.contains("The full document is now:"));
        assert!(!section.contains("<response_context"));
    }
}
