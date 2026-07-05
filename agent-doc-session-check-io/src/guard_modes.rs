use std::path::Path;

use agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode;
use agent_doc_run_context_io::RunContext;
use anyhow::Result;

pub fn resolve_pending_capture_guard_mode(file: &Path) -> Result<PendingCaptureGuardMode> {
    let content = crate::resolve_current_document_content(file, "pending_capture_guard_mode")?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(
        agent_doc_frontmatter::project_config::resolve_pending_capture_guard_mode(
            &fm,
            &project_config,
        ),
    )
}

pub fn resolve_pending_capture_guard_mode_with_context(
    _file: &Path,
    rc: &RunContext,
) -> Result<PendingCaptureGuardMode> {
    let fm = rc.frontmatter();
    let project_config = rc.project_config();
    Ok(
        agent_doc_frontmatter::project_config::resolve_pending_capture_guard_mode(
            &fm,
            &project_config,
        ),
    )
}

pub fn resolve_pending_done_guard_mode(file: &Path) -> Result<PendingCaptureGuardMode> {
    let content = crate::resolve_current_document_content(file, "pending_done_guard_mode")?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(
        agent_doc_frontmatter::project_config::resolve_pending_done_guard_mode(
            &fm,
            &project_config,
        ),
    )
}

pub fn resolve_pending_done_guard_mode_with_context(
    _file: &Path,
    rc: &RunContext,
) -> Result<PendingCaptureGuardMode> {
    let fm = rc.frontmatter();
    let project_config = rc.project_config();
    Ok(
        agent_doc_frontmatter::project_config::resolve_pending_done_guard_mode(
            &fm,
            &project_config,
        ),
    )
}

pub fn resolve_review_done_guard_mode(file: &Path) -> Result<PendingCaptureGuardMode> {
    let content = crate::resolve_current_document_content(file, "review_done_guard_mode")?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(agent_doc_frontmatter::project_config::resolve_review_done_guard_mode(&fm, &project_config))
}

pub fn resolve_auto_done(file: &Path) -> Result<bool> {
    let content = crate::resolve_current_document_content(file, "auto_done")?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(agent_doc_frontmatter::project_config::resolve_auto_done(
        &fm,
        &project_config,
    ))
}
