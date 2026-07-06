//! Route startup harness resolution.

use agent_doc_harness::HarnessConfig;
use agent_doc_run_context_io::AgentDocContextExt;
use std::path::Path;

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
    HarnessConfig::from_context(&fm, &global_config)
}
