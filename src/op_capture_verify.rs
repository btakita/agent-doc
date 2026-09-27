use agent_doc_turn::op_log::OpsLogEvent;
use anyhow::{Context, Result, bail};
use std::path::{Path, PathBuf};

/// Batch producer success marker written by `agent_doc_record_editor_ops_json`
/// (`#opcaptureverifybatchproducer`). Distinct from the singular
/// `editor_op_recorded`, and the only producer the JetBrains plugin drives.
const EDITOR_OPS_RECORDED_MARKER: &str = "editor_ops_recorded";
/// Batch producer failure marker written by the same FFI.
const EDITOR_OPS_RECORD_FAILED_MARKER: &str = "editor_ops_record_failed";

#[derive(Debug, Clone, PartialEq, Eq)]
struct OpCaptureVerification {
    doc_tag: String,
    ops_log: PathBuf,
    recorded_count: usize,
    batch_recorded_count: usize,
    accepted_count: usize,
    failed_count: usize,
    refused_count: usize,
    proof_count: usize,
    cafe_demo: bool,
}

pub fn run(file: &Path, expect_cafe_demo: bool) -> Result<()> {
    let report = verify(file, expect_cafe_demo)?;
    println!("op-capture verification ok for {}", file.display());
    println!("ops_log={}", report.ops_log.display());
    println!(
        "doc={} editor_op_recorded={} editor_ops_recorded={} editor_ops_for_base_accepted={} failures={} capture_refusals={} capture_proofs={}",
        report.doc_tag,
        report.recorded_count,
        report.batch_recorded_count,
        report.accepted_count,
        report.failed_count,
        report.refused_count,
        report.proof_count
    );
    if expect_cafe_demo {
        println!("cafe_demo=ok offset=6 delete_len=6 insert_non_ascii=true");
    }
    Ok(())
}

fn verify(file: &Path, expect_cafe_demo: bool) -> Result<OpCaptureVerification> {
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
            "ops.log has no op-capture lines for {doc_tag}; run the live editor edit and merge before verifying"
        );
    }

    let recorded: Vec<&str> = doc_lines
        .iter()
        .copied()
        .filter(|line| {
            OpsLogEvent::EditorOpRecorded.is_line(line) && line.contains("#qnodemerge4wire")
        })
        .collect();
    // `#opcaptureverifybatchproducer`: the shipped JetBrains plugin records through
    // `agent_doc_record_editor_ops_json` (TypingTracker), whose success marker is
    // `editor_ops_recorded ... transaction=batch`, NOT the singular
    // `editor_op_recorded` written by the one-op FFI. Requiring only the singular
    // marker made this verifier a permanent false NEGATIVE on every real JetBrains
    // session, however much operator typing had actually been captured. Either
    // producer proves capture.
    let batch_recorded: Vec<&str> = doc_lines
        .iter()
        .copied()
        .filter(|line| {
            line.contains(EDITOR_OPS_RECORDED_MARKER)
                && line.contains("transaction=batch")
                && line.contains("#qbasehashmemo")
        })
        .collect();
    // `#opcapturedormant`: a dormant ledger writes neither producer marker nor a
    // failure marker, so the reporter's own refusal receipts are the only evidence
    // that separates "the editor refused to hand the burst over" from "the operator
    // never typed". Name them in the failure instead of reporting a bare absence.
    let refused: Vec<&str> = doc_lines
        .iter()
        .copied()
        .filter(|line| OpsLogEvent::EditorOpCaptureRefused.is_line(line))
        .collect();
    let refused_count = refused.len();
    // `#opcaptureliveread`: the positive counterpart. A reporter that reached the
    // record FFI states the four facts this verification depends on — live
    // op-capture epoch generation, the operator/non-operator classification split,
    // shadow-replay agreement, and merge-base availability — so a producer marker
    // that IS present no longer rests on the absence of a refusal as its evidence.
    let proofs: Vec<&str> = doc_lines
        .iter()
        .copied()
        .filter(|line| OpsLogEvent::EditorOpCaptureProof.is_line(line))
        .collect();
    let proof_count = proofs.len();
    if recorded.is_empty() && batch_recorded.is_empty() {
        bail!(
            "missing editor-op producer marker for {doc_tag} in {} — expected either \
             `{}` (one-op FFI, #qnodemerge4wire) or `{EDITOR_OPS_RECORDED_MARKER} ... transaction=batch` \
             (batch FFI, #qbasehashmemo, the path the JetBrains TypingTracker uses).{}{}",
            ops_log.display(),
            OpsLogEvent::EditorOpRecorded,
            describe_capture_refusals(&refused),
            describe_capture_proofs(&proofs),
        );
    }

    let accepted: Vec<&str> = doc_lines
        .iter()
        .copied()
        .filter(|line| {
            OpsLogEvent::EditorOpsForBase.is_line(line)
                && line.contains("accepted=true")
                && line.contains("#qnodemerge4wire")
        })
        .collect();
    if accepted.is_empty() {
        bail!(
            "missing editor_ops_for_base accepted=true marker for {doc_tag} in {}",
            ops_log.display()
        );
    }

    let failed_count = doc_lines
        .iter()
        .filter(|line| {
            OpsLogEvent::EditorOpRecordFailed.is_line(line)
                || line.contains(EDITOR_OPS_RECORD_FAILED_MARKER)
        })
        .count();
    if failed_count > 0 {
        bail!(
            "found {failed_count} editor-op record failure marker(s) for {doc_tag} \
             (`{}` or `{EDITOR_OPS_RECORD_FAILED_MARKER}`)",
            OpsLogEvent::EditorOpRecordFailed,
        );
    }

    if expect_cafe_demo {
        verify_cafe_demo(&recorded, &batch_recorded, &accepted, &ops_log, &doc_tag)?;
    }

    Ok(OpCaptureVerification {
        doc_tag,
        ops_log,
        recorded_count: recorded.len(),
        batch_recorded_count: batch_recorded.len(),
        accepted_count: accepted.len(),
        failed_count,
        refused_count,
        proof_count,
        cafe_demo: expect_cafe_demo,
    })
}

/// Summarize `editor_op_capture_refused` receipts for a producer-marker failure.
///
/// With no receipts at all the reporter chain itself is unobserved — either the
/// plugin predates the refusal receipt or its `documentChanged` listener never
/// ran — which is a materially different diagnosis from a named refusal.
fn describe_capture_refusals(refused: &[&str]) -> String {
    if refused.is_empty() {
        return format!(
            " No `{}` receipts either: the reporter chain is unobserved, so either no operator \
             document change reached the editor listener or the plugin predates the refusal receipt.",
            OpsLogEvent::EditorOpCaptureRefused,
        );
    }
    let mut reasons: Vec<&str> = refused
        .iter()
        .filter_map(|line| {
            line.split_whitespace()
                .find_map(|field| field.strip_prefix("reason="))
        })
        .collect();
    reasons.sort_unstable();
    reasons.dedup();
    format!(
        " {} `{}` receipt(s) name why the burst was never handed over: {}.",
        refused.len(),
        OpsLogEvent::EditorOpCaptureRefused,
        reasons.join(", "),
    )
}

/// Summarize `editor_op_capture_proof` receipts for a producer-marker failure.
///
/// `#opcaptureliveread`: a proof receipt beside a missing producer marker is the
/// sharpest shape available — the reporter got as far as stating the four facts and
/// the record FFI still wrote nothing, which is neither a dormant chain nor a
/// refusal. Name the last proof's fields so the failing fact is read, not guessed.
fn describe_capture_proofs(proofs: &[&str]) -> String {
    let Some(last) = proofs.last() else {
        return String::new();
    };
    let fields: Vec<&str> = last
        .split_whitespace()
        .filter(|field| {
            field.starts_with("epoch_generation=")
                || field.starts_with("operator_ops=")
                || field.starts_with("non_operator_ops=")
                || field.starts_with("shadow_replay=")
                || field.starts_with("merge_base=")
        })
        .collect();
    format!(
        " {} `{}` receipt(s) show the reporter reached the record FFI; the latest states {}.",
        proofs.len(),
        OpsLogEvent::EditorOpCaptureProof,
        fields.join(" "),
    )
}

fn verify_cafe_demo(
    recorded: &[&str],
    batch_recorded: &[&str],
    accepted: &[&str],
    ops_log: &Path,
    doc_tag: &str,
) -> Result<()> {
    // Canonical byte contract for replacing "日本" in "café 日本 😀":
    // "café " is 6 UTF-8 bytes, "日本" is 6 bytes, and the emoji is 4 bytes.
    let sample = "café 日本 😀";
    let prefix_bytes = "café ".len();
    let delete_bytes = "日本".len();
    let emoji_bytes = "😀".len();
    if !(prefix_bytes == 6 && delete_bytes == 6 && emoji_bytes == 4 && sample.len() == 17) {
        bail!("internal non-ASCII byte contract check failed");
    }

    // `#opcaptureliveread`: read the byte contract off EITHER producer. The one-op
    // FFI renders it per op (`kind=delete offset=6 delete_len=6`); the batch FFI the
    // JetBrains TypingTracker drives renders the whole burst through the shared
    // summary (`offsets=6,6 delete_bytes=6 insert_non_ascii=true`). Demanding only
    // the per-op form made cafe-demo mode unsatisfiable on every real JetBrains
    // session — the same false negative `#opcaptureverifybatchproducer` removed from
    // plain mode, which survived here because cafe-demo had its own reader.
    let recorded_delete = recorded.iter().any(|line| {
        line.contains("kind=delete") && line.contains("offset=6") && line.contains("delete_len=6")
    }) || batch_recorded
        .iter()
        .any(|line| line.contains("offsets=6,6") && line.contains("delete_bytes=6"));
    let recorded_non_ascii_insert = recorded.iter().any(|line| {
        line.contains("kind=insert")
            && line.contains("offset=6")
            && line.contains("insert_non_ascii=true")
    }) || batch_recorded
        .iter()
        .any(|line| line.contains("offsets=6,6") && line.contains("insert_non_ascii=true"));
    let accepted_delete = accepted
        .iter()
        .any(|line| line.contains("offsets=") && line.contains("delete_bytes=6"));
    let accepted_non_ascii_insert = accepted
        .iter()
        .any(|line| line.contains("insert_non_ascii=true"));

    if !(recorded_delete
        && recorded_non_ascii_insert
        && accepted_delete
        && accepted_non_ascii_insert)
    {
        bail!(
            "missing cafe-demo byte evidence for {doc_tag} in {}; expected a producer \
             delete at byte offset 6 of 6 bytes (one-op `offset=6 delete_len=6`, or batch \
             `offsets=6,6 delete_bytes=6`) plus an accepted non-ASCII insert",
            ops_log.display()
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup_log(log: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        std::fs::write(dir.path().join(".agent-doc/logs/ops.log"), log).unwrap();
        (dir, doc)
    }

    #[test]
    fn verify_accepts_recorded_and_accepted_markers() {
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_op_recorded kind=insert offset=3 insert_bytes=1 insert_non_ascii=false base=abc #qnodemerge4wire doc=plan\n\
             [2026-06-24T00:00:01Z] editor_ops_for_base accepted=true ops=1 base=abc offsets=3 delete_bytes=0 insert_bytes=1 insert_non_ascii=false #qnodemerge4wire doc=plan\n",
        );

        let report = verify(&doc, false).unwrap();
        assert_eq!(report.recorded_count, 1);
        assert_eq!(report.accepted_count, 1);
        assert!(!report.cafe_demo);
    }

    /// `#opcaptureverifybatchproducer`: the JetBrains plugin records operator ops
    /// ONLY through `agent_doc_record_editor_ops_json` (TypingTracker), which writes
    /// `editor_ops_recorded ... transaction=batch #qbasehashmemo`. Requiring the
    /// singular `editor_op_recorded` made this verifier fail on every real
    /// JetBrains session regardless of how much was actually captured.
    #[test]
    fn verify_accepts_the_batch_producer_the_jetbrains_plugin_actually_uses() {
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_ops_recorded count=2 base=abc transaction=batch #qbasehashmemo doc=plan\n\
             [2026-06-24T00:00:01Z] editor_ops_for_base accepted=true ops=2 base=abc offsets=3 delete_bytes=0 insert_bytes=2 insert_non_ascii=false #qnodemerge4wire doc=plan\n",
        );

        let report = verify(&doc, false).unwrap();
        assert_eq!(
            report.recorded_count, 0,
            "the singular one-op marker is genuinely absent on this path"
        );
        assert_eq!(
            report.batch_recorded_count, 1,
            "the batch producer must be counted as capture evidence"
        );
        assert_eq!(report.accepted_count, 1);
    }

    #[test]
    fn verify_rejects_a_batch_record_failure_marker() {
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_ops_recorded count=1 base=abc transaction=batch #qbasehashmemo doc=plan\n\
             [2026-06-24T00:00:01Z] editor_ops_for_base accepted=true ops=1 base=abc offsets=3 delete_bytes=0 insert_bytes=1 insert_non_ascii=false #qnodemerge4wire doc=plan\n\
             [2026-06-24T00:00:02Z] editor_ops_record_failed count=1 transaction=batch error=disk #qbasehashmemo doc=plan\n",
        );

        let err = verify(&doc, false).unwrap_err().to_string();
        assert!(
            err.contains("record failure marker") && err.contains("editor_ops_record_failed"),
            "a batch producer failure must not be silently ignored: {err}"
        );
    }

    #[test]
    fn verify_names_both_producers_when_neither_is_present() {
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_op_epoch_closed cause=non_operator_projection action=cleared doc=plan\n",
        );

        let err = verify(&doc, false).unwrap_err().to_string();
        assert!(
            err.contains("editor_op_recorded") && err.contains("transaction=batch"),
            "the diagnostic must name both producers so a dormant ledger is not \
             misread as the wrong-marker bug: {err}"
        );
    }

    #[test]
    fn verify_names_the_refusal_reasons_behind_a_dormant_ledger() {
        // `#opcapturedormant`: the reporter refused the burst, so the absence has a
        // cause the operator can act on. Naming it is the whole point of the receipt.
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_op_capture_refused reason=all_ops_non_operator detail=ops=3 #opcapturedormant doc=plan\n\
             [2026-06-24T00:00:01Z] editor_op_capture_refused reason=shadow_replay_mismatch detail=ops=2_operator_ops=2 #opcapturedormant doc=plan\n",
        );

        let err = verify(&doc, false).unwrap_err().to_string();
        assert!(
            err.contains("2 `editor_op_capture_refused` receipt(s)")
                && err.contains("all_ops_non_operator")
                && err.contains("shadow_replay_mismatch"),
            "a dormant ledger with refusal receipts must report their reasons: {err}"
        );
    }

    #[test]
    fn verify_says_the_reporter_chain_is_unobserved_when_no_refusal_receipt_exists() {
        // Zero producers AND zero refusals is a different diagnosis from a named
        // refusal: nothing in the reporter chain ran, or the plugin is too old.
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_op_epoch_closed cause=non_operator_projection action=cleared doc=plan\n",
        );

        let err = verify(&doc, false).unwrap_err().to_string();
        assert!(
            err.contains("reporter chain is unobserved"),
            "a bare absence must be distinguished from a named refusal: {err}"
        );
    }

    #[test]
    fn verify_counts_refusals_alongside_a_successful_capture() {
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_op_capture_refused reason=doc_advanced_during_drain detail=ops=1_requeued=true #opcapturedormant doc=plan\n\
             [2026-06-24T00:00:01Z] editor_ops_recorded count=1 base=abc transaction=batch #qbasehashmemo doc=plan\n\
             [2026-06-24T00:00:02Z] editor_ops_for_base accepted=true ops=1 base=abc offsets=3 delete_bytes=0 insert_bytes=1 insert_non_ascii=false #qnodemerge4wire doc=plan\n",
        );

        let report = verify(&doc, false).unwrap();
        assert_eq!(report.batch_recorded_count, 1);
        assert_eq!(
            report.refused_count, 1,
            "a requeued burst is a refusal receipt, not a capture failure"
        );
    }

    #[test]
    fn verify_rejects_missing_acceptance_marker() {
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_op_recorded kind=insert offset=3 insert_bytes=1 insert_non_ascii=false base=abc #qnodemerge4wire doc=plan\n",
        );

        let err = verify(&doc, false).unwrap_err().to_string();
        assert!(
            err.contains("missing editor_ops_for_base accepted=true"),
            "unexpected error: {err}"
        );
    }

    /// `#opcaptureliveread`: drive the real producers and let the verifier read the
    /// ops.log they wrote, so the `editor_ops_recorded ... transaction=batch`
    /// contract stops depending on a human typing into IntelliJ.
    ///
    /// Every other test in this module hands the verifier ops-log text written by
    /// the test, which proves the PARSER and nothing about the producers. A parser
    /// test cannot catch a producer whose marker text drifts — exactly the
    /// `#opcaptureverifybatchproducer` defect, where the verifier demanded a marker
    /// the shipped plugin never wrote and no test noticed. This one calls the batch
    /// FFI the JetBrains `TypingTracker` calls, the proof FFI beside it, and the
    /// merge consumer, then verifies the log none of them were told about.
    #[test]
    fn a_synthetic_burst_through_the_real_producers_satisfies_verification() {
        use std::ffi::CString;

        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        let doc = root.join("plan.md");

        // The canonical non-ASCII byte contract: "café " is 6 UTF-8 bytes and
        // "日本" is 6, so a synthetic burst can assert byte offsets the UTF-16
        // editor side would get wrong.
        let base_text = "café 日本 😀\n";
        std::fs::write(&doc, base_text).unwrap();
        let base_hash = agent_doc_hash::content_hash(base_text);

        let file_c = CString::new(doc.to_str().unwrap()).unwrap();
        let base_c = CString::new(base_hash.as_str()).unwrap();
        let ops_c = CString::new(
            r#"[{"kind":"delete","offset":6,"len":6},{"kind":"insert","offset":6,"text":"世界"}]"#,
        )
        .unwrap();

        // 1. The reporter states the four facts it resolved for this burst.
        let rc = unsafe {
            agent_doc::ffi::agent_doc_log_editor_op_capture_proof(
                file_c.as_ptr(),
                7,
                2,
                0,
                1,
                base_c.as_ptr(),
            )
        };
        assert_eq!(rc, 1, "the proof receipt must be written");

        // 2. The batch producer the JetBrains TypingTracker actually drives.
        let rc = unsafe {
            agent_doc::ffi::agent_doc_record_editor_ops_json(
                file_c.as_ptr(),
                base_c.as_ptr(),
                ops_c.as_ptr(),
            )
        };
        assert_eq!(rc, 1, "the synthetic burst must record");

        // 3. The merge consumer, which writes the acceptance receipt.
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base_text).encode_state();
        let (merged, _state) = agent_doc_merge_io::merge_contents_crdt_with_ops(
            &doc,
            Some(&base_state),
            "café 日本 😀\n\nAgent response.\n",
            "café 世界 😀\n",
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        assert!(
            merged.contains("世界") && merged.contains("Agent response."),
            "the synthetic burst must survive the merge it proves:\n{merged}"
        );

        // 4. The verifier reads only what the producers above wrote.
        let ops_log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("transaction=batch"),
            "the batch producer marker must come from the FFI, not from the test:\n{ops_log}"
        );
        let report = verify(&doc, true).unwrap();
        assert_eq!(
            report.batch_recorded_count, 1,
            "the batch producer receipt must satisfy verification:\n{ops_log}"
        );
        assert_eq!(report.accepted_count, 1, "merge acceptance missing:\n{ops_log}");
        assert_eq!(report.failed_count, 0);
        assert_eq!(report.refused_count, 0, "a clean burst refuses nothing:\n{ops_log}");
        assert_eq!(
            report.proof_count, 1,
            "the reporter proof must be counted:\n{ops_log}"
        );
        assert!(report.cafe_demo, "the non-ASCII byte contract must hold");
    }

    /// The proof receipt names all four facts on one parseable line, and an absent
    /// generation or merge base reads as `unknown` / `unavailable` rather than as a
    /// dangling field or a plausible-looking `0`.
    #[test]
    fn the_capture_proof_receipt_names_every_fact_the_verification_depends_on() {
        use std::ffi::CString;

        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();
        let file_c = CString::new(doc.to_str().unwrap()).unwrap();

        let base_c = CString::new("abcdef0123456789").unwrap();
        let rc = unsafe {
            agent_doc::ffi::agent_doc_log_editor_op_capture_proof(
                file_c.as_ptr(),
                12,
                3,
                1,
                1,
                base_c.as_ptr(),
            )
        };
        assert_eq!(rc, 1);

        // No registration, no base, replay disagreed: the three "not available"
        // shapes must be distinguishable from real values.
        let rc = unsafe {
            agent_doc::ffi::agent_doc_log_editor_op_capture_proof(
                file_c.as_ptr(),
                -1,
                0,
                4,
                0,
                std::ptr::null(),
            )
        };
        assert_eq!(rc, 1);

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        let receipts: Vec<&str> = ops_log
            .lines()
            .filter(|line| OpsLogEvent::EditorOpCaptureProof.is_line(line))
            .collect();
        assert_eq!(receipts.len(), 2, "both receipts must be written:\n{ops_log}");
        assert!(
            receipts[0].contains("epoch_generation=12")
                && receipts[0].contains("operator_ops=3")
                && receipts[0].contains("non_operator_ops=1")
                && receipts[0].contains("shadow_replay=agreed")
                && receipts[0].contains("merge_base=abcdef012345")
                && receipts[0].contains("#opcaptureliveread"),
            "the receipt must name all four facts on one line: {}",
            receipts[0]
        );
        assert!(
            receipts[1].contains("epoch_generation=unknown")
                && receipts[1].contains("shadow_replay=disagreed")
                && receipts[1].contains("merge_base=unavailable"),
            "an absent generation or base must not read as a real value: {}",
            receipts[1]
        );
    }

    /// A proof receipt beside a missing producer marker is its own diagnosis: the
    /// reporter reached the record FFI and the FFI still wrote nothing.
    #[test]
    fn verify_names_the_proof_fields_when_the_producer_marker_is_still_missing() {
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_op_capture_proof epoch_generation=4 operator_ops=2 non_operator_ops=0 shadow_replay=agreed merge_base=abcdef012345 #opcaptureliveread doc=plan\n",
        );

        let err = verify(&doc, false).unwrap_err().to_string();
        assert!(
            err.contains("reached the record FFI")
                && err.contains("operator_ops=2")
                && err.contains("merge_base=abcdef012345"),
            "a proof beside a missing producer must name the fields it proved: {err}"
        );
        assert!(
            err.contains("reporter chain is unobserved"),
            "the refusal half of the diagnosis stays intact: {err}"
        );
    }

    #[test]
    fn verify_expect_cafe_demo_requires_byte_evidence() {
        let (_dir, doc) = setup_log(
            "[2026-06-24T00:00:00Z] editor_op_recorded kind=delete offset=6 delete_len=6 base=abc #qnodemerge4wire doc=plan\n\
             [2026-06-24T00:00:00Z] editor_op_recorded kind=insert offset=6 insert_bytes=6 insert_non_ascii=true base=abc #qnodemerge4wire doc=plan\n\
             [2026-06-24T00:00:01Z] editor_ops_for_base accepted=true ops=2 base=abc offsets=6,6 delete_bytes=6 insert_bytes=6 insert_non_ascii=true #qnodemerge4wire doc=plan\n",
        );

        let report = verify(&doc, true).unwrap();
        assert!(report.cafe_demo);
    }
}
