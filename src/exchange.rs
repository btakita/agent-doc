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
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
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
    let doc = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let comp = exchange_component(&doc)?;
    let new_inner = transform(comp.content(&doc))?;
    let new_doc = comp.replace_content(&doc, &new_inner);
    std::fs::write(file, &new_doc)
        .with_context(|| format!("failed to write {}", file.display()))?;
    // Re-baseline snapshot + CRDT from the mutated document, preserving the session
    // (from_current = true, preserve_session = true, force_disk = true).
    crate::reset::run(file, true, true, true)
        .context("failed to re-baseline snapshot/CRDT after exchange mutation")?;
    eprintln!("[exchange] mutated {} and re-baselined sidecars", file.display());
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
    mutate(file, |inner| Ok(exchange_tree::add_response(inner, header, &body)))
}

/// `agent-doc exchange add-prompt <FILE>` — append a user prompt turn (text read
/// from stdin).
pub fn add_prompt(file: &Path) -> Result<()> {
    let text = read_stdin()?;
    mutate(file, |inner| Ok(exchange_tree::add_prompt(inner, &text)))
}

/// `agent-doc exchange move <FILE> --id <N> --before|--after <Anchor>`.
pub fn move_node(file: &Path, node_id: &str, anchor: &str, before: bool) -> Result<()> {
    mutate(file, |inner| {
        exchange_tree::move_exchange_node(inner, node_id, anchor, before).with_context(|| {
            format!("move failed: missing node `{node_id}` or anchor `{anchor}`")
        })
    })
}
