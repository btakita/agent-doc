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
    cafe_demo: bool,
}

pub fn run(file: &Path, expect_cafe_demo: bool) -> Result<()> {
    let report = verify(file, expect_cafe_demo)?;
    println!("op-capture verification ok for {}", file.display());
    println!("ops_log={}", report.ops_log.display());
    println!(
        "doc={} editor_op_recorded={} editor_ops_recorded={} editor_ops_for_base_accepted={} failures={}",
        report.doc_tag,
        report.recorded_count,
        report.batch_recorded_count,
        report.accepted_count,
        report.failed_count
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
    if recorded.is_empty() && batch_recorded.is_empty() {
        bail!(
            "missing editor-op producer marker for {doc_tag} in {} — expected either \
             `{}` (one-op FFI, #qnodemerge4wire) or `{EDITOR_OPS_RECORDED_MARKER} ... transaction=batch` \
             (batch FFI, #qbasehashmemo, the path the JetBrains TypingTracker uses)",
            ops_log.display(),
            OpsLogEvent::EditorOpRecorded,
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
        verify_cafe_demo(&recorded, &accepted, &ops_log, &doc_tag)?;
    }

    Ok(OpCaptureVerification {
        doc_tag,
        ops_log,
        recorded_count: recorded.len(),
        batch_recorded_count: batch_recorded.len(),
        accepted_count: accepted.len(),
        failed_count,
        cafe_demo: expect_cafe_demo,
    })
}

fn verify_cafe_demo(
    recorded: &[&str],
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

    let recorded_delete = recorded.iter().any(|line| {
        line.contains("kind=delete") && line.contains("offset=6") && line.contains("delete_len=6")
    });
    let recorded_non_ascii_insert = recorded.iter().any(|line| {
        line.contains("kind=insert")
            && line.contains("offset=6")
            && line.contains("insert_non_ascii=true")
    });
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
            "missing cafe-demo byte evidence for {doc_tag} in {}; expected delete offset=6/delete_len=6 and accepted non-ASCII insert",
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
