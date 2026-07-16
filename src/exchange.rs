//! `agent-doc exchange` — structural operations on the `agent:exchange` component.
//!
//! Phase 4 of `tasks/agent-doc/plan-exchange-tree-seqcrdt-and-ipc-unify.md`. These
//! commands mutate the exchange as a tree of distinct nodes (one per response /
//! prompt) via [`agent_doc_markdown_ast::exchange_tree`], then re-baseline the
//! snapshot + CRDT via `reset --from-current --preserve-session`. This is the
//! binary-owned, node-safe replacement for a raw text edit + manual `reset` (the
//! flow used to remove the reitrades IPC-diagnostic pollution).

use agent_doc_element::element;
use agent_doc_markdown_ast::exchange_tree;
use anyhow::{Context, Result};
use std::io::Read;
use std::path::Path;

const EXCHANGE: &str = "exchange";

fn exchange_component(doc: &str) -> Result<element::Component> {
    element::parse(doc)
        .context("failed to parse document components")?
        .into_iter()
        .find(|c| c.name == EXCHANGE)
        .context("document has no <!-- agent:exchange --> component")
}

fn read_stdin() -> Result<String> {
    let mut buf = String::new();
    std::io::stdin()
        .read_to_string(&mut buf)
        .context("failed to read body from stdin")?;
    Ok(buf)
}

/// `agent-doc exchange list <FILE>` — print the exchange nodes as JSON.
pub fn list(file: &Path) -> Result<()> {
    let doc = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "exchange_command_document",
    )
    .with_context(|| format!("failed to resolve {}", file.display()))?;
    let comp = exchange_component(&doc)?;
    let nodes = exchange_tree::list_exchange_nodes(comp.content(&doc));
    let out = serde_json::json!({
        "file": file.display().to_string(),
        "nodes": nodes
            .iter()
            .map(|n| serde_json::json!({ "id": n.node_id, "kind": n.kind, "label": n.label }))
            .collect::<Vec<_>>(),
    });
    println!("{}", serde_json::to_string_pretty(&out)?);
    Ok(())
}

/// Apply a pure `inner -> inner` transform to the exchange body, write it back, and
/// re-baseline the snapshot + CRDT (session preserved).
fn mutate(file: &Path, transform: impl FnOnce(&str) -> Result<String>) -> Result<()> {
    let doc = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "exchange_command_document",
    )
    .with_context(|| format!("failed to resolve {}", file.display()))?;
    let comp = exchange_component(&doc)?;
    let new_inner = transform(comp.content(&doc))?;
    let new_doc = comp.replace_content(&doc, &new_inner);
    agent_doc_document_realtime_io::atomic_write_through_authority(file, &new_doc)
        .with_context(|| format!("failed to write {}", file.display()))?;
    // Re-baseline snapshot + CRDT from the authority-resolved document while
    // preserving the session and any active response capture. Do not elect disk
    // authority here: a console-steering prompt can arrive while an editor owns
    // the live buffer.
    crate::reset::run(file, true, true, false)
        .context("failed to re-baseline snapshot/CRDT after exchange mutation")?;
    eprintln!(
        "[exchange] mutated {} and re-baselined sidecars",
        file.display()
    );
    Ok(())
}

/// `agent-doc exchange remove <FILE> --id <NodeId>` — drop one node.
pub fn remove(file: &Path, node_id: &str) -> Result<()> {
    mutate(file, |inner| {
        exchange_tree::remove_exchange_node(inner, node_id)
            .with_context(|| format!("no exchange node with id `{node_id}`"))
    })
}

/// `agent-doc exchange add-response <FILE> --header <H>` — append a response turn
/// (body read from stdin).
pub fn add_response(file: &Path, header: &str) -> Result<()> {
    let body = read_stdin()?;
    mutate(file, |inner| {
        Ok(exchange_tree::add_response(inner, header, &body))
    })
}

/// `agent-doc exchange add-prompt <FILE>` — append a user prompt turn (text read
/// from stdin).
pub fn add_prompt(file: &Path) -> Result<()> {
    let text = read_stdin()?;
    add_prompt_content(file, &text)
}

fn add_prompt_content(file: &Path, text: &str) -> Result<()> {
    mutate(file, |inner| Ok(exchange_tree::add_prompt(inner, text)))
}

/// `agent-doc exchange move <FILE> --id <N> --before|--after <Anchor>`.
pub fn move_node(file: &Path, node_id: &str, anchor: &str, before: bool) -> Result<()> {
    mutate(file, |inner| {
        exchange_tree::move_exchange_node(inner, node_id, anchor, before)
            .with_context(|| format!("move failed: missing node `{node_id}` or anchor `{anchor}`"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_prompt_during_active_capture_preserves_and_rebases_the_response() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        let baseline = concat!(
            "---\nagent_doc_format: template\nsession: test\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: original — test\n\n",
            "Original answer.\n",
            "<!-- agent:boundary:old -->\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&doc, baseline).unwrap();
        agent_doc_snapshot_io::save(&doc, baseline, agent_doc_ops_log_io::log_op).unwrap();
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: current work — test\n\n",
            "Retained response.\n",
            "<!-- /patch:exchange -->",
        );
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();

        add_prompt_content(
            &doc,
            "The same lender can have different vestings; preserve this follow-up.",
        )
        .unwrap();

        let current = std::fs::read_to_string(&doc).unwrap();
        assert!(current.contains("The same lender can have different vestings"));
        let active = agent_doc_capture_io::load_active(&doc)
            .unwrap()
            .expect("active capture survives prompt insertion");
        assert_eq!(active.capture_id, capture.capture_id);
        assert_eq!(active.response_body, response);
        agent_doc_capture_io::validate_replay_with_current_content(&doc, &active, &current)
            .expect("prompt insertion rebases the retained response");
    }
}
