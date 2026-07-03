//! Route document preparation I/O.

use anyhow::{Context, Result};
use std::path::Path;

use agent_doc_frontmatter::frontmatter;

pub type RouteWriteDocumentFn =
    fn(file: &Path, next_content: &str, previous_content: &str, reason: &str) -> Result<()>;

#[derive(Clone, Copy)]
pub struct RouteDocumentPrepEffects {
    pub write_document: RouteWriteDocumentFn,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDocumentPreparation {
    pub content: String,
    pub session_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteDuplicatePromptCleanup {
    pub content: String,
    pub removed_answered_tail: bool,
    pub removed_comment: bool,
}

pub fn prepare_route_document(
    file: &Path,
    effects: RouteDocumentPrepEffects,
) -> Result<RouteDocumentPreparation> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    agent_doc_frontmatter_io::session::require_agent_doc_document(&content, file)?;
    let (mut updated_content, session_id) =
        agent_doc_frontmatter_io::session::ensure_session_for_file(&content, file)?;
    if updated_content != content {
        (effects.write_document)(file, &updated_content, &content, "route_session_id")
            .with_context(|| format!("failed to write {}", file.display()))?;
        eprintln!("[route] Generated session UUID: {}", session_id);
    }

    let snapshot_doc = agent_doc_snapshot_io::load(file).ok().flatten();
    let head_doc = agent_doc_git_io::revision::show_head(file).ok().flatten();
    let mut preserve_docs = Vec::new();
    preserve_docs.push(updated_content.as_str());
    if let Some(head_doc) = head_doc.as_deref() {
        preserve_docs.push(head_doc);
    }
    if let Some(snapshot_doc) = snapshot_doc.as_deref() {
        preserve_docs.push(snapshot_doc);
    }

    if let Some(cleanup) =
        scrub_duplicate_prompt_comments_for_route(&updated_content, &preserve_docs)?
    {
        (effects.write_document)(
            file,
            &cleanup.content,
            &updated_content,
            "route_dedup_scrub",
        )?;
        if cleanup.removed_answered_tail {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "duplicate_answered_exchange_prompt_tail_removed file={} source=route",
                    file.display()
                ),
            );
            eprintln!(
                "[route] removed duplicate answered prompt tail after exchange boundary in {}",
                file.display()
            );
        }
        if cleanup.removed_comment {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "post_exchange_duplicate_prompt_comment_removed file={} source=route",
                    file.display()
                ),
            );
            eprintln!(
                "[route] scrubbed duplicate prompt text from comment after exchange in {}",
                file.display()
            );
        }
        updated_content = cleanup.content;
    }

    Ok(RouteDocumentPreparation {
        content: updated_content,
        session_id,
    })
}

pub fn scrub_duplicate_prompt_comments_for_route(
    content: &str,
    preserve_docs: &[&str],
) -> Result<Option<RouteDuplicatePromptCleanup>> {
    let (frontmatter, _) = frontmatter::parse(content)
        .context("failed to parse document frontmatter before route cleanup")?;
    if !frontmatter.resolve_mode().is_template() {
        return Ok(None);
    }
    let mut cleaned_content = content.to_string();
    let mut removed_answered_tail = false;
    let mut removed_comment = false;
    if let Some(tail_cleaned) =
        agent_doc_template::remove_duplicate_answered_exchange_prompt_tail(&cleaned_content)
    {
        cleaned_content = tail_cleaned;
        removed_answered_tail = true;
    }
    if let Some(tail_cleaned) =
        agent_doc_template::remove_post_exchange_duplicate_prompt_comments_preserving_docs(
            &cleaned_content,
            preserve_docs,
        )
    {
        cleaned_content = tail_cleaned;
        removed_comment = true;
    }
    agent_doc_template::guard_no_duplicate_prompt_residue_outside_exchange(&cleaned_content)
        .context("route duplicate prompt residue guard failed")?;
    if removed_answered_tail || removed_comment {
        Ok(Some(RouteDuplicatePromptCleanup {
            content: cleaned_content,
            removed_answered_tail,
            removed_comment,
        }))
    } else {
        Ok(None)
    }
}
