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
//! ## The proving triple
//!
//! A pass requires three receipts for one document, in timestamp order:
//!
//! 1. a `controller_crdt_current_text source=captured-local-splice-recovery
//!    status=current` read — the splice recovery observed canonical text;
//! 2. an `editor_op_capture_proof` with `operator_ops >= 1` and
//!    `shadow_replay=agreed` — an operator-authored edit was captured and its
//!    shadow replay agreed, so the ops were kept rather than discarded;
//! 3. a later splice-recovery `status=current` read whose `text_hash` **differs**
//!    from (1) — the canonical response advanced independently across it.
//!
//! Every failure names which link is missing, because "no proof" and "no
//! evidence class at all" are different diagnoses and conflating them is what
//! kept this gate open on a guess.

use agent_doc_turn::op_log::OpsLogEvent;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// The `source=` tag the splice-recovery read site stamps on its
/// `controller_crdt_current_text` receipt.
const SPLICE_RECOVERY_SOURCE: &str = "source=captured-local-splice-recovery";

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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CapturedSpliceVerification {
    doc_tag: String,
    ops_log: PathBuf,
    splice_reads_total: usize,
    splice_current_reads: usize,
    distinct_text_hashes: usize,
    capture_proofs: usize,
    operator_proofs: usize,
    before: SpliceRead,
    proof: CaptureProof,
    after: SpliceRead,
}

pub fn run(file: &Path) -> Result<()> {
    let report = verify(file)?;
    println!(
        "captured-splice-recovery verification ok for {}",
        file.display()
    );
    println!("ops_log={}", report.ops_log.display());
    println!(
        "doc={} splice_reads={} splice_current_reads={} distinct_text_hashes={} capture_proofs={} operator_proofs={}",
        report.doc_tag,
        report.splice_reads_total,
        report.splice_current_reads,
        report.distinct_text_hashes,
        report.capture_proofs,
        report.operator_proofs,
    );
    println!(
        "proof=operator_edit_recovered_across_advanced_canonical before_hash={} before_len={} \
         epoch_generation={} operator_ops={} non_operator_ops={} shadow_replay={} \
         after_hash={} after_len={}",
        report.before.text_hash,
        report.before.text_len,
        report.proof.epoch_generation,
        report.proof.operator_ops,
        report.proof.non_operator_ops,
        report.proof.shadow_replay,
        report.after.text_hash,
        report.after.text_len,
    );
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
    let doc_lines: Vec<&str> = log.lines().filter(|line| line.contains(&doc_tag)).collect();
    if doc_lines.is_empty() {
        bail!(
            "ops.log has no receipts for {doc_tag} in {}; open the document in a live editor and \
             run a response cycle before verifying",
            ops_log.display()
        );
    }

    let mut splice_reads: Vec<SpliceRead> = Vec::new();
    let mut proofs: Vec<CaptureProof> = Vec::new();
    for (index, line) in doc_lines.iter().enumerate() {
        if OpsLogEvent::ControllerCrdtCurrentText.is_line(line)
            && line.contains(SPLICE_RECOVERY_SOURCE)
        {
            splice_reads.push(SpliceRead {
                index,
                status: field(line, "status").unwrap_or_else(|| "unknown".to_string()),
                text_hash: field(line, "text_hash").unwrap_or_default(),
                text_len: field(line, "text_len")
                    .and_then(|value| value.parse().ok())
                    .unwrap_or(0),
            });
        } else if OpsLogEvent::EditorOpCaptureProof.is_line(line) {
            proofs.push(CaptureProof {
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
            });
        }
    }

    if splice_reads.is_empty() {
        bail!(
            "no `{}` receipts with `{SPLICE_RECOVERY_SOURCE}` for {doc_tag} in {} — the \
             captured-local-splice-recovery path never ran, so there is nothing for an operator \
             edit to have recovered across",
            OpsLogEvent::ControllerCrdtCurrentText,
            ops_log.display(),
        );
    }

    let current_reads: Vec<&SpliceRead> = splice_reads
        .iter()
        .filter(|read| read.status == "current" && !read.text_hash.is_empty())
        .collect();
    if current_reads.is_empty() {
        bail!(
            "{} captured-local-splice-recovery receipt(s) for {doc_tag}, but none resolved \
             `status=current` (observed: {}) — the recovery never saw canonical text, so the \
             editor replica, not op capture, is the blocker; see the missing-replica recovery \
             path in {}",
            splice_reads.len(),
            describe_statuses(&splice_reads),
            ops_log.display(),
        );
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
        bail!(
            "{} captured-local-splice-recovery `status=current` read(s) for {doc_tag} all observed \
             the SAME canonical text (hash={}) — the canonical response never advanced between two \
             recoveries, so this run cannot prove recovery ACROSS an independent advance. Drive an \
             agent-doc response cycle between two operator edits and re-verify.",
            current_reads.len(),
            current_reads[0].text_hash,
        );
    }

    let operator_proofs: Vec<&CaptureProof> = proofs
        .iter()
        .filter(|proof| proof.operator_ops >= 1)
        .collect();
    if operator_proofs.is_empty() {
        bail!(
            "no `{}` receipt with `operator_ops>=1` for {doc_tag} in {} — {} capture proof(s) \
             present, all classifying every op as non-operator{}. The canonical text DID advance \
             across {} splice recoveries, so the missing link is a captured operator edit, not the \
             recovery path. Note: `editor_op_epoch_closed cause=` cannot answer this — \
             `non_operator_projection` is its only possible value.",
            OpsLogEvent::EditorOpCaptureProof,
            ops_log.display(),
            proofs.len(),
            describe_non_operator_proofs(&proofs),
            current_reads.len(),
        );
    }

    let agreed: Vec<&&CaptureProof> = operator_proofs
        .iter()
        .filter(|proof| proof.shadow_replay == "agreed")
        .collect();
    if agreed.is_empty() {
        bail!(
            "{} `{}` receipt(s) for {doc_tag} carry `operator_ops>=1`, but none report \
             `shadow_replay=agreed` (observed: {}) — a disagreeing shadow replay DISCARDS the \
             captured burst, so those operator edits never reached the merge and cannot have \
             recovered",
            operator_proofs.len(),
            OpsLogEvent::EditorOpCaptureProof,
            describe_replays(&operator_proofs),
        );
    }

    // The property is ordered: an operator capture must sit BETWEEN two
    // splice-recovery reads that observed different canonical text. A proof
    // before every read, or after every read, proves nothing about recovery
    // across an advance.
    let Some((before, proof, after)) = find_proving_triple(&current_reads, &agreed) else {
        bail!(
            "for {doc_tag} the receipts exist but never line up: {} splice-recovery \
             `status=current` read(s) across {distinct_text_hashes} distinct canonical text(s) and \
             {} agreed operator capture proof(s), but no proof falls BETWEEN two reads that \
             observed different text. Recovery across an independent advance is an ordering \
             property; an operator edit captured entirely before or after every advance does not \
             establish it.",
            current_reads.len(),
            agreed.len(),
        );
    };

    Ok(CapturedSpliceVerification {
        doc_tag,
        ops_log,
        splice_reads_total: splice_reads.len(),
        splice_current_reads: current_reads.len(),
        distinct_text_hashes,
        capture_proofs: proofs.len(),
        operator_proofs: operator_proofs.len(),
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
        std::fs::create_dir_all(dir.join(".agent-doc/logs")).unwrap();
        std::fs::write(dir.join(".agent-doc/logs/ops.log"), lines).unwrap();
        let doc = dir.join(format!("{DOC}.md"));
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
        assert_eq!(report.before.text_hash, "aaaa1111");
        assert_eq!(report.after.text_hash, "bbbb2222");
        assert_eq!(report.proof.operator_ops, 2);
        assert_eq!(report.distinct_text_hashes, 2);
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
