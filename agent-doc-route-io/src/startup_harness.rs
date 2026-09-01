//! Route startup harness resolution.

use agent_doc_harness::HarnessConfig;
use agent_doc_run_context_io::AgentDocContextExt;
use std::path::Path;

pub fn resolve_harness_from_authorities(
    file: &Path,
    fm: &agent_doc_frontmatter::frontmatter::Frontmatter,
    global_config: &agent_doc_config::Config,
) -> HarnessConfig {
    let active_actor_harness = if fm
        .agent
        .as_deref()
        .is_some_and(|value| !value.trim().is_empty())
    {
        None
    } else {
        agent_doc_project_root_io::project_root_or_file_parent(file)
            .ok()
            .and_then(|project_root| {
                agent_doc_controller_io::project_controller::authoritative_actor_binding(
                    &project_root,
                    file,
                )
                .ok()
                .flatten()
            })
            .map(|record| record.harness)
    };
    let selection = agent_doc_supervisor::harness_authority::resolve_harness_authority(
        &agent_doc_supervisor::harness_authority::HarnessAuthorityFacts {
            declared_document_agent: fm.agent.clone(),
            active_actor_harness,
            configured_default_agent: global_config.default_agent.clone(),
        },
    );
    HarnessConfig::from_agent_name(&selection.agent)
}

/// Resolve [`HarnessConfig`] from a file's frontmatter and global config.
pub fn resolve_harness_for_file(file: &Path) -> HarnessConfig {
    let content = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "route_startup_harness",
    )
    .unwrap_or_default();
    let rc = agent_doc_run_context_io::cycle_context(file.to_path_buf());
    rc.set_doc_content(content);
    let fm = rc.frontmatter();
    let global_config = rc.global_config();
    resolve_harness_from_authorities(file, &fm, &global_config)
}
