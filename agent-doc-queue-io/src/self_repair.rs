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
