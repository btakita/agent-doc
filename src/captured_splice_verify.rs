//! `#activateinstalledjetbrai` — verify, from receipts alone, that a captured
//! local editor edit recovered across an **independently advanced** canonical
//! response.
//!
//! This is the automation that replaces the human eyeball on that gate. The live
//! half — producing a genuine operator-authored editor op — is driven by
//! `scripts/xdotool-live-verify.sh captured-splice`; the evaluation is here,
//! because deciding whether the receipt trail proves the property is
//! deterministic and therefore belongs in the binary.
//!
//! ## Why `editor_op_epoch_closed` cannot answer this
//!
//! The gate was previously reasoned about by reading `editor_op_epoch_closed
//! cause=` and concluding, from a window in which every event said
//! `cause=non_operator_projection`, that no recovery carried an operator edit.
//! That inference cannot be drawn: `agent_doc_clear_editor_op_epoch` is the only
//! producer of that event and it writes exactly one cause, so
//! `non_operator_projection` is the field's **only** possible value. It records
//! "an epoch was cleared ahead of a non-operator mutation", never "this epoch
//! carried operator ops". A single-valued field discriminates nothing.
//!
//! The receipt that *does* discriminate is `editor_op_capture_proof`
//! (`#opcaptureliveread`), whose `operator_ops=<n>` is the operator/non-operator
//! classification split as the reporter saw it.
//!
//! ## Two accepted recovery shapes
//!
//! The property is "an operator edit captured against a canonical text that was
//! then advanced independently survives into the converged canonical". There are
//! two receipt-provable mechanisms that deliver it, and the verifier accepts
//! either. Accepting only one of them asserted a MECHANISM, not the property: the
//! 2026-10-11 live run on 0.35.490 recovered edit two through the CRDT forward
//! path, never ran the fallback, and was failed for it.
//!
//! ### Fallback triple (`captured-local-splice-recovery`)
//!
//! Three receipts for one document, in timestamp order:
//!
//! 1. a `controller_crdt_current_text source=captured-local-splice-recovery
//!    status=current` read — the splice recovery observed canonical text;
//! 2. an `editor_op_capture_proof` with `operator_ops >= 1` and
//!    `shadow_replay=agreed` — an operator-authored edit was captured and its
//!    shadow replay agreed, so the ops were kept rather than discarded;
//! 3. a later splice-recovery `status=current` read whose `text_hash` **differs**
//!    from (1) — the canonical response advanced independently across it.
//!
//! ### CRDT forward (`crdt_forward`)
//!
//! Four receipts for one document, in timestamp order:
//!
//! 1. a `crdt_response_cell_add applied=true delivery_converged=false` — the
//!    canonical response advanced while editor delivery was RETAINED, so the
//!    editor replica is stale relative to canonical. An advance that converged
//!    immediately is the healthy path and proves nothing about recovery;
//! 2. a later `editor_op_capture_proof` with `operator_ops >= 1`,
//!    `shadow_replay=agreed`, and a `merge_base` that is a real hash and is NOT
//!    the prefix of that advance's post-advance `content_hash` — the edit was
//!    stamped on a base that does not contain the advance;
//! 3. a later `crdt_document_op_delta_ingested outcome=Applied { changed: true }`
//!    — the editor's delta was merged into canonical rather than refused,
//!    quarantined, or found already present;
//! 4. a later `crdt_commit_barrier ready=true visible_delivery_projected=true` —
//!    delivery converged after the merge, so the editor holds canonical again.
//!
//! Every failure names which link is missing, because "no proof" and "no
//! evidence class at all" are different diagnoses and conflating them is what
//! kept this gate open on a guess. When neither shape is proven the failure
//! reports both shapes' missing links.

use agent_doc_turn::op_log::OpsLogEvent;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// The `source=` tag the splice-recovery read site stamps on its
/// `controller_crdt_current_text` receipt.
const SPLICE_RECOVERY_SOURCE: &str = "source=captured-local-splice-recovery";

/// The `Debug` rendering `crdt_document_op_delta_ingested` writes for a frame
/// that changed canonical. `Applied { changed: false }` is a duplicate the
/// canonical already held, which merged nothing.
const DELTA_APPLIED_CHANGED: &str = "outcome=Applied { changed: true }";

/// One splice-recovery observation of canonical text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SpliceRead {
    /// Position in the document's filtered receipt sequence (ordering only).
    index: usize,
    status: String,
    text_hash: String,
    text_len: usize,
}

/// One `editor_op_capture_proof` receipt.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureProof {
    index: usize,
    epoch_generation: String,
    operator_ops: usize,
    non_operator_ops: usize,
    shadow_replay: String,
    merge_base: String,
}

/// One `crdt_response_cell_add` — a canonical response advance.
#[derive(Debug, Clone, PartialEq, Eq)]
struct ResponseAdvance {
    index: usize,
    applied: bool,
    delivery_converged: bool,
    /// The post-advance canonical text hash.
    content_hash: String,
}

/// One `crdt_document_op_delta_ingested` — an editor delta reaching canonical.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DeltaIngest {
    index: usize,
    applied_changed: bool,
    outcome: String,
}

/// One `crdt_commit_barrier` observation.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CommitBarrier {
    index: usize,
    /// `ready=true visible_delivery_projected=true`: editors hold canonical.
    converged: bool,
}

/// One `editor_op_capture_refused`.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureRefusal {
    index: usize,
    reason: String,
}

/// Every receipt kind either recovery shape reads, for one document.
#[derive(Debug, Default)]
struct Receipts {
    splice_reads: Vec<SpliceRead>,
    proofs: Vec<CaptureProof>,
    advances: Vec<ResponseAdvance>,
    ingests: Vec<DeltaIngest>,
    barriers: Vec<CommitBarrier>,
    refusals: Vec<CaptureRefusal>,
}

impl Receipts {
    fn parse(doc_lines: &[&str]) -> Self {
        let mut receipts = Self::default();
        for (index, line) in doc_lines.iter().enumerate() {
            if OpsLogEvent::ControllerCrdtCurrentText.is_line(line)
                && line.contains(SPLICE_RECOVERY_SOURCE)
            {
                receipts.splice_reads.push(SpliceRead {
                    index,
                    status: field(line, "status").unwrap_or_else(|| "unknown".to_string()),
                    text_hash: field(line, "text_hash").unwrap_or_default(),
                    text_len: field(line, "text_len")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                });
            } else if OpsLogEvent::EditorOpCaptureProof.is_line(line) {
                receipts.proofs.push(CaptureProof {
                    index,
                    epoch_generation: field(line, "epoch_generation")
                        .unwrap_or_else(|| "unknown".to_string()),
                    operator_ops: field(line, "operator_ops")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                    non_operator_ops: field(line, "non_operator_ops")
                        .and_then(|value| value.parse().ok())
                        .unwrap_or(0),
                    shadow_replay: field(line, "shadow_replay")
                        .unwrap_or_else(|| "unknown".to_string()),
                    merge_base: field(line, "merge_base")
                        .unwrap_or_else(|| "unavailable".to_string()),
                });
            } else if OpsLogEvent::CrdtResponseCellAdd.is_line(line) {
                receipts.advances.push(ResponseAdvance {
                    index,
                    applied: field(line, "applied").as_deref() == Some("true"),
                    delivery_converged: field(line, "delivery_converged").as_deref()
                        == Some("true"),
                    content_hash: field(line, "content_hash").unwrap_or_default(),
                });
            } else if OpsLogEvent::CrdtDocumentOpDeltaIngested.is_line(line) {
                receipts.ingests.push(DeltaIngest {
                    index,
                    applied_changed: line.contains(DELTA_APPLIED_CHANGED),
                    outcome: delta_outcome(line),
                });
            } else if OpsLogEvent::CrdtCommitBarrier.is_line(line) {
                receipts.barriers.push(CommitBarrier {
                    index,
                    converged: field(line, "ready").as_deref() == Some("true")
                        && field(line, "visible_delivery_projected").as_deref() == Some("true"),
                });
            } else if OpsLogEvent::EditorOpCaptureRefused.is_line(line) {
                receipts.refusals.push(CaptureRefusal {
                    index,
                    reason: field(line, "reason").unwrap_or_else(|| "unknown".to_string()),
                });
            }
        }
        receipts
    }
}

/// The fallback triple: an agreed operator capture between two splice-recovery
/// reads that observed different canonical text.
#[derive(Debug, Clone, PartialEq, Eq)]
struct FallbackTriple {
    splice_current_reads: usize,
    distinct_text_hashes: usize,
    before: SpliceRead,
    proof: CaptureProof,
    after: SpliceRead,
}

/// The CRDT-forward chain: retained advance → pre-advance-base operator capture
/// → applied delta → converged delivery.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CrdtForwardChain {
    advance: ResponseAdvance,
    proof: CaptureProof,
    ingest: DeltaIngest,
    converged: CommitBarrier,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum RecoveryShape {
    Fallback(FallbackTriple),
    CrdtForward(CrdtForwardChain),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedSpliceVerification {
    doc_tag: String,
    ops_log: PathBuf,
    splice_reads_total: usize,
    capture_proofs: usize,
    operator_proofs: usize,
    shape: RecoveryShape,
}

pub fn run(file: &Path) -> Result<()> {
    let report = verify(file)?;
    println!(
        "captured-splice-recovery verification ok for {}",
        file.display()
    );
    println!("ops_log={}", report.ops_log.display());
    println!(
        "{} splice_reads={} capture_proofs={} operator_proofs={}",
        report.doc_tag, report.splice_reads_total, report.capture_proofs, report.operator_proofs,
    );
    match &report.shape {
        RecoveryShape::Fallback(triple) => println!(
            "shape=fallback proof=operator_edit_recovered_across_advanced_canonical \
             splice_current_reads={} distinct_text_hashes={} before_hash={} before_len={} \
             epoch_generation={} operator_ops={} non_operator_ops={} shadow_replay={} \
             after_hash={} after_len={}",
            triple.splice_current_reads,
            triple.distinct_text_hashes,
            triple.before.text_hash,
            triple.before.text_len,
            triple.proof.epoch_generation,
            triple.proof.operator_ops,
            triple.proof.non_operator_ops,
            triple.proof.shadow_replay,
            triple.after.text_hash,
            triple.after.text_len,
        ),
        RecoveryShape::CrdtForward(chain) => println!(
            "shape=crdt_forward proof=operator_edit_recovered_across_advanced_canonical \
             advance_content_hash={} epoch_generation={} operator_ops={} non_operator_ops={} \
             shadow_replay={} merge_base={} ingest_outcome={} delivery_converged=true",
            chain.advance.content_hash,
            chain.proof.epoch_generation,
            chain.proof.operator_ops,
            chain.proof.non_operator_ops,
            chain.proof.shadow_replay,
            chain.proof.merge_base,
            chain.ingest.outcome,
        ),
    }
    Ok(())
}

fn verify(file: &Path) -> Result<CapturedSpliceVerification> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let project_root = agent_doc_fs::find_project_root(&canonical)
        .with_context(|| format!("no .agent-doc project root found for {}", file.display()))?;
    let ops_log = project_root.join(".agent-doc/logs/ops.log");
    let log = std::fs::read_to_string(&ops_log)
        .with_context(|| format!("failed to read {}", ops_log.display()))?;

    let stem = canonical
        .file_stem()
        .and_then(|name| name.to_str())
        .context("document path has no UTF-8 file stem")?;
    let doc_tag = format!("doc={stem}");
    // Exact token match: `doc=scratch` must not admit `doc=scratch-2`.
    let doc_lines: Vec<&str> = log
        .lines()
        .filter(|line| line.split_ascii_whitespace().any(|token| token == doc_tag))
        .collect();
    if doc_lines.is_empty() {
        bail!(
            "ops.log has no receipts for {doc_tag} in {}; open the document in a live editor and \
             run a response cycle before verifying",
            ops_log.display()
        );
    }

    let receipts = Receipts::parse(&doc_lines);
    let shape = match (
        prove_fallback_triple(&receipts, &doc_tag, &ops_log),
        prove_crdt_forward(&receipts, &doc_tag),
    ) {
        (Ok(triple), _) => RecoveryShape::Fallback(triple),
        (Err(_), Ok(chain)) => RecoveryShape::CrdtForward(chain),
        (Err(fallback), Err(forward)) => bail!(
            "no accepted recovery shape is proven for {doc_tag} in {}.\n\
             - fallback (captured-local-splice-recovery): {fallback}\n\
             - crdt_forward (retained advance -> operator capture -> applied delta -> converged \
             delivery): {forward}",
            ops_log.display(),
        ),
    };

    Ok(CapturedSpliceVerification {
        doc_tag,
        ops_log,
        splice_reads_total: receipts.splice_reads.len(),
        capture_proofs: receipts.proofs.len(),
        operator_proofs: receipts
            .proofs
            .iter()
            .filter(|proof| proof.operator_ops >= 1)
            .count(),
        shape,
    })
}

/// The fallback shape, or the name of its missing link.
fn prove_fallback_triple(
    receipts: &Receipts,
    doc_tag: &str,
    ops_log: &Path,
) -> std::result::Result<FallbackTriple, String> {
    let splice_reads = &receipts.splice_reads;
    let proofs = &receipts.proofs;
    if splice_reads.is_empty() {
        return Err(format!(
            "no `{}` receipts with `{SPLICE_RECOVERY_SOURCE}` for {doc_tag} in {} — the \
             captured-local-splice-recovery path never ran, so there is nothing for an operator \
             edit to have recovered across",
            OpsLogEvent::ControllerCrdtCurrentText,
            ops_log.display(),
        ));
    }

    let current_reads: Vec<&SpliceRead> = splice_reads
        .iter()
        .filter(|read| read.status == "current" && !read.text_hash.is_empty())
        .collect();
    if current_reads.is_empty() {
        return Err(format!(
            "{} captured-local-splice-recovery receipt(s) for {doc_tag}, but none resolved \
             `status=current` (observed: {}) — the recovery never saw canonical text, so the \
             editor replica, not op capture, is the blocker; see the missing-replica recovery \
             path in {}",
            splice_reads.len(),
            describe_statuses(splice_reads),
            ops_log.display(),
        ));
    }

    let distinct_text_hashes = {
        let mut hashes: Vec<&str> = current_reads
            .iter()
            .map(|read| read.text_hash.as_str())
            .collect();
        hashes.sort_unstable();
        hashes.dedup();
        hashes.len()
    };
    if distinct_text_hashes < 2 {
        return Err(format!(
            "{} captured-local-splice-recovery `status=current` read(s) for {doc_tag} all observed \
             the SAME canonical text (hash={}) — the canonical response never advanced between two \
             recoveries, so this run cannot prove recovery ACROSS an independent advance. Drive an \
             agent-doc response cycle between two operator edits and re-verify.",
            current_reads.len(),
            current_reads[0].text_hash,
        ));
    }

    let operator_proofs: Vec<&CaptureProof> = proofs
        .iter()
        .filter(|proof| proof.operator_ops >= 1)
        .collect();
    if operator_proofs.is_empty() {
        return Err(format!(
            "no `{}` receipt with `operator_ops>=1` for {doc_tag} in {} — {} capture proof(s) \
             present, all classifying every op as non-operator{}. The canonical text DID advance \
             across {} splice recoveries, so the missing link is a captured operator edit, not the \
             recovery path. Note: `editor_op_epoch_closed cause=` cannot answer this — \
             `non_operator_projection` is its only possible value.",
            OpsLogEvent::EditorOpCaptureProof,
            ops_log.display(),
            proofs.len(),
            describe_non_operator_proofs(proofs),
            current_reads.len(),
        ));
    }

    let agreed: Vec<&&CaptureProof> = operator_proofs
        .iter()
        .filter(|proof| proof.shadow_replay == "agreed")
        .collect();
    if agreed.is_empty() {
        return Err(format!(
            "{} `{}` receipt(s) for {doc_tag} carry `operator_ops>=1`, but none report \
             `shadow_replay=agreed` (observed: {}) — a disagreeing shadow replay DISCARDS the \
             captured burst, so those operator edits never reached the merge and cannot have \
             recovered",
            operator_proofs.len(),
            OpsLogEvent::EditorOpCaptureProof,
            describe_replays(&operator_proofs),
        ));
    }

    // The property is ordered: an operator capture must sit BETWEEN two
    // splice-recovery reads that observed different canonical text. A proof
    // before every read, or after every read, proves nothing about recovery
    // across an advance.
    let Some((before, proof, after)) = find_proving_triple(&current_reads, &agreed) else {
        return Err(format!(
            "for {doc_tag} the receipts exist but never line up: {} splice-recovery \
             `status=current` read(s) across {distinct_text_hashes} distinct canonical text(s) and \
             {} agreed operator capture proof(s), but no proof falls BETWEEN two reads that \
             observed different text. Recovery across an independent advance is an ordering \
             property; an operator edit captured entirely before or after every advance does not \
             establish it.",
            current_reads.len(),
            agreed.len(),
        ));
    };

    Ok(FallbackTriple {
        splice_current_reads: current_reads.len(),
        distinct_text_hashes,
        before,
        proof,
        after,
    })
}

/// The earliest `(before, proof, after)` where an agreed operator capture sits
/// between two splice-recovery reads that observed different canonical text.
fn find_proving_triple(
    current_reads: &[&SpliceRead],
    agreed: &[&&CaptureProof],
) -> Option<(SpliceRead, CaptureProof, SpliceRead)> {
    for proof in agreed {
        let before = current_reads
            .iter()
            .rfind(|read| read.index < proof.index)?;
        let after = current_reads
            .iter()
            .find(|read| read.index > proof.index && read.text_hash != before.text_hash);
        if let Some(after) = after {
            return Some(((*before).clone(), (**proof).clone(), (*after).clone()));
        }
    }
    None
}

/// A `merge_base` that names a real base. `unavailable` means the reporter held
/// no base hash, so nothing proves which canonical the edit was stamped on.
fn merge_base_is_known(merge_base: &str) -> bool {
    !merge_base.is_empty() && merge_base.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// How far one (advance, capture) candidate got along the CRDT-forward chain.
/// Ordered: a later stage is strictly more progress.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum ForwardStage {
    /// An agreed operator capture follows a retained advance, but its base is
    /// unknown or already contains the advance.
    CaptureBase,
    /// The capture is on a pre-advance base, but no applied delta follows it.
    Ingest,
    /// The delta applied, but delivery never converged afterwards.
    Converged,
}

/// The CRDT-forward shape, or the name of its missing link.
fn prove_crdt_forward(
    receipts: &Receipts,
    doc_tag: &str,
) -> std::result::Result<CrdtForwardChain, String> {
    let advance_event = OpsLogEvent::CrdtResponseCellAdd;
    let applied: Vec<&ResponseAdvance> = receipts
        .advances
        .iter()
        .filter(|advance| advance.applied && !advance.content_hash.is_empty())
        .collect();
    if applied.is_empty() {
        return Err(format!(
            "no `{advance_event} applied=true` receipt for {doc_tag} ({} unapplied) — the canonical \
             response never advanced through the response-cell path, so there is no advance for \
             an editor delta to have been forwarded across",
            receipts.advances.len(),
        ));
    }
    let retained: Vec<&ResponseAdvance> = applied
        .iter()
        .copied()
        .filter(|advance| !advance.delivery_converged)
        .collect();
    if retained.is_empty() {
        return Err(format!(
            "{} applied `{advance_event}` receipt(s) for {doc_tag}, but every one reported \
             `delivery_converged=true` — the advance never RETAINED editor delivery, so the \
             editor already held it and an edit typed afterwards recovered across nothing. A \
             healthy-path delivery is not the stale-replica property.",
            applied.len(),
        ));
    }
    let first_retained = retained[0].index;

    let after_advance: Vec<&CaptureProof> = receipts
        .proofs
        .iter()
        .filter(|proof| proof.index > first_retained)
        .collect();
    let refused_after: Vec<&CaptureRefusal> = receipts
        .refusals
        .iter()
        .filter(|refusal| refusal.index > first_retained)
        .collect();
    if after_advance.is_empty() {
        return Err(format!(
            "no `{}` after the retained advance for {doc_tag} — {}",
            OpsLogEvent::EditorOpCaptureProof,
            describe_refusals_after_advance(&refused_after),
        ));
    }
    let operator_after: Vec<&CaptureProof> = after_advance
        .iter()
        .copied()
        .filter(|proof| proof.operator_ops >= 1)
        .collect();
    if operator_after.is_empty() {
        return Err(format!(
            "{} `{}` receipt(s) follow the retained advance for {doc_tag}, but none carry \
             `operator_ops>=1` — no operator edit was captured against the stale replica; {}",
            after_advance.len(),
            OpsLogEvent::EditorOpCaptureProof,
            describe_refusals_after_advance(&refused_after),
        ));
    }
    let agreed_after: Vec<&CaptureProof> = operator_after
        .iter()
        .copied()
        .filter(|proof| proof.shadow_replay == "agreed")
        .collect();
    if agreed_after.is_empty() {
        return Err(format!(
            "{} operator capture(s) follow the retained advance for {doc_tag}, but none report \
             `shadow_replay=agreed` (observed: {}) — a disagreeing shadow replay DISCARDS the \
             burst, so no operator delta was forwarded",
            operator_after.len(),
            describe_replays(&operator_after),
        ));
    }

    let mut furthest: Option<(ForwardStage, &ResponseAdvance, &CaptureProof)> = None;
    for proof in &agreed_after {
        // The advance this capture had to cross is the latest retained one
        // before it.
        let Some(advance) = retained
            .iter()
            .copied()
            .rfind(|advance| advance.index < proof.index)
        else {
            continue;
        };
        let stage = if !merge_base_is_known(&proof.merge_base)
            || advance.content_hash.starts_with(&proof.merge_base)
        {
            ForwardStage::CaptureBase
        } else if let Some(ingest) = receipts
            .ingests
            .iter()
            .find(|ingest| ingest.index > proof.index && ingest.applied_changed)
        {
            if let Some(converged) = receipts
                .barriers
                .iter()
                .find(|barrier| barrier.index > ingest.index && barrier.converged)
            {
                return Ok(CrdtForwardChain {
                    advance: advance.clone(),
                    proof: (*proof).clone(),
                    ingest: ingest.clone(),
                    converged: converged.clone(),
                });
            }
            ForwardStage::Converged
        } else {
            ForwardStage::Ingest
        };
        if furthest.is_none_or(|(best, _, _)| stage > best) {
            furthest = Some((stage, advance, proof));
        }
    }

    let Some((stage, advance, proof)) = furthest else {
        // Unreachable in practice: every agreed capture follows `first_retained`.
        return Err(format!(
            "no agreed operator capture for {doc_tag} pairs with a preceding retained advance"
        ));
    };
    Err(match stage {
        ForwardStage::CaptureBase => {
            if merge_base_is_known(&proof.merge_base) {
                format!(
                    "every agreed operator capture after the retained advance for {doc_tag} was \
                     stamped on `merge_base={}`, the POST-advance canonical (advance \
                     content_hash={}) — the editor already held the advance, so its edit never \
                     had to recover across it",
                    proof.merge_base, advance.content_hash,
                )
            } else {
                format!(
                    "the agreed operator capture(s) after the retained advance for {doc_tag} carry \
                     `merge_base={}` — with no base hash nothing proves the edit was stamped on a \
                     pre-advance base",
                    proof.merge_base,
                )
            }
        }
        ForwardStage::Ingest => format!(
            "an agreed operator capture on pre-advance base `merge_base={}` follows the retained \
             advance for {doc_tag}, but no `{} {DELTA_APPLIED_CHANGED}` follows it (ingests after \
             it: {}) — the captured delta never merged into canonical",
            proof.merge_base,
            OpsLogEvent::CrdtDocumentOpDeltaIngested,
            describe_ingests_after(&receipts.ingests, proof.index),
        ),
        ForwardStage::Converged => format!(
            "the operator delta captured on `merge_base={}` merged into canonical for {doc_tag}, \
             but no later `{} ready=true visible_delivery_projected=true` — delivery never \
             converged, so the editor was never shown to hold the recovered canonical",
            proof.merge_base,
            OpsLogEvent::CrdtCommitBarrier,
        ),
    })
}

/// Read a `key=value` field's value from an ops-log line.
///
/// Whitespace-delimited and exact on the key, so `text_hash=` cannot be
/// satisfied by `base_text_hash=` and `operator_ops=` cannot be satisfied by
/// `non_operator_ops=`.
fn field(line: &str, key: &str) -> Option<String> {
    line.split_ascii_whitespace().find_map(|token| {
        token
            .split_once('=')
            .filter(|(name, _)| *name == key)
            .map(|(_, value)| value.to_string())
    })
}

fn describe_statuses(reads: &[SpliceRead]) -> String {
    let mut statuses: Vec<&str> = reads.iter().map(|read| read.status.as_str()).collect();
    statuses.sort_unstable();
    statuses.dedup();
    statuses.join(", ")
}

fn describe_non_operator_proofs(proofs: &[CaptureProof]) -> String {
    if proofs.is_empty() {
        return " (no capture proof receipts at all — the reporter chain is unobserved, which is a \
                different diagnosis from a proof that classified every op as non-operator; \
                `agent-doc verify-op-capture <FILE>` names the producer side)"
            .to_string();
    }
    let total_non_operator: usize = proofs.iter().map(|proof| proof.non_operator_ops).sum();
    format!(" ({total_non_operator} non-operator op(s) across those proofs)")
}

fn describe_replays(proofs: &[&CaptureProof]) -> String {
    let mut replays: Vec<&str> = proofs
        .iter()
        .map(|proof| proof.shadow_replay.as_str())
        .collect();
    replays.sort_unstable();
    replays.dedup();
    replays.join(", ")
}

/// The `outcome=` value of a `crdt_document_op_delta_ingested` line. The
/// producer writes a `Debug` rendering that may contain spaces
/// (`Applied { changed: true }`), so it runs up to the next `key=` field.
fn delta_outcome(line: &str) -> String {
    let Some((_, rest)) = line.split_once(" outcome=") else {
        return "unknown".to_string();
    };
    let end = rest.find(" frame_lineage=").unwrap_or(rest.len());
    rest[..end].trim().to_string()
}

fn describe_refusals_after_advance(refusals: &[&CaptureRefusal]) -> String {
    if refusals.is_empty() {
        return "and no capture was refused either: the reporter chain never ran after the \
                advance, so edit two never reached op capture"
            .to_string();
    }
    let mut reasons: Vec<&str> = refusals
        .iter()
        .map(|refusal| refusal.reason.as_str())
        .collect();
    reasons.sort_unstable();
    reasons.dedup();
    format!(
        "{} capture(s) after it were REFUSED (`{}` reason(s): {}), so edit two was never kept",
        refusals.len(),
        OpsLogEvent::EditorOpCaptureRefused,
        reasons.join(", "),
    )
}

fn describe_ingests_after(ingests: &[DeltaIngest], after: usize) -> String {
    let mut outcomes: Vec<&str> = ingests
        .iter()
        .filter(|ingest| ingest.index > after)
        .map(|ingest| ingest.outcome.as_str())
        .collect();
    if outcomes.is_empty() {
        return "none".to_string();
    }
    outcomes.sort_unstable();
    outcomes.dedup();
    outcomes.join(", ")
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = "scratch";

    fn splice_read(status: &str, hash: &str, len: usize) -> String {
        if status == "current" {
            format!(
                "[2026-09-27T00:00:00Z] controller_crdt_current_text file=/tmp/{DOC}.md \
                 source=captured-local-splice-recovery status=current authority=cp_model \
                 text_len={len} text_hash={hash} live_editors=1 delivery_converged=true \
                 delivery_version=3 doc={DOC}\n"
            )
        } else {
            format!(
                "[2026-09-27T00:00:00Z] controller_crdt_current_text file=/tmp/{DOC}.md \
                 source=captured-local-splice-recovery status={status} authority=cp_model \
                 doc={DOC}\n"
            )
        }
    }

    fn capture_proof(operator_ops: usize, non_operator_ops: usize, replay: &str) -> String {
        format!(
            "[2026-09-27T00:00:00Z] editor_op_capture_proof epoch_generation=7 \
             operator_ops={operator_ops} non_operator_ops={non_operator_ops} \
             shadow_replay={replay} merge_base=abcdef012345 #opcaptureliveread doc={DOC}\n"
        )
    }

    /// Write `lines` as the project ops.log and return the scratch document path.
    fn project_with_log(dir: &std::path::Path, lines: &str) -> PathBuf {
        project_with_named_log(dir, DOC, lines)
    }

    fn project_with_named_log(dir: &std::path::Path, stem: &str, lines: &str) -> PathBuf {
        std::fs::create_dir_all(dir.join(".agent-doc/logs")).unwrap();
        std::fs::write(dir.join(".agent-doc/logs/ops.log"), lines).unwrap();
        let doc = dir.join(format!("{stem}.md"));
        std::fs::write(&doc, "# scratch\n").unwrap();
        doc
    }

    #[test]
    fn proving_triple_passes_and_names_the_advance() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}{}",
                splice_read("current", "aaaa1111", 42885),
                capture_proof(2, 0, "agreed"),
                splice_read("current", "bbbb2222", 42900),
            ),
        );

        let report = verify(&doc).unwrap();
        let RecoveryShape::Fallback(triple) = &report.shape else {
            panic!("expected the fallback shape: {:?}", report.shape);
        };
        assert_eq!(triple.before.text_hash, "aaaa1111");
        assert_eq!(triple.after.text_hash, "bbbb2222");
        assert_eq!(triple.proof.operator_ops, 2);
        assert_eq!(triple.distinct_text_hashes, 2);
        assert_eq!(report.operator_proofs, 1);
    }

    /// The state `#activateinstalledjetbrai` was actually in: the recovery path
    /// is healthy and the canonical text advances, but nothing captured an
    /// operator op. The failure must say so — and must not offer
    /// `editor_op_epoch_closed` as evidence either way.
    #[test]
    fn advanced_canonical_without_an_operator_capture_fails_on_the_capture() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}{}",
                splice_read("current", "aaaa1111", 42885),
                capture_proof(0, 4, "agreed"),
                splice_read("current", "bbbb2222", 42900),
            ),
        );

        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(err.contains("operator_ops>=1"), "{err}");
        assert!(err.contains("4 non-operator op(s)"), "{err}");
        assert!(
            err.contains("non_operator_projection` is its only possible value"),
            "the failure must refuse `editor_op_epoch_closed cause=` as evidence: {err}"
        );
    }

    /// No capture proofs at all is an unobserved reporter chain, not a proof
    /// that classified everything as non-operator.
    #[test]
    fn absent_capture_proofs_report_an_unobserved_reporter_chain() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}",
                splice_read("current", "aaaa1111", 42885),
                splice_read("current", "bbbb2222", 42900),
            ),
        );

        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(err.contains("reporter chain is unobserved"), "{err}");
        assert!(err.contains("verify-op-capture"), "{err}");
    }

    /// The live sampleorders / agent-doc-bugs shape: splice recoveries ran,
    /// but every one of them was refused a replica, so the blocker is upstream.
    #[test]
    fn splice_reads_that_never_saw_canonical_text_blame_the_replica() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}{}",
                splice_read("editor_attached_model_missing", "", 0),
                splice_read("editor_attached_model_missing", "", 0),
                capture_proof(3, 0, "agreed"),
            ),
        );

        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(err.contains("none resolved `status=current`"), "{err}");
        assert!(err.contains("editor_attached_model_missing"), "{err}");
        assert!(err.contains("not op capture, is the blocker"), "{err}");
    }

    /// Two recoveries over identical canonical text prove recovery, but not
    /// recovery ACROSS an independent advance — which is the whole gate.
    #[test]
    fn unadvanced_canonical_text_fails_even_with_an_operator_capture() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}{}",
                splice_read("current", "aaaa1111", 42885),
                capture_proof(2, 0, "agreed"),
                splice_read("current", "aaaa1111", 42885),
            ),
        );

        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(err.contains("never advanced between two"), "{err}");
    }

    /// A disagreeing shadow replay discards the burst, so those ops never
    /// reached the merge.
    #[test]
    fn disagreeing_shadow_replay_is_not_a_recovered_operator_edit() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}{}",
                splice_read("current", "aaaa1111", 42885),
                capture_proof(2, 0, "disagreed"),
                splice_read("current", "bbbb2222", 42900),
            ),
        );

        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(err.contains("shadow_replay=agreed"), "{err}");
        assert!(err.contains("DISCARDS the captured burst"), "{err}");
    }

    /// Ordering is load-bearing: an operator capture after every advance never
    /// recovered across one.
    #[test]
    fn operator_capture_after_every_advance_does_not_prove_the_property() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}{}",
                splice_read("current", "aaaa1111", 42885),
                splice_read("current", "bbbb2222", 42900),
                capture_proof(2, 0, "agreed"),
            ),
        );

        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(err.contains("no proof falls BETWEEN two reads"), "{err}");
    }

    /// Receipts for another document must not satisfy this document's gate.
    #[test]
    fn receipts_for_another_document_do_not_count() {
        let dir = tempfile::TempDir::new().unwrap();
        let other = splice_read("current", "cccc3333", 100).replace("doc=scratch", "doc=elsewhere");
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}{}",
                other.clone(),
                other.replace("cccc3333", "dddd4444"),
                capture_proof(2, 0, "agreed").replace("doc=scratch", "doc=elsewhere"),
            ),
        );

        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(err.contains("no receipts for doc=scratch"), "{err}");
    }

    // --- crdt_forward: the 2026-10-11 live shape ------------------------------

    /// Receipts trimmed verbatim from the 2026-10-11 live run on 0.35.490
    /// (upstream recipe, retained advance with `delivery_converged=false`):
    /// edit two was captured on the pre-advance base `d232ee81c5ad`, forwarded
    /// as a CRDT delta, applied, the plugin re-bootstrapped, and delivery
    /// converged. The fallback `captured-local-splice-recovery` read never ran.
    const LIVE_FORWARD_STEM: &str = "xdotool-captured-splice-1791744122";
    const LIVE_FORWARD_LOG: &str =
        include_str!("fixtures/captured_splice_crdt_forward_2026_10_11.ops.log");
    /// The retained advance's receipt line in the fixture.
    const LIVE_ADVANCE_MARK: &str = "crdt_response_cell_add ";
    /// The first post-advance operator capture and its applied delta.
    const LIVE_EDIT_TWO_PROOF: &str = "operator_ops=28 non_operator_ops=0 shadow_replay=agreed \
                                       merge_base=d232ee81c5ad";

    fn verify_live_forward(log: &str) -> Result<CapturedSpliceVerification> {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_named_log(dir.path(), LIVE_FORWARD_STEM, log);
        verify(&doc)
    }

    /// Apply `edit` to every fixture line for which `select` holds; assert it
    /// changed at least one, so a mutation that no longer matches the fixture
    /// cannot pass as a surviving one.
    fn mutate_lines(
        select: impl Fn(&str) -> bool,
        edit: impl Fn(&str) -> Option<String>,
    ) -> String {
        let mut changed = 0;
        let mut out = String::new();
        for line in LIVE_FORWARD_LOG.lines() {
            if select(line) {
                changed += 1;
                if let Some(replacement) = edit(line) {
                    out.push_str(&replacement);
                    out.push('\n');
                }
            } else {
                out.push_str(line);
                out.push('\n');
            }
        }
        assert!(changed > 0, "mutation matched no fixture line");
        out
    }

    fn after_live_advance(line: &str) -> bool {
        let advance = LIVE_FORWARD_LOG.find(LIVE_ADVANCE_MARK).unwrap();
        LIVE_FORWARD_LOG.find(line).unwrap() > advance
    }

    fn live_forward_err(log: &str) -> String {
        format!("{:#}", verify_live_forward(log).unwrap_err())
    }

    #[test]
    fn live_2026_10_11_crdt_forward_recovery_passes() {
        assert!(
            !LIVE_FORWARD_LOG.contains(SPLICE_RECOVERY_SOURCE),
            "the fixture must not carry the fallback receipt, or it proves the old shape"
        );
        assert!(
            LIVE_FORWARD_LOG.contains(LIVE_EDIT_TWO_PROOF),
            "the fixture must carry edit two's capture on the pre-advance base"
        );
        let report = verify_live_forward(LIVE_FORWARD_LOG).unwrap();
        let RecoveryShape::CrdtForward(chain) = &report.shape else {
            panic!("expected the crdt_forward shape: {:?}", report.shape);
        };
        assert!(!chain.advance.delivery_converged);
        assert!(chain.advance.content_hash.starts_with("cb9801cfca17"));
        assert_eq!(chain.proof.operator_ops, 28);
        assert_eq!(chain.proof.merge_base, "d232ee81c5ad");
        assert_eq!(chain.ingest.outcome, "Applied { changed: true }");
        assert!(chain.converged.converged);
        assert!(chain.advance.index < chain.proof.index);
        assert!(chain.proof.index < chain.ingest.index);
        assert!(chain.ingest.index < chain.converged.index);
        assert_eq!(report.splice_reads_total, 0);
    }

    /// Clause (a): an advance that converged immediately never retained, so
    /// the same receipts must not pass.
    #[test]
    fn crdt_forward_rejects_an_advance_that_never_retained() {
        let log = mutate_lines(
            |line| line.contains(LIVE_ADVANCE_MARK),
            |line| Some(line.replace("delivery_converged=false", "delivery_converged=true")),
        );
        let err = live_forward_err(&log);
        assert!(err.contains("never RETAINED editor delivery"), "{err}");
        assert!(
            err.contains("no `controller_crdt_current_text` receipts"),
            "{err}"
        );
    }

    /// Clause (a): no response advance at all.
    #[test]
    fn crdt_forward_rejects_a_run_without_an_advance() {
        let log = mutate_lines(|line| line.contains(LIVE_ADVANCE_MARK), |_| None);
        let err = live_forward_err(&log);
        assert!(
            err.contains("no `crdt_response_cell_add applied=true`"),
            "{err}"
        );
    }

    /// Clause (a) ordering: captures that all precede the retained advance
    /// recovered across nothing.
    #[test]
    fn crdt_forward_rejects_captures_that_precede_the_advance() {
        let advance_line = LIVE_FORWARD_LOG
            .lines()
            .find(|line| line.contains(LIVE_ADVANCE_MARK))
            .unwrap();
        let mut log: String = LIVE_FORWARD_LOG
            .lines()
            .filter(|line| *line != advance_line)
            .map(|line| format!("{line}\n"))
            .collect();
        log.push_str(advance_line);
        log.push('\n');
        let err = live_forward_err(&log);
        assert!(
            err.contains("no `editor_op_capture_proof` after the retained advance"),
            "{err}"
        );
    }

    /// Clause (b): no operator capture after the advance.
    #[test]
    fn crdt_forward_rejects_a_run_without_an_operator_capture_after_the_advance() {
        let log = mutate_lines(
            |line| OpsLogEvent::EditorOpCaptureProof.is_line(line) && after_live_advance(line),
            |line| {
                let ops = field(line, "operator_ops").unwrap();
                Some(line.replace(
                    &format!("operator_ops={ops} non_operator_ops=0"),
                    &format!("operator_ops=0 non_operator_ops={ops}"),
                ))
            },
        );
        let err = live_forward_err(&log);
        assert!(err.contains("none carry `operator_ops>=1`"), "{err}");
        assert!(
            err.contains("all_ops_non_operator"),
            "the refusals must be named: {err}"
        );
    }

    /// Clause (b): the captures after the advance were refused, not proven.
    #[test]
    fn crdt_forward_rejects_a_refused_capture() {
        let log = mutate_lines(
            |line| OpsLogEvent::EditorOpCaptureProof.is_line(line) && after_live_advance(line),
            |line| {
                let (stamp, _) = line.split_once(' ').unwrap();
                Some(format!(
                    "{stamp} editor_op_capture_refused reason=base_hash_unavailable detail=ops=1 \
                     #opcapturedormant doc={LIVE_FORWARD_STEM}"
                ))
            },
        );
        let err = live_forward_err(&log);
        assert!(err.contains("were REFUSED"), "{err}");
        assert!(err.contains("base_hash_unavailable"), "{err}");
    }

    /// Clause (b): a disagreeing shadow replay discarded the burst.
    #[test]
    fn crdt_forward_rejects_a_disagreeing_shadow_replay() {
        let log = mutate_lines(
            |line| OpsLogEvent::EditorOpCaptureProof.is_line(line) && after_live_advance(line),
            |line| Some(line.replace("shadow_replay=agreed", "shadow_replay=disagreed")),
        );
        let err = live_forward_err(&log);
        assert!(err.contains("DISCARDS the burst"), "{err}");
    }

    /// Clause (b): a capture stamped on the POST-advance canonical never had to
    /// recover across the advance.
    #[test]
    fn crdt_forward_rejects_a_capture_on_the_post_advance_base() {
        let log = mutate_lines(
            |line| line.contains("merge_base=d232ee81c5ad"),
            |line| Some(line.replace("merge_base=d232ee81c5ad", "merge_base=cb9801cfca17")),
        );
        let err = live_forward_err(&log);
        assert!(err.contains("the POST-advance canonical"), "{err}");
    }

    /// Clause (b): an unknown merge base proves nothing about which canonical
    /// the edit was stamped on.
    #[test]
    fn crdt_forward_rejects_an_unavailable_merge_base() {
        let log = mutate_lines(
            |line| line.contains("merge_base=d232ee81c5ad"),
            |line| Some(line.replace("merge_base=d232ee81c5ad", "merge_base=unavailable")),
        );
        let err = live_forward_err(&log);
        assert!(err.contains("`merge_base=unavailable`"), "{err}");
    }

    /// Clause (c): the captured delta never merged into canonical.
    #[test]
    fn crdt_forward_rejects_a_delta_that_never_applied() {
        let log = mutate_lines(
            |line| {
                OpsLogEvent::CrdtDocumentOpDeltaIngested.is_line(line) && after_live_advance(line)
            },
            |line| Some(line.replace("Applied { changed: true }", "StaleLineage")),
        );
        let err = live_forward_err(&log);
        assert!(err.contains("the captured delta never merged"), "{err}");
        assert!(
            err.contains("StaleLineage"),
            "the observed outcome must be named: {err}"
        );
    }

    /// Clause (c): `Applied { changed: false }` merged nothing.
    #[test]
    fn crdt_forward_rejects_an_unchanged_delta() {
        let log = mutate_lines(
            |line| {
                OpsLogEvent::CrdtDocumentOpDeltaIngested.is_line(line) && after_live_advance(line)
            },
            |line| Some(line.replace("Applied { changed: true }", "Applied { changed: false }")),
        );
        let err = live_forward_err(&log);
        assert!(err.contains("the captured delta never merged"), "{err}");
    }

    /// Clause (c): delivery never converged after the merge.
    #[test]
    fn crdt_forward_rejects_delivery_that_never_converged() {
        let log = mutate_lines(
            |line| OpsLogEvent::CrdtCommitBarrier.is_line(line),
            |line| {
                Some(line.replace(
                    "visible_delivery_projected=true",
                    "visible_delivery_projected=false",
                ))
            },
        );
        let err = live_forward_err(&log);
        assert!(err.contains("delivery never converged"), "{err}");
    }

    /// The fallback-only diagnoses still surface when neither shape holds, so
    /// a run with splice reads keeps its precise fallback missing link.
    #[test]
    fn failure_reports_both_shapes_missing_links() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &format!(
                "{}{}",
                splice_read("current", "aaaa1111", 42885),
                capture_proof(2, 0, "agreed"),
            ),
        );
        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(
            err.contains("- fallback (captured-local-splice-recovery):"),
            "{err}"
        );
        assert!(err.contains("never advanced between two"), "{err}");
        assert!(err.contains("- crdt_forward"), "{err}");
        assert!(
            err.contains("no `crdt_response_cell_add applied=true`"),
            "{err}"
        );
    }

    /// A document whose name extends this one's must not lend it receipts.
    #[test]
    fn receipts_for_a_prefix_named_document_do_not_count() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = project_with_log(
            dir.path(),
            &capture_proof(2, 0, "agreed").replace("doc=scratch", "doc=scratch-2"),
        );
        let err = format!("{:#}", verify(&doc).unwrap_err());
        assert!(err.contains("no receipts for doc=scratch"), "{err}");
    }

    #[test]
    fn delta_outcome_reads_the_spaced_debug_rendering() {
        let line = "crdt_document_op_delta_ingested file=/x authority=multi_replica \
                    outcome=Applied { changed: true } frame_lineage=abc delta_len=1";
        assert_eq!(delta_outcome(line), "Applied { changed: true }");
        assert_eq!(
            delta_outcome("crdt_document_op_delta_ingested file=/x"),
            "unknown"
        );
    }

    /// `field` must be exact on the key, or `non_operator_ops=` would satisfy a
    /// query for `operator_ops=` and every non-operator proof would read as a
    /// passing one.
    #[test]
    fn field_lookup_is_exact_on_the_key() {
        let line = "editor_op_capture_proof operator_ops=0 non_operator_ops=7 text_hash=ab";
        assert_eq!(field(line, "operator_ops").as_deref(), Some("0"));
        assert_eq!(field(line, "non_operator_ops").as_deref(), Some("7"));
        assert_eq!(field(line, "text_hash").as_deref(), Some("ab"));
        assert_eq!(field(line, "hash"), None);
    }
}
