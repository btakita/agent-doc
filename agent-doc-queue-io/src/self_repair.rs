//! `#selfrepaircleanup`: [`agent_doc_queue::self_repair`] with attempt
//! liveness read from the claim ledger; the follow-up ledgers keyed on row
//! text follow each row the repair rewrites.

use std::path::Path;

use agent_doc_queue::reconcile_incremental::{
    ReconcileCache, ReconcileInputs, ReconcileReport, Reconciled, reconcile_full,
    reconcile_incremental,
};
use agent_doc_queue::self_repair::{SelfRepair, self_repair_document_with_provenance};

/// The external answers the queue reconcile passes read for one document
/// revision (`#incrementalreconcile`): claim-ledger attempt liveness and row
/// claims, response proof from `agent:exchange` and the response index, and
/// struck provenance from the committed copies (read lazily: most revisions
/// have no row that needs it).
pub struct ReconcileInputSources<'a> {
    file: &'a Path,
    content: &'a str,
    /// `(owner, generation)` of every ledger claim; `None` when the ledger is
    /// unreadable.
    attempts: Option<Vec<(String, u64)>>,
    /// `(identity, collapsed text)` of every ledger claim; `None` when the
    /// ledger is unreadable.
    claims: Option<Vec<(agent_doc_element_queue::QueueItemIdentity, String)>>,
    originals: std::cell::OnceCell<std::collections::BTreeSet<String>>,
    committed: std::cell::OnceCell<Vec<String>>,
}

fn normalize_response_id(id: &str) -> String {
    id.trim()
        .trim_start_matches("[#")
        .trim_start_matches('#')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

impl<'a> ReconcileInputSources<'a> {
    /// Read the sources for `content`, the revision the passes run on.
    pub fn load(file: &'a Path, content: &'a str) -> Self {
        let ledger = crate::queue_claim::load_ledger(file).ok();
        let attempts = ledger.as_ref().map(|ledger| {
            ledger
                .claims
                .iter()
                .map(|claim| (claim.owner.clone(), claim.generation))
                .collect()
        });
        let claims = ledger.as_ref().map(|ledger| {
            ledger
                .claims
                .iter()
                .map(|claim| (claim.identity.clone(), collapse(&claim.item_text)))
                .collect()
        });
        Self {
            file,
            content,
            attempts,
            claims,
            originals: std::cell::OnceCell::new(),
            committed: std::cell::OnceCell::new(),
        }
    }

    /// A claim label is stale when the ledger no longer holds that exact
    /// `(owner, generation)` attempt; an unreadable ledger keeps every label.
    pub fn is_live(&self, owner: &str, generation: u64) -> bool {
        match &self.attempts {
            Some(attempts) => attempts
                .iter()
                .any(|(held, held_generation)| held == owner && *held_generation == generation),
            None => true,
        }
    }

    /// A misplaced response tree is retired only when its original is proven
    /// elsewhere: in this revision's `agent:exchange`, or the response index
    /// of the exchange archive.
    pub fn response_proven(&self, id: &str) -> bool {
        self.originals
            .get_or_init(|| {
                let mut originals: std::collections::BTreeSet<String> =
                    agent_doc_archive_io::indexed_response_ids(self.file)
                        .map(|ids| ids.iter().map(|id| normalize_response_id(id)).collect())
                        .unwrap_or_default();
                if let Ok(components) = agent_doc_element::element::parse(self.content) {
                    for exchange in components
                        .iter()
                        .filter(|component| component.name == "exchange")
                    {
                        originals.extend(
                            agent_doc_archive_io::response_ids_in_content(
                                exchange.content(self.content),
                            )
                            .iter()
                            .map(|id| normalize_response_id(id)),
                        );
                    }
                }
                originals
            })
            .contains(&normalize_response_id(id))
    }

    /// `#e107moved`: a column-0 row a torn merge put inside another row's
    /// response tree gets its strike back only when a committed copy (the
    /// sync baseline or git HEAD) holds it struck outside any response tree.
    pub fn struck_provenance(&self, work: &str) -> bool {
        self.committed
            .get_or_init(|| {
                [
                    agent_doc_snapshot_io::load_document_baseline(self.file)
                        .ok()
                        .flatten(),
                    agent_doc_git_io::revision::show_head(self.file)
                        .ok()
                        .flatten(),
                ]
                .into_iter()
                .flatten()
                .collect()
            })
            .iter()
            .any(|copy| agent_doc_queue::response_tree_rehome::top_level_row_struck_in(copy, work))
    }

    /// `#treefinish`: a row whose work identity, or whose text, any ledger
    /// claim holds is never struck; an unreadable ledger strikes nothing
    /// (every row is assumed claimed).
    pub fn is_claimed(&self, row: &str) -> bool {
        match &self.claims {
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
                // claim whose text contains the row's work text, or is
                // contained in it, holds the row too. Short texts never match
                // by text.
                let long = |text: &str| text.chars().count() >= 8;
                claims.iter().any(|(held, text)| {
                    *held == identity
                        || (long(&work)
                            && long(text)
                            && (text.contains(&work) || work.contains(text)))
                })
            }
        }
    }

    /// Run `f` with these sources as the chain's [`ReconcileInputs`].
    pub fn with_inputs<T>(&self, f: impl FnOnce(&ReconcileInputs<'_>) -> T) -> T {
        let is_live = |owner: &str, generation: u64| self.is_live(owner, generation);
        let response_proven = |id: &str| self.response_proven(id);
        let struck_provenance = |work: &str| self.struck_provenance(work);
        let is_claimed = |row: &str| self.is_claimed(row);
        f(&ReconcileInputs {
            is_live: &is_live,
            response_proven: &response_proven,
            struck_provenance: &struck_provenance,
            is_claimed: &is_claimed,
        })
    }
}

/// Carry the follow-up records keyed on row text to the rows a self-repair
/// rewrote: their root provenance and owner classification follow them to
/// the new text (the old key stays until its row is gone, so a write that
/// never lands loses nothing).
pub fn carry_self_repair_rewrites(file: &Path, repairs: &[SelfRepair]) {
    let rewrites: Vec<(String, String)> = repairs
        .iter()
        .filter_map(|repair| repair.rewritten.clone())
        .collect();
    crate::followup_provenance_ledger::carry_rewritten_rows(file, &rewrites);
    crate::followup_owner_ledger::carry_rewritten_rows(file, &rewrites);
}

/// Run every self-repair over `content`. A claim label is stale when the
/// ledger no longer holds that exact `(owner, generation)` attempt; an
/// unreadable ledger keeps every label (no claim is assumed retired). A
/// response tree copied under a response-structure row is retired only when
/// its id's original is in `agent:exchange` or the response index.
pub fn self_repair_document_for(file: &Path, content: &str) -> Option<(String, Vec<SelfRepair>)> {
    let sources = ReconcileInputSources::load(file, content);
    let repaired = self_repair_document_with_provenance(
        content,
        &|owner: &str, generation: u64| sources.is_live(owner, generation),
        &|id: &str| sources.response_proven(id),
        &|work: &str| sources.struck_provenance(work),
    )?;
    carry_self_repair_rewrites(file, &repaired.1);
    Some(repaired)
}

/// The response passes of `agent-doc queue sync`
/// ([`agent_doc_queue::reconcile_incremental::queue_sync_response_chain`])
/// with this document's ledgers and provenance, carrying self-repair
/// rewrites into the follow-up ledgers as [`self_repair_document_for`] does.
pub fn queue_sync_response_chain_for(
    file: &Path,
    content: &str,
) -> agent_doc_queue::reconcile_incremental::QueueSyncResponses {
    let sources = ReconcileInputSources::load(file, content);
    let chain = sources.with_inputs(|inputs| {
        agent_doc_queue::reconcile_incremental::queue_sync_response_chain(content, inputs)
    });
    carry_self_repair_rewrites(file, &chain.repairs);
    chain
}

/// `#treefinish`: [`agent_doc_queue::response_tree_sync::sync_response_trees_striking`]
/// with row claims read from the claim ledger. A row whose work identity, or
/// whose text, any ledger claim holds is never struck; an unreadable ledger
/// strikes nothing (every row is assumed claimed).
pub fn sync_response_trees_for(
    file: &Path,
    content: &str,
) -> Option<agent_doc_queue::response_tree_sync::ResponseTreeSync> {
    let sources = ReconcileInputSources::load(file, content);
    agent_doc_queue::response_tree_sync::sync_response_trees_striking(content, &|row: &str| {
        sources.is_claimed(row)
    })
}

/// `#incrementalreconcile`: the queue/closeout chain
/// ([`agent_doc_queue::reconcile_incremental`]) over `content` with this
/// document's external answers, reusing `cache` (one per document) across
/// revisions. Byte-identical to [`reconcile_chain_full_for`].
pub fn reconcile_chain_for(
    file: &Path,
    content: &str,
    cache: &mut ReconcileCache,
) -> ReconcileReport {
    let sources = ReconcileInputSources::load(file, content);
    sources.with_inputs(|inputs| reconcile_incremental(None, content, inputs, cache))
}

/// [`reconcile_chain_for`] from the revision's changeset: `changeset` turns
/// the revision `cache` reconciled last into `content`
/// ([`agent_doc_queue::reconcile_incremental::reconcile_incremental_changeset`]),
/// so the chain never compares through `agent:exchange` to find what changed.
pub fn reconcile_chain_changeset_for(
    file: &Path,
    changeset: &[agent_doc_queue::reconcile_incremental::TextSplice],
    content: &str,
    cache: &mut ReconcileCache,
) -> ReconcileReport {
    let sources = ReconcileInputSources::load(file, content);
    sources.with_inputs(|inputs| {
        agent_doc_queue::reconcile_incremental::reconcile_incremental_changeset(
            changeset, content, inputs, cache,
        )
    })
}

/// The reference [`reconcile_chain_for`] equals: every pass on the whole
/// document with no cache.
pub fn reconcile_chain_full_for(file: &Path, content: &str) -> Reconciled {
    let sources = ReconcileInputSources::load(file, content);
    sources.with_inputs(|inputs| reconcile_full(content, inputs))
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}
