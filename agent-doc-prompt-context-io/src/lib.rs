use agent_doc_frontmatter::frontmatter;
use agent_doc_frontmatter_io::session::ResolvedSshContext;
use agent_doc_prompt_context::{
    DocumentSectionContext, document_section_needs_response_toc, render_document_section,
    render_remote_host_scope,
};
#[cfg(test)]
use agent_doc_session_accretion::SessionAccretionLevel;
use agent_doc_session_accretion::SessionAccretionReport;
use agent_doc_workflow::session_cycle::prompt_targets_from_diff;

use std::path::Path;

pub mod dynamic_context;

pub fn build_document_section(
    file: &Path,
    diff_text: &str,
    doc: &str,
    report: Option<&SessionAccretionReport>,
) -> String {
    build_document_section_with_remote_host_scope(
        file,
        diff_text,
        doc,
        report,
        &remote_host_scope_for_file(file, doc),
    )
}

/// Build the document section using a cached SSH/project context.
pub fn build_document_section_with_ssh_context(
    file: &Path,
    diff_text: &str,
    doc: &str,
    report: Option<&SessionAccretionReport>,
    ssh: &ResolvedSshContext,
) -> String {
    build_document_section_with_remote_host_scope(
        file,
        diff_text,
        doc,
        report,
        &remote_host_scope_for_file_with_context(file, doc, ssh),
    )
}

fn build_document_section_with_remote_host_scope(
    file: &Path,
    diff_text: &str,
    doc: &str,
    report: Option<&SessionAccretionReport>,
    remote_host_scope: &str,
) -> String {
    let prompt_targets = prompt_targets_from_diff(diff_text);
    let response_toc = document_section_needs_response_toc(doc, report, &prompt_targets)
        .then(|| agent_doc_response_toc_io::render_prompt_toc(file, doc, &prompt_targets))
        .flatten();

    let mut section = render_document_section(DocumentSectionContext {
        doc,
        report,
        prompt_targets: &prompt_targets,
        response_toc: response_toc.as_deref(),
        remote_host_scope,
    });
    if let Ok(Some(snapshot)) =
        dynamic_context::build_dynamic_context_snapshot(file, doc, &prompt_targets)
        && let Some(dynamic_context) = snapshot.as_prompt_section()
    {
        section.push_str("\n\n");
        section.push_str(&dynamic_context);
    }
    section
}

fn remote_host_scope_for_file(file: &Path, doc: &str) -> String {
    let declared_targets = agent_doc_frontmatter_io::session::parse_for_file(doc, file)
        .or_else(|_| frontmatter::parse(doc))
        .ok()
        .map(|(fm, _)| fm.required_ssh_targets)
        .unwrap_or_default();
    render_remote_host_scope(&declared_targets)
}

fn remote_host_scope_for_file_with_context(
    file: &Path,
    doc: &str,
    ssh: &ResolvedSshContext,
) -> String {
    let declared_targets =
        agent_doc_frontmatter_io::session::parse_for_file_with_context(doc, file, ssh)
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
