use agent_doc_element::element;
use agent_doc_element_backlog::backlog;
use agent_doc_session_accretion::{SessionAccretionReport, level_label};

pub mod dynamic_context;
pub mod loaded_context;

const BACKLOG_HEAD_LIMIT: usize = 3;
pub const RECENT_EXCHANGE_TURNS_LIMIT: usize = 2;
const FORMAT_REQUIREMENT_COMPONENT_SIGNALS: &[&str] = &[
    "backlog", "pending", "todo", "icebox", "exchange", "response",
];
const FORMAT_REQUIREMENT_SHAPE_SIGNALS: &[&str] = &[
    "2-level",
    "two-level",
    "two level",
    "numeric list",
    "numbered list",
    "bullet list",
    "organize",
    "format",
    "grouped",
    "section",
    "sections",
    "heading",
    "headings",
    "priority",
    "top",
];

#[derive(Debug, Clone)]
struct ResponseSection {
    start_line: usize,
    end_line: usize,
    text: String,
}

pub struct BoundedResponseContext<'a> {
    pub components: &'a [element::Component],
    pub doc: &'a str,
    pub report: &'a SessionAccretionReport,
    pub prompt_targets: &'a [String],
    pub response_toc: &'a str,
    pub remote_host_scope: &'a str,
}

pub struct DocumentSectionContext<'a> {
    pub doc: &'a str,
    pub report: Option<&'a SessionAccretionReport>,
    pub prompt_targets: &'a [String],
    pub response_toc: Option<&'a str>,
    pub remote_host_scope: &'a str,
}

pub struct AgentPromptContext<'a> {
    pub template_mode: bool,
    pub diff_text: &'a str,
    pub doc: &'a str,
    pub document_section: &'a str,
}

pub struct StreamingAgentPromptContext<'a> {
    pub resuming: bool,
    pub diff_text: &'a str,
    pub doc: &'a str,
    pub document_section: &'a str,
}

pub fn render_agent_prompt(input: AgentPromptContext<'_>) -> String {
    let prompt_bearing = agent_doc_diff::format_prompt_bearing_changes(input.diff_text)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements = format_active_format_requirements(input.doc)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();

    if input.template_mode {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            input.diff_text, prompt_bearing, active_format_requirements, input.document_section
        )
    } else {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content. Write your response in markdown.\n\
             Do not include a ## Assistant heading — it will be added automatically.\n\
             If the user inserted prompt-bearing edits inline, classify them as prompt targets vs content edits before responding.",
            input.diff_text, prompt_bearing, active_format_requirements, input.document_section
        )
    }
}

pub fn render_streaming_agent_prompt(input: StreamingAgentPromptContext<'_>) -> String {
    let prompt_bearing_changes = agent_doc_diff::format_prompt_bearing_changes(input.diff_text)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();
    let active_format_requirements = format_active_format_requirements(input.doc)
        .map(|section| format!("\n\n{}\n", section))
        .unwrap_or_default();

    if input.resuming {
        format!(
            "The user edited the session document. Here is the diff since the last run:\n\n\
             <diff>\n{}\n</diff>\n\n\
             {}{}\
             {}\
             Respond to the user's new content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            input.diff_text,
            prompt_bearing_changes,
            active_format_requirements,
            input.document_section
        )
    } else {
        format!(
            "The user is starting a session document. Here is the full document:\n\n\
             {}\
             <document>\n{}\n</document>\n\n\
             Respond to the user's content. Write your response in markdown.\n\
             Format your response as patch blocks targeting document components.\n\
             Example: <!-- patch:exchange -->\\nYour response\\n<!-- /patch:exchange -->",
            active_format_requirements, input.doc
        )
    }
}

pub fn render_full_document_section(doc: &str, remote_host_scope: &str) -> String {
    format!(
        "{}The full document is now:\n\n<document>\n{}\n</document>\n\n",
        remote_host_scope, doc
    )
}

pub fn render_remote_host_scope(required_ssh_targets: &[String]) -> String {
    let declared = if required_ssh_targets.is_empty() {
        "No required SSH targets are declared for this document.".to_string()
    } else {
        format!(
            "Declared required SSH targets for this document: {}.",
            required_ssh_targets.join(", ")
        )
    };

    format!(
        "<remote_host_scope>\n\
         {declared}\n\
         Globally approved SSH commands, ambient SSH config, and unrelated project history are not evidence that a named remote host belongs to this document's project. Use a named remote host only when the current user prompt, this session document/frontmatter, project-local `.agent-doc/config.toml`, or project-local runbooks explicitly identify it; otherwise ask or record a follow-up to confirm the intended host.\n\
         </remote_host_scope>\n\n",
    )
}

pub fn document_section_needs_response_toc(
    doc: &str,
    report: Option<&SessionAccretionReport>,
    prompt_targets: &[String],
) -> bool {
    should_render_bounded_document_section(report, prompt_targets) && element::parse(doc).is_ok()
}

pub fn render_document_section(input: DocumentSectionContext<'_>) -> String {
    if !should_render_bounded_document_section(input.report, input.prompt_targets) {
        return render_full_document_section(input.doc, input.remote_host_scope);
    }

    let Ok(components) = element::parse(input.doc) else {
        return render_full_document_section(input.doc, input.remote_host_scope);
    };

    let response_toc = input
        .response_toc
        .unwrap_or("No live or archived response TOC entries are available yet.");
    let report = input
        .report
        .expect("bounded document section requires an accretion report");
    render_bounded_response_context(BoundedResponseContext {
        components: &components,
        doc: input.doc,
        report,
        prompt_targets: input.prompt_targets,
        response_toc,
        remote_host_scope: input.remote_host_scope,
    })
}

pub fn format_active_format_requirements(content: &str) -> Option<String> {
    let requirements = collect_active_format_requirements(content);
    if requirements.is_empty() {
        return None;
    }

    let mut rendered = String::from(
        "Active document-level formatting / structure requirements carried forward from earlier user prompts:\n\
         Preserve these when they still apply to the component or response you are updating.\n\
         If your response-format rules prevent an exact match, say that explicitly instead of silently flattening the structure.\n\n",
    );
    for (idx, requirement) in requirements.iter().enumerate() {
        rendered.push_str(&format!(
            "<requirement index=\"{}\">\n{}\n</requirement>\n\n",
            idx + 1,
            requirement
        ));
    }
    Some(rendered)
}

pub fn render_bounded_response_context(input: BoundedResponseContext<'_>) -> String {
    let session_summary = extract_session_summary(input.components, input.doc)
        .unwrap_or_else(|| "No explicit `### Session Summary` block is present yet.".to_string());
    let backlog_head = render_backlog_head(input.components, input.doc)
        .unwrap_or_else(|| "No active backlog items found.".to_string());
    let recent_exchange_turns =
        render_recent_exchange_turns(input.components, input.doc, input.prompt_targets)
            .unwrap_or_else(|| {
                "No earlier `### Re:` turns are available in the current exchange.".to_string()
            });
    let available_components = render_available_components(input.components);

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
        level_label(input.report.level),
        input.remote_host_scope,
        level_label(input.report.level),
        render_prompt_targets(input.prompt_targets),
        session_summary.trim_end(),
        backlog_head.trim_end(),
        input.response_toc.trim_end(),
        RECENT_EXCHANGE_TURNS_LIMIT,
        recent_exchange_turns.trim_end(),
        available_components.trim_end(),
    );
    if let Some(reason) = input.report.reasons.first() {
        rendered.insert_str(
            0,
            &format!("Accretion reason: {}.\n\n", reason.trim_end_matches('.')),
        );
    }
    rendered
}

fn should_render_bounded_document_section(
    report: Option<&SessionAccretionReport>,
    prompt_targets: &[String],
) -> bool {
    let Some(report) = report else {
        return false;
    };
    !prompt_targets.is_empty() && !report.is_healthy()
}

fn collect_active_format_requirements(content: &str) -> Vec<String> {
    let mut requirements = Vec::new();
    let mut current_block = Vec::new();

    for raw_line in content.lines() {
        let trimmed_start = raw_line.trim_start();
        if let Some(rest) = trimmed_start.strip_prefix('❯') {
            if !current_block.is_empty() {
                maybe_push_format_requirement(&mut requirements, &current_block.join("\n"));
                current_block.clear();
            }
            let line = rest.trim_start();
            if !line.is_empty() {
                current_block.push(line.to_string());
            }
            continue;
        }

        if current_block.is_empty() {
            continue;
        }

        if trimmed_start.is_empty() {
            maybe_push_format_requirement(&mut requirements, &current_block.join("\n"));
            current_block.clear();
            continue;
        }

        if raw_line.starts_with(' ') || raw_line.starts_with('\t') {
            current_block.push(trimmed_start.to_string());
            continue;
        }

        maybe_push_format_requirement(&mut requirements, &current_block.join("\n"));
        current_block.clear();
    }

    if !current_block.is_empty() {
        maybe_push_format_requirement(&mut requirements, &current_block.join("\n"));
    }

    requirements
}

fn maybe_push_format_requirement(requirements: &mut Vec<String>, block: &str) {
    if looks_like_format_requirement(block)
        && !requirements.iter().any(|existing| existing == block)
    {
        requirements.push(block.to_string());
    }
}

fn looks_like_format_requirement(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    let references_component = FORMAT_REQUIREMENT_COMPONENT_SIGNALS
        .iter()
        .any(|signal| lower.contains(signal));
    let references_shape = FORMAT_REQUIREMENT_SHAPE_SIGNALS
        .iter()
        .any(|signal| lower.contains(signal));
    let imperative_shape = lower.contains("use a ")
        || lower.contains("use an ")
        || lower.contains("keep ")
        || lower.contains("place ")
        || lower.contains("preserve ");

    references_component && (references_shape || imperative_shape)
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
    let (_, items, _) = backlog::parse_items(backlog.content(doc));
    let active: Vec<&backlog::PendingItem> = items.iter().filter(|item| !item.is_done()).collect();
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

fn format_pending_head(item: &backlog::PendingItem) -> String {
    let checkbox = match (&item.state, &item.gate_type) {
        (backlog::PendingState::Gated, Some(gt)) => format!("[/{}]", gt),
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
        .strip_prefix("\u{276f} ")
        .or_else(|| line.trim().strip_prefix('\u{276f}').map(str::trim_start))
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

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_session_accretion::SessionAccretionLevel;

    fn warn_report() -> SessionAccretionReport {
        SessionAccretionReport {
            level: SessionAccretionLevel::Warn,
            reasons: vec!["document hit 2 no-op closeouts in the last 30 minutes".to_string()],
            ..Default::default()
        }
    }

    fn parse_components(doc: &str) -> Vec<element::Component> {
        element::parse(doc).expect("test document should parse")
    }

    fn context_doc() -> &'static str {
        concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: current topic - gpt-5\n\n",
            "Older response body.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#ctxpack] Add bounded context pack\n",
            "- [ ] [#noopcap] Collapse noop churn\n",
            "- [ ] [#chkptcap] Capture checkpoints\n",
            "- [ ] [#later] Fourth item\n",
            "<!-- /agent:backlog -->\n",
        )
    }

    #[test]
    fn format_active_format_requirements_surfaces_prior_backlog_shape_directive() {
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization - gpt-5\n",
            "I reorganized the backlog.\n",
        );

        let rendered =
            format_active_format_requirements(doc).expect("expected formatting requirements");

        assert!(
            rendered.contains(
                "Active document-level formatting / structure requirements carried forward"
            )
        );
        assert!(rendered.contains(
            "Please organize the backlog into a 2-level list. Place the urgent-security matters at the top. Use a numeric list where appropriate."
        ));
        assert!(
            rendered.contains(
                "If your response-format rules prevent an exact match, say that explicitly"
            )
        );
    }

    #[test]
    fn format_active_format_requirements_ignores_agent_confirmation_prose() {
        let doc = concat!(
            "### Re: backlog organization - gpt-5\n",
            "I reorganized the backlog into numbered sections with urgent work at the top.\n",
        );

        assert!(format_active_format_requirements(doc).is_none());
    }

    #[test]
    fn render_agent_prompt_carries_forward_active_format_requirements() {
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization - gpt-5\n",
            "Done.\n",
        );

        let prompt = render_agent_prompt(AgentPromptContext {
            template_mode: true,
            diff_text: "diff",
            doc,
            document_section: "<document>doc</document>",
        });
        assert!(
            prompt.contains(
                "Active document-level formatting / structure requirements carried forward"
            )
        );
        assert!(prompt.contains(
            "Please organize the backlog into a 2-level list. Place the urgent-security matters at the top. Use a numeric list where appropriate."
        ));
        assert!(prompt.contains("Format your response as patch blocks"));
    }

    #[test]
    fn render_streaming_agent_prompt_first_submit_uses_full_document_without_diff() {
        let prompt = render_streaming_agent_prompt(StreamingAgentPromptContext {
            resuming: false,
            diff_text: "diff here",
            doc: "doc content",
            document_section: "<document>ignored</document>",
        });

        assert!(prompt.contains("starting a session"));
        assert!(prompt.contains("doc content"));
        assert!(!prompt.contains("diff here"));
    }

    #[test]
    fn render_streaming_agent_prompt_resume_uses_diff_and_document_section() {
        let prompt = render_streaming_agent_prompt(StreamingAgentPromptContext {
            resuming: true,
            diff_text: "diff here",
            doc: "doc content",
            document_section: "The full document is now:\n\n<document>\ndoc content\n</document>\n\n",
        });

        assert!(prompt.contains("edited the session document"));
        assert!(prompt.contains("diff here"));
        assert!(prompt.contains("doc content"));
        assert!(prompt.contains("patch:exchange"));
    }

    #[test]
    fn render_streaming_agent_prompt_resume_lists_required_response_targets() {
        let diff = "--- snapshot\n+++ document\n@@ -1 +1,5 @@\n\
ctx\n\
+❯ First unresolved question?\n\
+\n\
+❯ Second unresolved question?\n";
        let prompt = render_streaming_agent_prompt(StreamingAgentPromptContext {
            resuming: true,
            diff_text: diff,
            doc: "doc content",
            document_section: "<document>doc content</document>\n\n",
        });

        assert!(prompt.contains("User-authored prompt-bearing changes (oldest first):"));
        assert!(prompt.contains("Do not stop at the newest question"));
        assert!(prompt.contains("kind=\"prompt_target\""));
        assert!(prompt.contains("❯ First unresolved question?"));
        assert!(prompt.contains("❯ Second unresolved question?"));
    }

    #[test]
    fn render_streaming_agent_prompt_carries_forward_active_format_requirements() {
        let doc = concat!(
            "❯ Please organize the backlog into a 2-level list. ",
            "Place the urgent-security matters at the top. ",
            "Use a numeric list where appropriate.\n",
            "### Re: backlog organization - gpt-5\n",
            "Done.\n",
        );

        let prompt = render_streaming_agent_prompt(StreamingAgentPromptContext {
            resuming: true,
            diff_text: "diff",
            doc,
            document_section: "<document>doc</document>\n\n",
        });

        assert!(
            prompt.contains(
                "Active document-level formatting / structure requirements carried forward"
            )
        );
        assert!(prompt.contains(
            "Please organize the backlog into a 2-level list. Place the urgent-security matters at the top. Use a numeric list where appropriate."
        ));
    }

    #[test]
    fn render_streaming_agent_prompt_includes_bounded_document_section() {
        let prompt_targets = vec!["do [#ctxpack]. spec-test-build-install-commit-push".to_string()];
        let document_section = render_document_section(DocumentSectionContext {
            doc: context_doc(),
            report: Some(&warn_report()),
            prompt_targets: &prompt_targets,
            response_toc: None,
            remote_host_scope: "<remote_host_scope>\nNo targets.\n</remote_host_scope>\n\n",
        });
        let diff = "--- snapshot\n+++ document\n@@ -1,3 +1,4 @@\n\
Done.\n\
+do [#ctxpack]. spec-test-build-install-commit-push\n\
<!-- /agent:exchange -->\n";
        let prompt = render_streaming_agent_prompt(StreamingAgentPromptContext {
            resuming: true,
            diff_text: diff,
            doc: context_doc(),
            document_section: &document_section,
        });

        assert!(prompt.contains("<response_context level=\"warn\">"));
        assert!(prompt.contains("do [#ctxpack]. spec-test-build-install-commit-push"));
        assert!(prompt.contains("No live or archived response TOC entries are available yet."));
    }

    #[test]
    fn render_remote_host_scope_lists_no_declared_targets() {
        let scope = render_remote_host_scope(&[]);

        assert!(scope.contains("<remote_host_scope>"));
        assert!(scope.contains("No required SSH targets are declared"));
        assert!(scope.contains("Globally approved SSH commands"));
        assert!(scope.contains("ambient SSH config"));
    }

    #[test]
    fn render_remote_host_scope_lists_declared_targets() {
        let scope = render_remote_host_scope(&[
            "buildparty-worker".to_string(),
            "sampleorders-server".to_string(),
        ]);

        assert!(
            scope.contains(
                "Declared required SSH targets for this document: buildparty-worker, sampleorders-server."
            ),
            "{scope}"
        );
        assert!(scope.contains("unrelated project history"));
    }

    #[test]
    fn render_bounded_response_context_includes_pure_context_sections() {
        let doc = context_doc();
        let components = parse_components(doc);
        let prompt_targets = vec!["do [#ctxpack]. spec-test-build-install-commit-push".to_string()];
        let section = render_bounded_response_context(BoundedResponseContext {
            components: &components,
            doc,
            report: &warn_report(),
            prompt_targets: &prompt_targets,
            response_toc: "- current topic",
            remote_host_scope: "<remote_host_scope>\nNo targets.\n</remote_host_scope>\n\n",
        });

        assert!(section.contains("warn-level context accretion"));
        assert!(section.contains("<response_context level=\"warn\">"));
        assert!(section.contains("do [#ctxpack]. spec-test-build-install-commit-push"));
        assert!(section.contains("### Session Summary\n\nCompacted earlier turns."));
        assert!(section.contains("- [ ] [#ctxpack] Add bounded context pack"));
        assert!(section.contains("- 1 more active item(s)"));
        assert!(section.contains("<response_toc"));
        assert!(section.contains("- current topic"));
        assert!(section.contains("<recent_exchange_turns limit=\"2\">"));
        assert!(section.contains("### Re: current topic - gpt-5"));
        assert!(section.contains("Older response body."));
        assert!(section.contains("ask for more previous turns"));
        assert!(section.contains("available_components"));
        assert!(section.contains("<remote_host_scope>"));
    }

    #[test]
    fn render_document_section_falls_back_to_full_document_when_healthy() {
        let prompt_targets = vec!["do [#ctxpack]. spec-test-build-install-commit-push".to_string()];
        let section = render_document_section(DocumentSectionContext {
            doc: "doc body",
            report: Some(&SessionAccretionReport::default()),
            prompt_targets: &prompt_targets,
            response_toc: Some("- current topic"),
            remote_host_scope: "<remote_host_scope>\nNo targets.\n</remote_host_scope>\n\n",
        });

        assert!(section.contains("The full document is now:"));
        assert!(section.contains("<document>\ndoc body\n</document>"));
        assert!(section.contains("<remote_host_scope>"));
        assert!(!section.contains("<response_context"));
        assert!(!document_section_needs_response_toc(
            "doc body",
            Some(&SessionAccretionReport::default()),
            &prompt_targets
        ));
    }

    #[test]
    fn render_document_section_falls_back_to_full_document_without_prompt_targets() {
        let section = render_document_section(DocumentSectionContext {
            doc: "doc body",
            report: Some(&warn_report()),
            prompt_targets: &[],
            response_toc: None,
            remote_host_scope: "",
        });

        assert!(section.contains("The full document is now:"));
        assert!(!section.contains("<response_context"));
        assert!(!document_section_needs_response_toc(
            "doc body",
            Some(&warn_report()),
            &[]
        ));
    }

    #[test]
    fn render_document_section_uses_bounded_context_for_warn_prompt_targets() {
        let prompt_targets = vec!["do [#ctxpack]. spec-test-build-install-commit-push".to_string()];
        let section = render_document_section(DocumentSectionContext {
            doc: context_doc(),
            report: Some(&warn_report()),
            prompt_targets: &prompt_targets,
            response_toc: None,
            remote_host_scope: "<remote_host_scope>\nNo targets.\n</remote_host_scope>\n\n",
        });

        assert!(document_section_needs_response_toc(
            context_doc(),
            Some(&warn_report()),
            &prompt_targets
        ));
        assert!(section.contains("warn-level context accretion"));
        assert!(section.contains("<response_context level=\"warn\">"));
        assert!(section.contains("do [#ctxpack]. spec-test-build-install-commit-push"));
        assert!(section.contains("### Session Summary\n\nCompacted earlier turns."));
        assert!(section.contains("- [ ] [#ctxpack] Add bounded context pack"));
        assert!(section.contains("No live or archived response TOC entries are available yet."));
        assert!(section.contains("<remote_host_scope>"));
    }

    #[test]
    fn render_bounded_response_context_anchors_tail_prompt_to_immediately_previous_response() {
        let doc = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: older topic - gpt-5\n\n",
            "Old response body.\n\n",
            "### Re: latest topic - gpt-5\n\n",
            "Latest response body.\n",
            "do [#tailctx]. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending / Not Built\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#tailctx] Tail follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = parse_components(doc);
        let prompt_targets = vec!["do [#tailctx]. spec-test-build-install-commit-push".to_string()];
        let section = render_bounded_response_context(BoundedResponseContext {
            components: &components,
            doc,
            report: &warn_report(),
            prompt_targets: &prompt_targets,
            response_toc: "- latest topic",
            remote_host_scope: "",
        });

        assert!(section.contains("### Re: latest topic - gpt-5"));
        assert!(section.contains("Latest response body."));
        assert!(!section.contains("Old response body."));
    }

    #[test]
    fn collect_recent_exchange_turn_sections_anchors_inline_prompt_edit_to_enclosing_response() {
        let exchange = concat!(
            "### Session Summary\n\n",
            "Compacted earlier turns.\n\n",
            "### Re: earlier topic - gpt-5\n\n",
            "Earlier response body.\n",
            "do [#inlinectx]. spec-test-build-install-commit-push\n\n",
            "### Re: latest topic - gpt-5\n\n",
            "Latest response body.\n",
        );

        let sections = collect_recent_exchange_turn_sections(
            exchange,
            &["do [#inlinectx]. spec-test-build-install-commit-push".to_string()],
        );
        assert_eq!(sections.len(), 1);
        assert!(sections[0].contains("### Re: earlier topic - gpt-5"));
        assert!(sections[0].contains("Earlier response body."));
        assert!(!sections[0].contains("Latest response body."));
    }
}
