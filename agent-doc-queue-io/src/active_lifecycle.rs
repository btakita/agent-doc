//! `#managedlistnodrift`: the state.db loader behind `agent:active`.
//!
//! Reads the claim ledger, the reported-release receipts, and the lineage
//! ledger, and joins them through
//! [`agent_doc_work_projection::active_lifecycle::ActiveLifecycle::from_ledgers`].
//! The binary installs [`load`] at startup.

use std::path::Path;

use agent_doc_work_projection::active_lifecycle::{ActiveLifecycle, LineageFactsIndex};
use anyhow::{Context, Result};

fn lineage_facts(file: &Path) -> Result<LineageFactsIndex> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(LineageFactsIndex::default());
    };
    let hash = agent_doc_hash::path_hash(file)
        .with_context(|| format!("canonicalize queue lifecycle document: {}", file.display()))?;
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_crdt_relay_io::load_queue_lifecycle_lineage_facts(&conn, &hash)
}

/// Derive the durable attempt state using the same claim inputs as the queue
/// projector. The caller supplies the already aligned claims and observed time.
pub fn from_ledgers(
    file: &Path,
    claims: &agent_doc_queue::queue_claim::QueueClaimLedger,
    receipts: &agent_doc_queue::claim_release_receipt::ClaimReleaseReceiptLedger,
    now_secs: u64,
) -> ActiveLifecycle {
    let lineage = if claims.claims.is_empty() && receipts.receipts.is_empty() {
        LineageFactsIndex::default()
    } else {
        lineage_facts(file).unwrap_or_else(|err| {
            eprintln!(
                "[queue] lineage unreadable for {} ({err:#}); ages fall back to attempt records",
                file.display()
            );
            LineageFactsIndex::default()
        })
    };
    ActiveLifecycle::from_ledgers_with_liveness(claims, receipts, &lineage, now_secs, |claim| {
        crate::queue_claim::claim_owner_liveness(file, claim)
    })
}

/// The lifecycle for an authoritative document cut. Follow row edits in
/// memory so status, Active, and queue markers use the same identity.
pub fn load(file: &Path, content: &str) -> Result<ActiveLifecycle> {
    let claims = crate::queue_claim::load_ledger_following(file, content)?;
    let receipts = crate::claim_release_receipt::load(file)?;
    Ok(from_ledgers(
        file,
        &claims,
        &receipts,
        crate::queue_claim::claim_clock_now_secs(),
    ))
}

/// Durable completion debt for live rows in this document cut.
pub fn reported_count(file: &Path, content: &str) -> Result<usize> {
    agent_doc_work_projection::reported_queue_count(content, &load(file, content)?)
}

/// Re-derive status and Active together for a live queue transition. On read
/// failure preserve both summaries until the retained ledgers are readable.
pub fn reconcile_summaries(file: &Path, content: &str) -> Result<Option<String>> {
    match load(file, content) {
        Ok(lifecycle) => {
            agent_doc_work_projection::reconcile_queue_lifecycle_summaries(content, &lifecycle)
        }
        Err(err) => {
            eprintln!(
                "[queue] durable lifecycle unreadable for {} ({err:#}); status and Active left unchanged",
                file.display()
            );
            Ok(None)
        }
    }
}
