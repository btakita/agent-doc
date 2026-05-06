use crate::{component, diff, pending, session_accretion};

const BACKLOG_HEAD_LIMIT: usize = 3;
const RECENT_EXCHANGE_TURNS_LIMIT: usize = 2;

pub(crate) fn build_document_section(
    diff_text: &str,
    doc: &str,
    report: Option<&session_accretion::SessionAccretionReport>,
) -> String {
    let prompt_targets = extract_prompt_targets(diff_text);
    let Some(report) = report else {
        return full_document_section(doc);
    };
    if prompt_targets.is_empty() || report.is_healthy() {
        return full_document_section(doc);
    }

    let Ok(components) = component::parse(doc) else {
        return full_document_section(doc);
    };

    let session_summary = extract_session_summary(&components, doc)
        .unwrap_or_else(|| "No explicit `### Session Summary` block is present yet.".to_string());
    let backlog_head = render_backlog_head(&components, doc)
        .unwrap_or_else(|| "No active backlog items found.".to_string());
    let recent_exchange_turns =
        render_recent_exchange_turns(&components, doc).unwrap_or_else(|| {
            "No earlier `### Re:` turns are available in the current exchange.".to_string()
        });
    let available_components = render_available_components(&components);

    let mut rendered = format!(
        "The session is showing {}-level context accretion, so the on-disk document stays full length while this prompt uses a bounded recent-context pack instead. If older history becomes necessary, ask for more previous turns instead of assuming hidden context.\n\n\
         <response_context level=\"{}\">\n\
         <prompt_targets oldest_first=\"true\">\n{}\n\
         </prompt_targets>\n\n\
         <session_summary>\n{}\n\
         </session_summary>\n\n\
         <backlog_head>\n{}\n\
         </backlog_head>\n\n\
         <recent_exchange_turns limit=\"{}\">\n{}\n\
         </recent_exchange_turns>\n\n\
         <available_components>\n{}\n\
         </available_components>\n\
         </response_context>\n\n",
        level_name(report.level),
        level_name(report.level),
        render_prompt_targets(&prompt_targets),
        session_summary.trim_end(),
        backlog_head.trim_end(),
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

fn full_document_section(doc: &str) -> String {
    format!(
        "The full document is now:\n\n<document>\n{}\n</document>\n\n",
        doc
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

fn extract_session_summary(components: &[component::Component], doc: &str) -> Option<String> {
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

fn render_backlog_head(components: &[component::Component], doc: &str) -> Option<String> {
    let backlog = components
        .iter()
        .find(|comp| component::is_backlog_component(&comp.name))?;
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

fn render_available_components(components: &[component::Component]) -> String {
    components
        .iter()
        .map(|comp| comp.name.as_str())
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect::<Vec<_>>()
        .join("\n")
}

fn render_recent_exchange_turns(components: &[component::Component], doc: &str) -> Option<String> {
    let exchange = components.iter().find(|comp| comp.name == "exchange")?;
    let sections = collect_recent_exchange_turn_sections(exchange.content(doc));
    if sections.is_empty() {
        return None;
    }
    Some(sections.join("\n\n"))
}

fn collect_recent_exchange_turn_sections(exchange_body: &str) -> Vec<String> {
    let lines: Vec<&str> = exchange_body.lines().collect();
    let mut sections = Vec::new();
    let mut current = Vec::new();

    for line in lines {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<!-- agent:boundary:") {
            break;
        }
        if trimmed.starts_with("### Re:") && !current.is_empty() {
            sections.push(current.join("\n").trim().to_string());
            current.clear();
        }
        if !current.is_empty() || trimmed.starts_with("### Re:") {
            current.push(line);
        }
    }

    if !current.is_empty() {
        sections.push(current.join("\n").trim().to_string());
    }

    let keep_from = sections.len().saturating_sub(RECENT_EXCHANGE_TURNS_LIMIT);
    sections
        .into_iter()
        .skip(keep_from)
        .filter(|section| !section.is_empty())
        .collect()
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
            "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n+❯ Hello\n",
            "doc body",
            Some(&session_accretion::SessionAccretionReport::default()),
        );
        assert!(section.contains("The full document is now:"));
        assert!(section.contains("<document>\ndoc body\n</document>"));
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

        let section = build_document_section(diff, doc, Some(&warn_report()));
        assert!(section.contains("warn-level context accretion"));
        assert!(section.contains("<response_context level=\"warn\">"));
        assert!(section.contains("do [#ctxpack]. spec-test-build-install-commit-push"));
        assert!(section.contains("### Session Summary\n\nCompacted earlier turns."));
        assert!(section.contains("- [ ] [#ctxpack] Add bounded context pack"));
        assert!(section.contains("- 1 more active item(s)"));
        assert!(section.contains("<recent_exchange_turns limit=\"2\">"));
        assert!(section.contains("### Re: current topic — gpt-5"));
        assert!(section.contains("Older response body."));
        assert!(section.contains("ask for more previous turns"));
        assert!(section.contains("available_components"));
    }

    #[test]
    fn build_document_section_keeps_full_document_for_warn_content_edits_without_prompt_targets() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1 @@\n-Old\n+Updated wording.\n";
        let section = build_document_section(diff, "doc body", Some(&warn_report()));
        assert!(section.contains("The full document is now:"));
        assert!(!section.contains("<response_context"));
    }
}
