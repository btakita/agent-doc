//! `#selfrepaircleanup`: [`agent_doc_queue::self_repair`] with attempt
//! liveness read from the claim ledger.

use std::path::Path;

use agent_doc_queue::self_repair::{SelfRepair, self_repair_document};

/// Run every self-repair over `content`. A claim label is stale when the
/// ledger no longer holds that exact `(owner, generation)` attempt; an
/// unreadable ledger keeps every label (no claim is assumed retired). A
/// response tree copied under a response-structure row is retired only when
/// its id's original is in `agent:exchange` or the response index.
pub fn self_repair_document_for(file: &Path, content: &str) -> Option<(String, Vec<SelfRepair>)> {
    let attempts: Option<Vec<(String, u64)>> =
        crate::queue_claim::load_ledger(file).ok().map(|ledger| {
            ledger
                .claims
                .iter()
                .map(|claim| (claim.owner.clone(), claim.generation))
                .collect()
        });
    let is_live = |owner: &str, generation: u64| match &attempts {
        Some(attempts) => attempts
            .iter()
            .any(|(held, held_generation)| held == owner && *held_generation == generation),
        None => true,
    };
    // A misplaced response tree is retired only when its original is
    // proven elsewhere: in this document's `agent:exchange`, or the response
    // index of the exchange archive.
    let normalize = |id: &str| {
        id.trim()
            .trim_start_matches("[#")
            .trim_start_matches('#')
            .trim_end_matches(']')
            .to_ascii_lowercase()
    };
    let mut originals: std::collections::BTreeSet<String> =
        agent_doc_archive_io::indexed_response_ids(file)
            .map(|ids| ids.iter().map(|id| normalize(id)).collect())
            .unwrap_or_default();
    if let Ok(components) = agent_doc_element::element::parse(content) {
        for exchange in components
            .iter()
            .filter(|component| component.name == "exchange")
        {
            originals.extend(
                agent_doc_archive_io::response_ids_in_content(exchange.content(content))
                    .iter()
                    .map(|id| normalize(id)),
            );
        }
    }
    let response_proven = |id: &str| originals.contains(&normalize(id));
    self_repair_document(content, &is_live, &response_proven)
}

/// `#treefinish`: [`agent_doc_queue::response_tree_sync::sync_response_trees_striking`]
/// with row claims read from the claim ledger. A row whose work identity, or
/// whose text, any ledger claim holds is never struck; an unreadable ledger
/// strikes nothing (every row is assumed claimed).
pub fn sync_response_trees_for(
    file: &Path,
    content: &str,
) -> Option<agent_doc_queue::response_tree_sync::ResponseTreeSync> {
    let claims: Option<Vec<(agent_doc_element_queue::QueueItemIdentity, String)>> =
        crate::queue_claim::load_ledger(file).ok().map(|ledger| {
            ledger
                .claims
                .iter()
                .map(|claim| (claim.identity.clone(), collapse(&claim.item_text)))
                .collect()
        });
    let is_claimed = |row: &str| match &claims {
        None => true,
        Some(claims) if claims.is_empty() => false,
        Some(claims) => {
            let identity = agent_doc_queue::queue_claim::claim_identity(row);
            let work = collapse(
                agent_doc_element_queue::struck_work_text(row)
                    .trim()
                    .trim_start_matches("### "),
            );
            // A nested row's claim keys its lineage (`parent: leaf`), so a
            // claim whose text contains the row's work text, or is contained
            // in it, holds the row too. Short texts never match by text.
            let long = |text: &str| text.chars().count() >= 8;
            claims.iter().any(|(held, text)| {
                *held == identity
                    || (long(&work) && long(text) && (text.contains(&work) || work.contains(text)))
            })
        }
    };
    agent_doc_queue::response_tree_sync::sync_response_trees_striking(content, &is_claimed)
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
