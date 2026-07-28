//! Template I/O — `&Path`-taking wrappers around the pure
//! [`agent_doc_template`] surface.
//!
//! Keeps project-config lookup and file reads out of the pure template crate
//! while staying independent from orchestration.

use anyhow::{Context, Result};
use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use agent_doc_element::element;
use agent_doc_flow::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use agent_doc_frontmatter::project_config::{ComponentConfig, ProjectConfig};
use agent_doc_template::patchback::{PatchbackShape, TemplatePatchbackPlan};
use agent_doc_template::response_materialization::response_materialization_probe;
use agent_doc_template::{
    ComponentInfo, PatchBlock, TemplateInfo, apply_patches_pure, apply_patches_with_overrides_pure,
};

pub mod backlog_normalization;
pub mod response_materialization_io;
pub mod write_normalize;
pub use backlog_normalization::{
    NormalizedTemplateResponse, canonicalize_response_for_capture,
    canonicalize_response_for_capture_with_current_content, enforce_no_replace_pending,
    normalize_backlog_patch_response, pending_replace_escape_hatch_enabled,
};
pub use response_materialization_io::{
    ipc_response_materialized_or_fallback_with_recycle, log_ipc_proof_failure,
    log_ipc_proof_failure_with_recycle, log_partial_response_materialization_for_retry,
};
pub use write_normalize::{
    enforce_imperative_response_contract, enforce_imperative_response_contract_for_diff,
    normalize_user_prompts_in_exchange_safe, template_mode_overrides_for_current_doc,
};

/// Parse a model response for template patchback blocks and log parse decisions
/// through an injected sink.
pub fn parse_template_patchback(
    file: &Path,
    response: &str,
    source: &str,
    mut logger: impl FnMut(&Path, &str),
) -> Result<TemplatePatchbackPlan> {
    let plan = agent_doc_template::patchback::parse_template_patchback_plan(response)
        .context("failed to parse patch blocks from response")?;
    let parse_outcome = if (plan.shape == PatchbackShape::MalformedPatch
        && !plan.unmatched.trim().is_empty())
        || plan.shape == PatchbackShape::EscapedComponentMarkers
    {
        FlowOutcome::FailedClosed
    } else {
        FlowOutcome::Completed
    };

    if plan.marker_count > 0 || plan.raw_component_block_count > 0 {
        log_patchback_parse_event(file, plan.shape, parse_outcome, &mut logger);
        logger(
            file,
            &format!(
                "template_patchback_parse_shape file={} source={} response_hash={} markers={} patches={} exchange_patches={} unmatched_len={}",
                file.display(),
                source,
                agent_doc_hash::content_hash(response),
                plan.marker_count,
                plan.patches.len(),
                plan.exchange_patch_count,
                plan.unmatched.trim().len()
            ),
        );
    }

    if plan.has_malformed_orphan_markers() {
        logger(
            file,
            &format!(
                "template_patchback_malformed_rejected file={} source={} response_hash={} markers={} unmatched_len={} reason=patch_markers_without_closed_blocks",
                file.display(),
                source,
                agent_doc_hash::content_hash(response),
                plan.marker_count,
                plan.unmatched.trim().len()
            ),
        );
        anyhow::bail!(
            "malformed template patchback: found patch/replace markers but no closed patch blocks parsed; refusing to append unmatched content"
        );
    }
    if plan.has_escaped_component_markers() {
        logger(
            file,
            &format!(
                "template_patchback_escaped_component_rejected file={} source={} response_hash={} component_blocks={} reason=raw_component_markers_without_patch_blocks",
                file.display(),
                source,
                agent_doc_hash::content_hash(response),
                plan.raw_component_block_count
            ),
        );
        anyhow::bail!(
            "escaped template patchback: response carries raw `<!-- agent:NAME -->` component blocks instead of supported `<!-- patch:* -->` patch blocks; refusing to commit them as literal exchange text. Wrap the response in `<!-- patch:exchange -->` … `<!-- /patch:exchange -->` blocks, or rerun `agent-doc write --commit {}` to absorb an already-visible `### Re:` response.",
            file.display()
        );
    }

    Ok(plan)
}

pub fn patchback_parse_event(shape: PatchbackShape, outcome: FlowOutcome) -> FlowEvent {
    FlowEvent::new(
        FlowName::DocumentMutation,
        FlowStage::PatchbackParse,
        outcome,
    )
    .with_reason(shape.as_str())
}

pub fn log_patchback_parse_event(
    file: &Path,
    shape: PatchbackShape,
    outcome: FlowOutcome,
    logger: impl FnMut(&Path, &str),
) {
    agent_doc_flow_io::log_flow_event(file, patchback_parse_event(shape, outcome), logger);
}

pub fn log_template_structure_guard_event(
    file: &Path,
    reason: agent_doc_template::structure_guard::TemplateStructureGuardReason,
    outcome: FlowOutcome,
    logger: impl FnMut(&Path, &str),
) {
    agent_doc_flow_io::log_flow_event(
        file,
        agent_doc_template::structure_guard::template_structure_guard_event(reason, outcome),
        logger,
    );
}

pub fn lift_pending_from_exchange_safe(content: &str, file: &Path) -> String {
    match agent_doc_document::write_normalization::lift_pending_from_exchange(content) {
        Some(repaired) => {
            eprintln!(
                "[write] repaired: lifted agent:pending out of agent:exchange for {}",
                file.display()
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!("lift_pending_from_exchange file={}", file.display()),
            );
            repaired
        }
        None => content.to_string(),
    }
}

pub fn repair_response_prompt_order_for_file(
    doc: &str,
    response: Option<&str>,
    file: &Path,
    prompt_must_exist_in: Option<&str>,
) -> Result<Option<String>> {
    let repaired = agent_doc_element_exchange::repair_response_precedes_prompt_in_exchange(
        doc,
        response,
        prompt_must_exist_in,
    )
    .with_context(|| {
        format!(
            "failed to parse {} for response/prompt order repair",
            file.display()
        )
    })?;
    log_response_prompt_order_repair(file, &repaired);
    Ok(repaired)
}

pub fn repair_response_prompt_order_for_file_with_prompt_growth(
    doc: &str,
    response: Option<&str>,
    file: &Path,
    prompt_growth: agent_doc_element_exchange::PromptGrowthProvenanceInput<'_>,
) -> Result<Option<String>> {
    let repaired =
        agent_doc_element_exchange::repair_response_precedes_prompt_in_exchange_with_prompt_growth(
            doc,
            response,
            prompt_growth,
        )
        .with_context(|| {
            format!(
                "failed to parse {} for response/prompt order repair",
                file.display()
            )
        })?;
    log_response_prompt_order_repair(file, &repaired);
    Ok(repaired)
}

fn log_response_prompt_order_repair(file: &Path, repaired: &Option<String>) {
    if repaired.is_some() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "response_prompt_order_repaired file={} before_commit=true",
                file.display()
            ),
        );
    }
}

pub fn log_duplicate_prompt_residue_guard(file: &Path) {
    log_template_structure_guard_event(
        file,
        agent_doc_template::structure_guard::TemplateStructureGuardReason::DuplicatePromptResidue,
        FlowOutcome::FailedClosed,
        agent_doc_ops_log_io::log_op,
    );
}

pub fn normalize_template_structure_or_fail(content: &str, file: &Path) -> Result<String> {
    normalize_template_structure_or_fail_preserving(content, file, None)
}

pub fn normalize_template_structure_or_fail_preserving(
    content: &str,
    file: &Path,
    preserve_doc: Option<&str>,
) -> Result<String> {
    let malformed_close_repaired =
        agent_doc_element::element::repair_malformed_exchange_close_comment(content);
    if malformed_close_repaired.is_some() {
        eprintln!(
            "[write] normalize_template_structure: restored malformed closing exchange marker"
        );
    }
    let repairable_content = malformed_close_repaired.as_deref().unwrap_or(content);
    let orphan_repaired = agent_doc_element::element::repair_standalone_orphan_comment_terminators(
        repairable_content,
    );
    if orphan_repaired.is_some() {
        eprintln!(
            "[write] normalize_template_structure: removed standalone orphan HTML comment terminator"
        );
    }
    let repairable_content = orphan_repaired.as_deref().unwrap_or(repairable_content);
    let lifted = lift_pending_from_exchange_safe(repairable_content, file);
    let deduped_openers = {
        let mut result = lifted;
        while let Some(merged) = agent_doc_template::repair_duplicate_exchange_opener(&result)? {
            eprintln!("[write] normalize_template_structure: merged duplicate exchange opener");
            result = merged;
        }
        result
    };
    let (normalized, _) =
        agent_doc_element_exchange_io::repair_duplicate_prompt_artifacts_with_log(
            &agent_doc_element::element::strip_backlog_patch_attr(&deduped_openers),
            file,
            agent_doc_element_exchange_io::DuplicatePromptRepairOptions::new("structure")
                .preserving(preserve_doc),
            agent_doc_ops_log_io::log_op,
            log_duplicate_prompt_residue_guard,
        )?;
    match agent_doc_template::guard_no_conversation_tail_outside_exchange(&normalized) {
        Ok(()) => Ok(normalized),
        Err(err)
            if agent_doc_element::element::unmatched_component_close_name(&err)
                == Some("exchange") =>
        {
            if let Some(repaired) =
                agent_doc_template::repair_duplicate_exchange_close_scaffold(&normalized)?
            {
                log_template_structure_guard_event(
                    file,
                    agent_doc_template::structure_guard::TemplateStructureGuardReason::DuplicateScaffoldDropped,
                    FlowOutcome::Completed,
                    agent_doc_ops_log_io::log_op,
                );
                let (repaired, _) =
                    agent_doc_element_exchange_io::repair_duplicate_prompt_artifacts_with_log(
                        &repaired,
                        file,
                        agent_doc_element_exchange_io::DuplicatePromptRepairOptions::new(
                            "duplicate-scaffold repair",
                        )
                        .preserving(preserve_doc),
                        agent_doc_ops_log_io::log_op,
                        log_duplicate_prompt_residue_guard,
                    )?;
                agent_doc_template::guard_no_conversation_tail_outside_exchange(&repaired)
                    .context(format!(
                        "template structure guard failed for {} after duplicate-scaffold repair",
                        file.display()
                    ))?;
                return Ok(repaired);
            }
            if agent_doc_template::repair_duplicate_exchange_close_mixed_scaffold_tail(&normalized)?
                .is_some()
            {
                log_template_structure_guard_event(
                    file,
                    agent_doc_template::structure_guard::TemplateStructureGuardReason::MixedDuplicateScaffoldTail,
                    FlowOutcome::FailedClosed,
                    agent_doc_ops_log_io::log_op,
                );
                anyhow::bail!(
                    "mixed duplicate scaffold tail for {}: live conversation text is interleaved with duplicated template scaffold; refusing automatic closeout repair",
                    file.display()
                );
            }
            if let Some(repaired) =
                agent_doc_template::repair_duplicate_exchange_close_tail(&normalized)?
            {
                log_template_structure_guard_event(
                    file,
                    agent_doc_template::structure_guard::TemplateStructureGuardReason::DuplicateCloseTailMoved,
                    FlowOutcome::Completed,
                    agent_doc_ops_log_io::log_op,
                );
                let (repaired, _) =
                    agent_doc_element_exchange_io::repair_duplicate_prompt_artifacts_with_log(
                        &repaired,
                        file,
                        agent_doc_element_exchange_io::DuplicatePromptRepairOptions::new(
                            "duplicate-close repair",
                        )
                        .preserving(preserve_doc),
                        agent_doc_ops_log_io::log_op,
                        log_duplicate_prompt_residue_guard,
                    )?;
                agent_doc_template::guard_no_conversation_tail_outside_exchange(&repaired)
                    .context(format!(
                        "template structure guard failed for {} after duplicate-close repair",
                        file.display()
                    ))?;
                return Ok(repaired);
            }
            Err(err)
                .with_context(|| format!("template structure guard failed for {}", file.display()))
        }
        Err(err) => Err(err)
            .with_context(|| format!("template structure guard failed for {}", file.display())),
    }
}

pub fn response_materialization_probe_from_ipc_payload(payload: &serde_json::Value) -> String {
    let patches = payload
        .get("patches")
        .and_then(|value| value.as_array())
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let name = item
                        .get("component")
                        .or_else(|| item.get("name"))
                        .and_then(|value| value.as_str())?;
                    let content = item.get("content").and_then(|value| value.as_str())?;
                    Some(PatchBlock::new(name, content))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let unmatched = payload
        .get("unmatched")
        .and_then(|value| value.as_str())
        .unwrap_or("");
    response_materialization_probe(&patches, unmatched)
}

/// File-based wrapper for the pure
/// [`agent_doc_template::apply_patches_pure`]. Loads component/max_lines
/// configs from the document's `.agent-doc/config.toml`, derives the
/// boundary summary from the file stem, and delegates.
pub fn apply_patches(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
) -> Result<String> {
    apply_patches_with_project_config(doc, patches, unmatched, file, None)
}

/// Apply template patches using an already-loaded project config when supplied.
pub fn apply_patches_with_project_config(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    project_config: Option<Arc<ProjectConfig>>,
) -> Result<String> {
    let summary = file.file_stem().and_then(|s| s.to_str());
    let component_configs = load_component_configs(file, project_config.as_ref());
    let max_lines_configs = load_max_lines_configs(file, project_config.as_ref());
    apply_patches_pure(
        doc,
        patches,
        unmatched,
        summary,
        &component_configs,
        &max_lines_configs,
    )
}

/// File-based wrapper for
/// [`agent_doc_template::apply_patches_with_overrides_pure`].
pub fn apply_patches_with_overrides(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    mode_overrides: &HashMap<String, String>,
) -> Result<String> {
    apply_patches_with_overrides_with_project_config(
        doc,
        patches,
        unmatched,
        file,
        mode_overrides,
        None,
    )
}

/// Apply template patches with mode overrides and an already-loaded project
/// config when supplied.
pub fn apply_patches_with_overrides_with_project_config(
    doc: &str,
    patches: &[PatchBlock],
    unmatched: &str,
    file: &Path,
    mode_overrides: &HashMap<String, String>,
    project_config: Option<Arc<ProjectConfig>>,
) -> Result<String> {
    let summary = file.file_stem().and_then(|s| s.to_str());
    let component_configs = load_component_configs(file, project_config.as_ref());
    let max_lines_configs = load_max_lines_configs(file, project_config.as_ref());
    apply_patches_with_overrides_pure(
        doc,
        patches,
        unmatched,
        summary,
        &component_configs,
        &max_lines_configs,
        mode_overrides,
    )
}

/// Get template info for a document (for plugin rendering).
pub fn template_info(file: &Path) -> Result<TemplateInfo> {
    template_info_with_project_config(file, None)
}

/// Get template info using an already-loaded project config when supplied.
pub fn template_info_with_project_config(
    file: &Path,
    project_config: Option<Arc<ProjectConfig>>,
) -> Result<TemplateInfo> {
    let doc = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "template_info_with_project_config",
    )?;

    let (fm, _body) = agent_doc_frontmatter::frontmatter::parse(&doc)?;
    let template_mode = fm.resolve_mode().is_template();

    let components = element::parse(&doc)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let configs = load_component_configs(file, project_config.as_ref());

    let component_infos: Vec<ComponentInfo> = components
        .iter()
        .map(|comp| {
            let content = comp.content(&doc).to_string();
            let mode = comp
                .patch_mode()
                .map(|s| s.to_string())
                .or_else(|| configs.get(&comp.name).cloned())
                .unwrap_or_else(|| default_mode(&comp.name).to_string());
            let line = doc[..comp.open_start].matches('\n').count() + 1;
            ComponentInfo {
                name: comp.name.clone(),
                mode,
                content,
                line,
                max_entries: None,
            }
        })
        .collect();

    Ok(TemplateInfo {
        template_mode,
        components: component_infos,
    })
}

/// Load component mode configs from `.agent-doc/config.toml` (under [components] section).
fn load_component_configs(
    file: &Path,
    project_config: Option<&Arc<ProjectConfig>>,
) -> HashMap<String, String> {
    let proj_cfg = load_project_from_doc(file, project_config);
    proj_cfg
        .components
        .iter()
        .map(|(name, cfg): (&String, &ComponentConfig)| (name.clone(), cfg.patch.clone()))
        .collect()
}

/// Load max_lines settings from `.agent-doc/config.toml` (under [components] section).
fn load_max_lines_configs(
    file: &Path,
    project_config: Option<&Arc<ProjectConfig>>,
) -> HashMap<String, usize> {
    let proj_cfg = load_project_from_doc(file, project_config);
    proj_cfg
        .components
        .iter()
        .filter(|(_, cfg)| cfg.max_lines > 0)
        .map(|(name, cfg): (&String, &ComponentConfig)| (name.clone(), cfg.max_lines))
        .collect()
}

/// Resolve project config by walking up from a document path to find `.agent-doc/config.toml`.
fn load_project_from_doc(
    file: &Path,
    project_config: Option<&Arc<ProjectConfig>>,
) -> Arc<ProjectConfig> {
    if let Some(project_config) = project_config {
        return Arc::clone(project_config);
    }
    let start = file.parent().unwrap_or(file);
    let mut current = start;
    loop {
        let candidate = current.join(".agent-doc").join("config.toml");
        if candidate.exists() {
            return Arc::new(agent_doc_project_config_io::load_project_from(&candidate));
        }
        match current.parent() {
            Some(p) if p != current => current = p,
            _ => break,
        }
    }
    Arc::new(agent_doc_project_config_io::load_project())
}

/// Default mode for a component by name.
fn default_mode(name: &str) -> &'static str {
    match name {
        "exchange" | "findings" => "append",
        _ => "replace",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_template_patchback_rejects_raw_component_form_before_commit() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();
        let raw_template_form = concat!(
            "<!-- agent:status -->\n",
            "Work complete.\n",
            "<!-- /agent:status -->\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: closeout - gpt-5\n\n",
            "Implemented and verified.\n",
            "<!-- /agent:exchange -->\n",
        );
        let err =
            parse_template_patchback(&file, raw_template_form, "test", |_, _| {}).unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("escaped template patchback"),
            "expected escaped-component rejection, got: {msg}"
        );
        assert!(
            msg.contains("<!-- patch:exchange -->"),
            "diagnostic must point to the supported patch form, got: {msg}"
        );
    }

    #[test]
    fn parse_template_patchback_accepts_plain_response_without_component_markers() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();
        let plain = "### Re: closeout - gpt-5\n\nImplemented and verified.\n";
        let plan = parse_template_patchback(&file, plain, "test", |_, _| {}).unwrap();
        assert_eq!(plan.shape, PatchbackShape::PlainResponse);
    }

    #[test]
    fn patchback_parse_event_carries_shape_reason() {
        let event =
            patchback_parse_event(PatchbackShape::MalformedPatch, FlowOutcome::FailedClosed);

        assert_eq!(event.flow, FlowName::DocumentMutation);
        assert_eq!(event.stage, FlowStage::PatchbackParse);
        assert_eq!(event.outcome, FlowOutcome::FailedClosed);
        assert_eq!(event.reason.as_deref(), Some("malformed_patch"));
    }

    #[test]
    fn parse_template_patchback_rejects_orphan_patch_markers() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("doc.md");
        std::fs::write(&file, "").unwrap();
        let err = parse_template_patchback(
            &file,
            "<!-- patch:exchange -->\n### Re: broken\n",
            "test",
            |_, _| {},
        )
        .unwrap_err();

        assert!(err.to_string().contains("malformed template patchback"));
    }

    #[test]
    fn response_materialization_probe_from_ipc_payload_uses_patch_component_alias() {
        let payload = serde_json::json!({
            "patches": [
                {
                    "component": "exchange",
                    "content": "### Re: do [#x]\n\nDone.\n"
                }
            ],
            "unmatched": "ignored because exchange patch is selected"
        });

        let probe = response_materialization_probe_from_ipc_payload(&payload);

        assert!(probe.contains("### Re: do [#x]"));
        assert!(!probe.contains("ignored because exchange patch is selected"));
    }

    #[test]
    fn normalize_template_structure_repairs_dbj7_orphan_comment_terminator() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("doc.md");
        let doc = concat!(
            "<!-- agent:exchange -->\n",
            "> 📌 do [#dbj7]\n\n",
            "### Re: #dbj7 — gpt-5\n\n",
            "Response body.\n\n",
            " -->\n",
            "<!-- /agent:exchange -->\n",
        );

        let normalized = normalize_template_structure_or_fail(doc, &file).unwrap();

        assert!(!normalized.contains("\n -->\n"));
        assert!(normalized.contains("Response body."));
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&normalized),
            None
        );
    }

    #[test]
    fn normalize_template_structure_repairs_close_without_replacing_operator_queue_cut() {
        let temp = TempDir::new().unwrap();
        let file = temp.path().join("doc.md");
        let doc = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "❯ Prompt.\n",
            "### Re: Prompt. — gpt-5\n\nResponse body.\n",
            "<!-- /agent:exchange --\n",
            "<!-- agent:queue -->\n",
            "- do [#operator-added]\n",
            "<!-- /agent:queue -->\n",
        );

        let normalized = normalize_template_structure_or_fail(doc, &file).unwrap();

        assert!(normalized.contains("<!-- /agent:exchange -->\n"));
        assert!(normalized.contains("- do [#operator-added]\n"));
        assert!(!normalized.contains("#operator-deleted"));
        assert_eq!(
            agent_doc_element::element::structural_corruption_reason(&normalized),
            None
        );
    }

    #[test]
    fn apply_patches_with_project_config_uses_cached_config() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let config_path = dir.path().join(".agent-doc/config.toml");
        std::fs::write(&config_path, "[components.exchange]\npatch = \"prepend\"\n").unwrap();
        let doc_path = dir.path().join("doc.md");
        let doc = "<!-- agent:exchange -->\nold\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc_path, doc).unwrap();
        let project_config = Arc::new(agent_doc_project_config_io::load_project_from(&config_path));

        let (patches, unmatched) = agent_doc_template::parse_patches(
            "<!-- patch:exchange -->\nnew\n<!-- /patch:exchange -->\n",
        )
        .unwrap();
        let patched = apply_patches_with_project_config(
            doc,
            &patches,
            &unmatched,
            &doc_path,
            Some(project_config),
        )
        .unwrap();

        assert!(
            patched.contains("new\nold"),
            "component config should make exchange prepend, got:\n{patched}"
        );
    }
}
