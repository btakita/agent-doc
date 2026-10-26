//! One-time repair of finished-work evidence that outlived its row
//! (`#stalecompletionrepair`).
//!
//! Since `#strikeretires`, every binary strike retires the struck row's
//! completion cell and its reported-release receipt. Strikes made before that
//! fix, or made by hand in the editor, left both behind. Both are keyed only by
//! work identity, so a leftover projects `🔄` / `☑️` onto a later row with the
//! same text.
//!
//! The repair is defined purely over one document cut and its durable claim
//! and receipt state: evidence is stale when its work identity has no live
//! (non-struck) row in ANY queue component of the document and no active
//! (non-`Lost`) claim attempt. Stale cells are retired through the same
//! structural retirement the strike paths use
//! ([`retire_consumed_completion_feedback`]); stale receipts through
//! [`ClaimReleaseReceiptLedger::retire_struck`]. Operator exchange text and
//! heading-shaped prose are never evidence and are never touched.
//!
//! [`retire_consumed_completion_feedback`]: crate::queue_consume::retire_consumed_completion_feedback

use std::collections::HashSet;

use agent_doc_document::queue_projection::{strip_in_progress_marker, strip_priority_markers};
use agent_doc_document::subagent_completion::{
    CompletionEvidence, completion_evidence, heading_item_text,
};
use agent_doc_element::element;
use agent_doc_element_queue::QueueItemIdentity;

use crate::claim_release_receipt::{ClaimReleaseReceipt, ClaimReleaseReceiptLedger};
use crate::document_queue::{self, QueueEntry};
use crate::queue_claim::{QueueClaimLedger, QueueClaimState, claim_identity};

/// Why a document cut cannot be judged: without every queue parsed, a live
/// row could be missed and its evidence wrongly called stale, so the repair
/// refuses instead of guessing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StaleEvidenceRefusal {
    /// The component structure does not parse.
    UnparseableComponents(String),
    /// A queue component body does not parse.
    UnparseableQueue(String),
    /// The document has no queue component, so no row can be proven absent.
    NoQueueComponent,
}

impl std::fmt::Display for StaleEvidenceRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnparseableComponents(err) => {
                write!(f, "the document's components do not parse ({err})")
            }
            Self::UnparseableQueue(err) => write!(f, "a queue component does not parse ({err})"),
            Self::NoQueueComponent => write!(
                f,
                "the document has no queue component, so no row can be proven absent"
            ),
        }
    }
}

impl std::error::Error for StaleEvidenceRefusal {}

/// The work identities whose evidence must stay: every live row of every
/// queue component (both its own text and the heading-normalized form a
/// completion cell records), plus every active claim attempt.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct KeptWork {
    pub identities: HashSet<QueueItemIdentity>,
    live_rows: HashSet<QueueItemIdentity>,
    claimed: HashSet<QueueItemIdentity>,
}

impl KeptWork {
    /// True when `identity` has a live row (not merely an active claim).
    pub fn has_live_row(&self, identity: &QueueItemIdentity) -> bool {
        self.live_rows.contains(identity)
    }

    pub fn keeps(&self, identity: &QueueItemIdentity) -> bool {
        self.identities.contains(identity)
    }

    /// True when an active (non-`Lost`) claim attempt owns `identity`.
    pub fn is_claimed(&self, identity: &QueueItemIdentity) -> bool {
        self.claimed.contains(identity)
    }
}

fn row_identities(text: &str) -> [QueueItemIdentity; 2] {
    [
        claim_identity(text),
        claim_identity(&heading_item_text(text)),
    ]
}

/// Derive [`KeptWork`] for `content` and its claim ledger. Refuses when any
/// queue component cannot be read, because a missed live row would make its
/// evidence look stale.
pub fn kept_work(
    content: &str,
    claims: &QueueClaimLedger,
) -> Result<KeptWork, StaleEvidenceRefusal> {
    let components = element::parse(content)
        .map_err(|err| StaleEvidenceRefusal::UnparseableComponents(format!("{err:#}")))?;
    let mut live_rows = HashSet::new();
    let mut queues = 0;
    for component in components.iter().filter(|c| c.name == "queue") {
        queues += 1;
        let entries = document_queue::parse(component.content(content))
            .map_err(|err| StaleEvidenceRefusal::UnparseableQueue(format!("{err:#}")))?;
        for entry in entries {
            let QueueEntry::Prompt(prompt) = entry else {
                continue;
            };
            let text = strip_in_progress_marker(&strip_priority_markers(prompt.text.trim()))
                .trim()
                .to_string();
            if !text.is_empty() {
                live_rows.extend(row_identities(&text));
            }
        }
    }
    if queues == 0 {
        return Err(StaleEvidenceRefusal::NoQueueComponent);
    }
    let mut identities = live_rows.clone();
    let mut claimed = HashSet::new();
    for claim in &claims.claims {
        let mut forms = vec![claim.identity.clone()];
        forms.extend(row_identities(&claim.item_text));
        for alias in &claim.tracked_item_aliases {
            forms.extend(row_identities(alias));
        }
        // An active attempt keeps its evidence even without a visible row; a
        // lost attempt keeps it only through a live row (already above) or a
        // live tracked alias of that row.
        let alias_live = claim.tracked_item_aliases.iter().any(|alias| {
            row_identities(alias)
                .iter()
                .any(|id| live_rows.contains(id))
        });
        if claim.state != QueueClaimState::Lost {
            claimed.extend(forms.iter().cloned());
        }
        if claim.state != QueueClaimState::Lost || alias_live {
            identities.extend(forms);
        }
    }
    Ok(KeptWork {
        identities,
        live_rows,
        claimed,
    })
}

/// What the repair would retire from one document.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StaleCompletionPlan {
    /// `content` without the retired completion cells.
    pub content: String,
    /// The retired cells, in document order: every stale cell plus every
    /// explicitly named untracked live-row cell.
    pub stale_cells: Vec<CompletionEvidence>,
    /// Cells kept because their work has a live row and their attempt is
    /// still tracked by a claim or receipt.
    pub kept_live_cells: usize,
    /// Cells kept because their work has a live row, although no claim
    /// attempt or receipt tracks their key any more. Such a cell may be a
    /// leftover painting `🔄` onto a NEW row with the same text, or the only
    /// durable evidence that a live row's work finished after its claim was
    /// pruned; only the operator can tell, so it is retired only when named
    /// by key.
    pub untracked_live_cells: Vec<CompletionEvidence>,
    /// Cells kept only because an active claim attempt still owns their work.
    pub kept_claimed_cells: usize,
    /// Explicitly named keys no cell carries (already retired, or a typo):
    /// reported, never an error, so a rerun stays a no-op.
    pub missing_keys: Vec<String>,
    /// Keys of the retired cells.
    pub retired_keys: HashSet<String>,
}

impl StaleCompletionPlan {
    pub fn is_empty(&self) -> bool {
        self.stale_cells.is_empty()
    }
}

/// Why an explicitly named cell key cannot be retired.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExplicitKeyRefusal {
    /// A claim attempt or receipt still tracks the key: it is live work
    /// evidence.
    Tracked(String),
    /// An active claim attempt owns the cell's work.
    Claimed(String),
}

impl std::fmt::Display for ExplicitKeyRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Tracked(key) => write!(
                f,
                "key {key} belongs to an attempt a claim or receipt still tracks; it is live work evidence"
            ),
            Self::Claimed(key) => write!(
                f,
                "key {key} names a cell whose work an active claim attempt owns"
            ),
        }
    }
}

impl std::error::Error for ExplicitKeyRefusal {}

/// Plan the retirement of every completion cell in `content` whose work is
/// not in `kept`, plus each cell named in `explicit_keys` that has a live row
/// but whose attempt `tracked_keys` (the completion keys of every claim
/// attempt and receipt) no longer contains and whose work no active claim
/// owns. Naming a tracked or claimed cell refuses the whole plan. Idempotent:
/// planning over the planned content finds nothing (a named key already
/// retired is listed in [`StaleCompletionPlan::missing_keys`]).
pub fn plan_stale_completion_cells(
    content: &str,
    kept: &KeptWork,
    tracked_keys: &HashSet<String>,
    explicit_keys: &HashSet<String>,
) -> Result<StaleCompletionPlan, ExplicitKeyRefusal> {
    let mut plan = StaleCompletionPlan {
        content: content.to_string(),
        ..StaleCompletionPlan::default()
    };
    let mut named = HashSet::new();
    for evidence in completion_evidence(content) {
        let identity = claim_identity(&evidence.item);
        let explicit = explicit_keys.contains(&evidence.key);
        if explicit {
            named.insert(evidence.key.clone());
        }
        let stale = if kept.is_claimed(&identity) {
            if explicit {
                return Err(ExplicitKeyRefusal::Claimed(evidence.key));
            }
            plan.kept_claimed_cells += 1;
            false
        } else if kept.has_live_row(&identity) || kept.keeps(&identity) {
            if tracked_keys.contains(&evidence.key) {
                if explicit {
                    return Err(ExplicitKeyRefusal::Tracked(evidence.key));
                }
                plan.kept_live_cells += 1;
                false
            } else if explicit {
                true
            } else {
                plan.untracked_live_cells.push(evidence.clone());
                false
            }
        } else {
            true
        };
        if stale {
            plan.retired_keys.insert(evidence.key.clone());
            plan.stale_cells.push(evidence);
        }
    }
    plan.missing_keys = explicit_keys
        .iter()
        .filter(|key| !named.contains(*key))
        .cloned()
        .collect();
    plan.missing_keys.sort();
    if !plan.retired_keys.is_empty() {
        plan.content = retire_cells_by_key(content, &plan.retired_keys);
    }
    Ok(plan)
}

/// `content` without the binary completion cells whose key is in `keys`,
/// through the strike paths' structural cell retirement (only attested cells
/// are ever presented or removed).
pub fn retire_cells_by_key(content: &str, keys: &HashSet<String>) -> String {
    agent_doc_document::subagent_completion::strip_completion_cells_matching(content, |evidence| {
        keys.contains(&evidence.key)
    })
}

/// The receipts whose work is not in `kept`.
pub fn stale_receipts<'a>(
    ledger: &'a ClaimReleaseReceiptLedger,
    kept: &KeptWork,
) -> Vec<&'a ClaimReleaseReceipt> {
    ledger
        .receipts
        .iter()
        .filter(|receipt| !kept.keeps(&receipt.identity))
        .collect()
}

/// Drop the receipts whose work is not in `kept`, through the strike path's
/// own [`ClaimReleaseReceiptLedger::retire_struck`]. Returns the dropped ones.
pub fn retire_stale_receipts(
    ledger: &mut ClaimReleaseReceiptLedger,
    kept: &KeptWork,
) -> Vec<ClaimReleaseReceipt> {
    let stale = stale_receipts(ledger, kept)
        .into_iter()
        .cloned()
        .collect::<Vec<_>>();
    if stale.is_empty() {
        return stale;
    }
    let struck = stale
        .iter()
        .map(|receipt| receipt.identity.clone())
        .collect::<HashSet<_>>();
    ledger.retire_struck(&struck, Some(&kept.identities));
    stale
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_document::subagent_completion::cell_with_response;

    fn doc(exchange: &str, queue: &str) -> String {
        format!(
            concat!(
                "---\nagent_doc_session: stale-repair\n---\n\n",
                "<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n\n",
                "## Run <!-- agent:run id=\"main\" subagents -->\n{queue}<!-- /agent:run -->\n\n",
                "<!-- agent:queue id=\"side\" -->\n- side row\n<!-- /agent:queue -->\n",
            ),
            exchange = exchange,
            queue = queue,
        )
    }

    fn cell(owner: &str, item: &str, key: &str) -> String {
        cell_with_response(owner, item, key, "Done.").unwrap()
    }

    fn claims() -> QueueClaimLedger {
        QueueClaimLedger::default()
    }

    fn plan(content: &str, kept: &KeptWork) -> StaleCompletionPlan {
        plan_stale_completion_cells(content, kept, &HashSet::new(), &HashSet::new()).unwrap()
    }

    fn items(plan: &StaleCompletionPlan) -> Vec<&str> {
        plan.stale_cells
            .iter()
            .map(|evidence| evidence.item.as_str())
            .collect()
    }

    /// `#stalecompletionrepair`: a cell whose row was struck before
    /// `#strikeretires` (or by hand) is retired; the live row's cell, a
    /// sibling queue's live row's cell, and operator prose stay byte for byte;
    /// planning again finds nothing.
    #[test]
    fn retires_only_cells_without_a_live_row_and_is_idempotent() {
        let stale = cell(
            "subagent:release_publish_patch",
            "release + publish",
            "k-stale",
        );
        let live = cell("subagent:live", "still open work", "k-live");
        let side = cell("subagent:side", "side row", "k-side");
        let prose = "### Re: Subagent completed: `subagent:x` returned work for queue item “release + publish” — agent-doc\nOperator note, not a binary cell.\n";
        let operator = "### Re: release — opus\n\nReleased and verified.\n";
        let content = doc(
            &format!("{stale}\n\n{live}\n\n{side}\n\n{prose}\n{operator}"),
            "- ~~release + publish~~\n- 🔄 still open work\n",
        );
        let kept = kept_work(&content, &claims()).unwrap();
        let plan = plan(&content, &kept);

        assert_eq!(
            plan.stale_cells
                .iter()
                .map(|evidence| evidence.key.as_str())
                .collect::<Vec<_>>(),
            ["k-stale"]
        );
        assert_eq!(plan.untracked_live_cells.len(), 2, "kept, only listed");
        assert_eq!(plan.retired_keys, HashSet::from(["k-stale".to_string()]));
        assert!(!plan.content.contains(&stale), "{}", plan.content);
        assert!(plan.content.contains(&live));
        assert!(plan.content.contains(&side));
        assert!(plan.content.contains(prose), "operator prose kept");
        assert!(plan.content.contains(operator), "operator response kept");
        assert!(plan.content.contains("- ~~release + publish~~\n"));

        let first = plan;
        let again = self::plan(
            &first.content,
            &kept_work(&first.content, &claims()).unwrap(),
        );
        assert!(again.is_empty());
        assert_eq!(again.content, first.content);
    }

    /// A row the operator deleted (rather than struck) leaves no live row
    /// either; its leftover cell is just as stale. A claimed live row keeps
    /// its cell whatever lifecycle marker it carries.
    #[test]
    fn deleted_row_cell_is_stale_and_claimed_row_cell_is_kept() {
        let gone = cell("subagent:gone", "deleted request", "k-gone");
        let claimed = cell("subagent:claimed", "claimed request", "k-claimed");
        let content = doc(
            &format!("{gone}\n\n{claimed}\n"),
            "- ☑️ [#subagent:claimed#2] claimed request\n",
        );
        let plan = plan(&content, &kept_work(&content, &claims()).unwrap());
        assert_eq!(items(&plan), ["deleted request"]);
        assert!(plan.content.contains(&claimed));
    }

    /// The heading neutralizes em dashes and curly quotes; the live row is
    /// still matched through its heading form, so its cell is never retired.
    #[test]
    fn heading_normalized_item_still_matches_its_live_row() {
        let item = "fix “quoted” thing — now";
        let live = cell("subagent:q", item, "k-q");
        let content = doc(&format!("{live}\n"), &format!("- {item}\n"));
        let plan = plan(&content, &kept_work(&content, &claims()).unwrap());
        assert!(plan.is_empty(), "{:?}", plan.stale_cells);
        assert_eq!(plan.untracked_live_cells.len(), 1);
    }

    /// An active claim attempt keeps its cell even when its row is not
    /// visible; a lost attempt does not.
    #[test]
    fn active_claim_keeps_cell_lost_claim_does_not() {
        let active = cell("subagent:a", "active work", "k-a");
        let lost = cell("subagent:l", "lost work", "k-l");
        let content = doc(&format!("{active}\n\n{lost}\n"), "- other\n");
        let mut ledger = QueueClaimLedger::default();
        for (item, state) in [
            ("active work", QueueClaimState::Reported),
            ("lost work", QueueClaimState::Lost),
        ] {
            ledger.claim(item, "subagent:x", 100, 600);
            ledger.claims.last_mut().unwrap().state = state;
        }
        let plan = plan(&content, &kept_work(&content, &ledger).unwrap());
        assert_eq!(items(&plan), ["lost work"]);
        assert_eq!(plan.kept_claimed_cells, 1);
    }

    /// The live 2026-10-26 shape: an old attempt's cell paints `🔄` onto a
    /// NEW row with the same text. It has a live row, so it is only listed,
    /// never retired by default. Named by key, an untracked cell is retired;
    /// a tracked or claimed cell, or an unknown key, is refused.
    #[test]
    fn explicit_key_retires_an_untracked_live_row_cell_only() {
        let old = cell(
            "subagent:release_publish_patch",
            "release + publish",
            "k-old",
        );
        let tracked = cell("subagent:now", "integrating now", "k-now");
        let content = doc(
            &format!("{old}\n\n{tracked}\n"),
            "- 🔄 release + publish\n- ☑️ [#subagent:now#1] integrating now\n",
        );
        let kept = kept_work(&content, &claims()).unwrap();
        let tracked_keys = HashSet::from(["k-now".to_string()]);
        let none = HashSet::new();

        let default = plan_stale_completion_cells(&content, &kept, &tracked_keys, &none).unwrap();
        assert!(default.is_empty());
        assert_eq!(default.kept_live_cells, 1);
        assert_eq!(
            default
                .untracked_live_cells
                .iter()
                .map(|evidence| evidence.key.as_str())
                .collect::<Vec<_>>(),
            ["k-old"]
        );

        let named = HashSet::from(["k-old".to_string()]);
        let plan = plan_stale_completion_cells(&content, &kept, &tracked_keys, &named).unwrap();
        assert_eq!(items(&plan), ["release + publish"]);
        assert!(!plan.content.contains(&old));
        assert!(plan.content.contains(&tracked));
        assert!(
            plan.content.contains("- 🔄 release + publish\n"),
            "rows are untouched"
        );

        assert_eq!(
            plan_stale_completion_cells(
                &content,
                &kept,
                &tracked_keys,
                &HashSet::from(["k-now".to_string()])
            ),
            Err(ExplicitKeyRefusal::Tracked("k-now".to_string()))
        );
        // Rerun after retirement: the key is reported missing, not an error.
        let rerun =
            plan_stale_completion_cells(&plan.content, &kept, &tracked_keys, &named).unwrap();
        assert!(rerun.is_empty());
        assert_eq!(rerun.missing_keys, ["k-old"]);
        assert_eq!(rerun.content, plan.content);

        let mut ledger = QueueClaimLedger::default();
        ledger.claim("release + publish", "subagent:new", 100, 600);
        let claimed = kept_work(&content, &ledger).unwrap();
        assert_eq!(
            plan_stale_completion_cells(
                &content,
                &claimed,
                &tracked_keys,
                &HashSet::from(["k-old".to_string()])
            ),
            Err(ExplicitKeyRefusal::Claimed("k-old".to_string()))
        );
    }

    /// Without every queue parsed, no row can be proven absent.
    #[test]
    fn refuses_without_a_queue_component() {
        let content = "---\nagent_doc_session: x\n---\n\n<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n";
        assert_eq!(
            kept_work(content, &claims()),
            Err(StaleEvidenceRefusal::NoQueueComponent)
        );
    }
}
