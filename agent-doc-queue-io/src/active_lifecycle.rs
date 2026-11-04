//! `#managedlistnodrift`: the state.db loader behind `agent:active`.
//!
//! Reads the claim ledger, the reported-release receipts, and the lineage
//! ledger, and joins them through
//! [`agent_doc_work_projection::active_lifecycle::ActiveLifecycle::from_ledgers`].
//! The binary installs [`load`] at startup.

use std::collections::HashMap;
use std::path::Path;

use agent_doc_work_projection::active_lifecycle::{ActiveLifecycle, LineageFacts};
use anyhow::Result;

fn lineage_facts(file: &Path) -> HashMap<(String, u64), LineageFacts> {
    if agent_doc_fs::find_project_root(file).is_none() {
        return HashMap::new();
    }
    let ledger = match crate::queue_lineage::load(file) {
        Ok(ledger) => ledger,
        Err(err) => {
            // Lineage only refines ages and the integrated note; an unreadable
            // lineage never hides or invents an active entry.
            eprintln!(
                "[active] lineage unreadable for {} ({err:#}); ages fall back to claim records",
                file.display()
            );
            return HashMap::new();
        }
    };
    ledger
        .attempts
        .into_iter()
        .map(|attempt| {
            let latest = attempt
                .events
                .iter()
                .max_by_key(|event| event.at_secs)
                .map(|event| {
                    (
                        format!("{:?}", event.kind).to_ascii_lowercase(),
                        event.at_secs,
                    )
                });
            let has = |kind: crate::queue_lineage::QueueLineageEventKind| {
                attempt.events.iter().any(|event| event.kind == kind)
            };
            let facts = LineageFacts {
                latest,
                integrated: has(crate::queue_lineage::QueueLineageEventKind::Integrated),
                ended: has(crate::queue_lineage::QueueLineageEventKind::Cancelled)
                    || has(crate::queue_lineage::QueueLineageEventKind::Removed),
            };
            ((attempt.owner, attempt.generation), facts)
        })
        .collect()
}

/// The durable lifecycle of `file`'s queue attempts. An unreadable claim or
/// receipt ledger is an error, so the caller leaves `agent:active` unchanged.
pub fn load(file: &Path) -> Result<ActiveLifecycle> {
    let claims = crate::queue_claim::load_ledger(file)?;
    let receipts = crate::claim_release_receipt::load(file)?;
    Ok(ActiveLifecycle::from_ledgers(
        &claims,
        &receipts,
        &lineage_facts(file),
        crate::queue_claim::claim_clock_now_secs(),
    ))
}
