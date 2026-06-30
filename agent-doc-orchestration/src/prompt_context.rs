use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_prompt_context::{
    BoundedResponseContext, render_bounded_response_context, render_full_document_section,
};
#[cfg(test)]
use agent_doc_session_accretion::SessionAccretionLevel;
use agent_doc_session_accretion::SessionAccretionReport;
use agent_doc_workflow::session_cycle::prompt_targets_from_diff;

use crate::frontmatter_io;
use std::path::Path;

pub fn build_document_section(
    file: &Path,
    diff_text: &str,
    doc: &str,
    report: Option<&SessionAccretionReport>,
) -> String {
    let prompt_targets = prompt_targets_from_diff(diff_text);
    let remote_host_scope = render_remote_host_scope(file, doc);
    let Some(report) = report else {
        return render_full_document_section(doc, &remote_host_scope);
    };
    if prompt_targets.is_empty() || report.is_healthy() {
        return render_full_document_section(doc, &remote_host_scope);
    }

    let Ok(components) = element::parse(doc) else {
        return render_full_document_section(doc, &remote_host_scope);
    };

    let response_toc = crate::response_toc::render_prompt_toc(file, doc, &prompt_targets)
        .unwrap_or_else(|| {
            "No live or archived response TOC entries are available yet.".to_string()
        });

    render_bounded_response_context(BoundedResponseContext {
        components: &components,
        doc,
        report,
        prompt_targets: &prompt_targets,
        response_toc: &response_toc,
        remote_host_scope: &remote_host_scope,
    })
}

fn render_remote_host_scope(file: &Path, doc: &str) -> String {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let declared_targets = frontmatter_io::parse_for_file_with_context(doc, file, &rc)
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

#[cfg(test)]
mod tests {
    use super::*;

    fn warn_report() -> SessionAccretionReport {
        SessionAccretionReport {
            level: SessionAccretionLevel::Warn,
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
            Some(&SessionAccretionReport::default()),
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
            Some(&SessionAccretionReport::default()),
        );

        assert!(
            section.contains("Declared required SSH targets for this document: buildparty-worker.")
        );
        assert!(section.contains("unrelated project history"));
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
