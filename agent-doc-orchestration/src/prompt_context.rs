use agent_doc_frontmatter::frontmatter;
use agent_doc_prompt_context::{
    DocumentSectionContext, document_section_needs_response_toc, render_document_section,
    render_remote_host_scope,
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
    let remote_host_scope = remote_host_scope_for_file(file, doc);
    let response_toc = document_section_needs_response_toc(doc, report, &prompt_targets)
        .then(|| agent_doc_response_toc_io::render_prompt_toc(file, doc, &prompt_targets))
        .flatten();

    render_document_section(DocumentSectionContext {
        doc,
        report,
        prompt_targets: &prompt_targets,
        response_toc: response_toc.as_deref(),
        remote_host_scope: &remote_host_scope,
    })
}

fn remote_host_scope_for_file(file: &Path, doc: &str) -> String {
    let rc = crate::graph::RunContext::new(file.to_path_buf());
    let declared_targets = frontmatter_io::parse_for_file_with_context(doc, file, &rc)
        .or_else(|_| frontmatter::parse(doc))
        .ok()
        .map(|(fm, _)| fm.required_ssh_targets)
        .unwrap_or_default();
    render_remote_host_scope(&declared_targets)
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
