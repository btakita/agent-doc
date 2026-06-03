//! # Module: queue_cmd
//!
//! CLI subcommands for managing the `agent:queue` component.
//!
//! - `agent-doc queue sync <FILE>` — one-shot sync from backlog items with
//!   `queue` attribute into `agent:queue`.

use anyhow::{bail, Context, Result};
use std::path::Path;

use crate::component;
use crate::pending;
use crate::queue;
use crate::snapshot;

pub fn sync(file: &Path) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let components = component::parse(&content)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let queue_comp = components.iter().find(|c| c.name == "queue");
    let Some(qc) = queue_comp else {
        bail!(
            "{}: no agent:queue component found. Add `<!-- agent:queue -->..<!-- /agent:queue -->` to the document.",
            file.display()
        );
    };

    let mut mode: Option<queue::BacklogQueueSyncMode> = None;
    let mut ids: Vec<String> = Vec::new();
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let Some(value) = comp.attrs.get("queue") else {
            continue;
        };
        let Some(comp_mode) = queue::BacklogQueueSyncMode::parse(value) else {
            continue;
        };
        if mode.is_none() {
            mode = Some(comp_mode);
        }
        let body = &content[comp.open_end..comp.close_start];
        ids.extend(pending::active_item_ids(body));
    }

    let Some(effective_mode) = mode else {
        bail!(
            "{}: no agent:backlog/agent:icebox component carries a `queue` attribute. \
             Add `<!-- agent:backlog queue -->` (or `queue=sync`, `queue=prepend`) to enable sync.",
            file.display()
        );
    };

    if ids.is_empty() {
        bail!(
            "{}: no active backlog items found to sync. Add `[ ] [#id] ...` items to agent:backlog first.",
            file.display()
        );
    }

    let body = &content[qc.open_end..qc.close_start];
    let entries = queue::parse(body)
        .with_context(|| format!("failed to parse queue body in {}", file.display()))?;

    let Some(synced) = queue::sync_backlog_into_queue(&entries, &ids, effective_mode) else {
        println!(
            "{}: queue already in sync ({} active backlog id(s), {:?} mode). No changes.",
            file.display(),
            ids.len(),
            effective_mode
        );
        return Ok(());
    };

    let new_body = queue::render(&synced);
    let new_content = qc.replace_content(&content, &new_body);

    std::fs::write(file, &new_content)
        .with_context(|| format!("failed to write {}", file.display()))?;

    let prompt_count = synced
        .iter()
        .filter(|e| matches!(e, queue::QueueEntry::Prompt(_)))
        .count();
    println!(
        "{}: synced {} backlog id(s) → {} queue prompt(s) ({:?} mode)",
        file.display(),
        ids.len(),
        prompt_count,
        effective_mode
    );

    if let Err(e) = snapshot::save(file, &new_content) {
        eprintln!("[queue sync] warning: failed to update snapshot: {}", e);
    }

    Ok(())
}
