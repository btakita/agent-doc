use std::path::Path;

use agent_doc_frontmatter::frontmatter::PendingCaptureGuardMode;
use agent_doc_frontmatter::project_config::{
    resolve_pending_capture_guard_mode as resolve_pending_capture_mode,
    resolve_pending_done_guard_mode as resolve_pending_done_mode,
    resolve_review_done_guard_mode as resolve_review_done_mode,
};
use agent_doc_run_context_io::{AgentDocContextExt, CycleContext};
use anyhow::Result;

fn frontmatter_for_mode(
    file: &Path,
    source: &str,
    force_disk: bool,
) -> Result<agent_doc_frontmatter::frontmatter::Frontmatter> {
    let content =
        crate::resolve_current_document_content_with_force_disk(file, source, force_disk)?;
    let (fm, _) = agent_doc_frontmatter::frontmatter::parse(&content)?;
    Ok(fm)
}

pub fn resolve_pending_capture_guard_mode(file: &Path) -> Result<PendingCaptureGuardMode> {
    resolve_pending_capture_mode_with_force_disk(file, false)
}

pub fn resolve_pending_capture_mode_with_force_disk(
    file: &Path,
    force_disk: bool,
) -> Result<PendingCaptureGuardMode> {
    let fm = frontmatter_for_mode(file, "pending_capture_mode", force_disk)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(resolve_pending_capture_mode(&fm, &project_config))
}

pub fn resolve_pending_capture_guard_mode_with_context(
    _file: &Path,
    rc: &CycleContext,
) -> Result<PendingCaptureGuardMode> {
    let fm = rc.frontmatter();
    let project_config = rc.project_config();
    Ok(resolve_pending_capture_mode(&fm, &project_config))
}

pub fn resolve_pending_done_guard_mode(file: &Path) -> Result<PendingCaptureGuardMode> {
    resolve_pending_done_mode_with_force_disk(file, false)
}

pub fn resolve_pending_done_mode_with_force_disk(
    file: &Path,
    force_disk: bool,
) -> Result<PendingCaptureGuardMode> {
    let fm = frontmatter_for_mode(file, "pending_done_mode", force_disk)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(resolve_pending_done_mode(&fm, &project_config))
}

pub fn resolve_pending_done_guard_mode_with_context(
    _file: &Path,
    rc: &CycleContext,
) -> Result<PendingCaptureGuardMode> {
    let fm = rc.frontmatter();
    let project_config = rc.project_config();
    Ok(resolve_pending_done_mode(&fm, &project_config))
}

pub fn resolve_review_done_guard_mode(file: &Path) -> Result<PendingCaptureGuardMode> {
    resolve_review_done_guard_mode_with_force_disk(file, false)
}

pub fn resolve_review_done_guard_mode_with_force_disk(
    file: &Path,
    force_disk: bool,
) -> Result<PendingCaptureGuardMode> {
    let fm = frontmatter_for_mode(file, "review_done_mode", force_disk)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(resolve_review_done_mode(&fm, &project_config))
}

pub fn resolve_auto_done(file: &Path) -> Result<bool> {
    resolve_auto_done_with_force_disk(file, false)
}

pub fn resolve_auto_done_with_force_disk(file: &Path, force_disk: bool) -> Result<bool> {
    let fm = frontmatter_for_mode(file, "auto_done", force_disk)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(agent_doc_frontmatter::project_config::resolve_auto_done(
        &fm,
        &project_config,
    ))
}

pub fn resolve_per_component_convergence(file: &Path) -> Result<bool> {
    let fm = frontmatter_for_mode(file, "per_component_convergence", false)?;
    let project_config = agent_doc_project_config_io::load_project_for_doc(file);
    Ok(
        agent_doc_frontmatter::project_config::resolve_per_component_convergence(
            &fm,
            &project_config,
        ),
    )
}
