//! Git-backed merge adapters for agent-doc documents.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// 3-way merge using `git merge-file --diff3`.
///
/// Returns merged content. Append-only conflicts are auto-resolved by
/// concatenating both additions (ours first, then theirs). True conflicts retain
/// standard conflict markers.
pub fn merge_contents(base: &str, ours: &str, theirs: &str) -> Result<String> {
    let tmp = tempfile::TempDir::new().context("failed to create temp dir for merge")?;

    let base_path = tmp.path().join("base");
    let ours_path = tmp.path().join("ours");
    let theirs_path = tmp.path().join("theirs");

    std::fs::write(&base_path, base)?;
    std::fs::write(&ours_path, ours)?;
    std::fs::write(&theirs_path, theirs)?;

    let output = Command::new("git")
        .current_dir(tmp.path())
        .args([
            "merge-file",
            "-p",
            "--diff3",
            "-L",
            "agent-response",
            "-L",
            "original",
            "-L",
            "your-edits",
            &ours_path.to_string_lossy(),
            &base_path.to_string_lossy(),
            &theirs_path.to_string_lossy(),
        ])
        .output()?;

    let merged = String::from_utf8(output.stdout)
        .map_err(|e| anyhow::anyhow!("merge produced invalid UTF-8: {}", e))?;

    if output.status.success() {
        eprintln!("[write] Merge successful - user edits preserved.");
        return Ok(merged);
    }

    if output.status.code() == Some(1) {
        let (resolved, remaining_conflicts) = agent_doc_merge::resolve_append_conflicts(&merged);
        if remaining_conflicts {
            eprintln!(
                "[write] WARNING: True merge conflicts remain. Please resolve conflict markers manually."
            );
        } else {
            eprintln!("[write] Merge conflicts auto-resolved (append-friendly).");
        }
        return Ok(resolved);
    }

    anyhow::bail!(
        "git merge-file failed: {}",
        String::from_utf8_lossy(&output.stderr)
    )
}

/// Op-capture-aware CRDT merge.
///
/// Same contract and return shape as [`agent_doc_merge::merge_contents_crdt`],
/// but for the `theirs` (editor) side it prefers the editor's real captured
/// operations over a Myers diff guess when they are available and provably
/// aligned. Captured ops are used only when their state-ledger base hash matches the
/// resolved base text and replaying the ops onto that base reproduces `theirs`.
/// Otherwise this degrades to the plain CRDT merge path.
///
/// The capture row is consumed after the merge attempt, whether the ops
/// were used or stale, so stale ops cannot leak into a later epoch.
pub fn merge_contents_crdt_with_ops(
    doc: &Path,
    base_state: Option<&[u8]>,
    ours: &str,
    theirs: &str,
    mut logger: impl FnMut(&Path, &str),
) -> Result<(String, Vec<u8>)> {
    let base_text = match base_state {
        Some(bytes) => agent_doc_merge::crdt::CrdtDoc::decode_state(bytes)
            .map(|d| d.to_text())
            .unwrap_or_default(),
        None => String::new(),
    };

    let editor_ops = match agent_doc_op_capture_io::editor_ops_for_base(doc, &base_text) {
        Ok(ops) => ops,
        Err(e) => {
            eprintln!(
                "[op-capture] failed to load ops for {} ({e}); falling back to diff-guess",
                doc.display()
            );
            None
        }
    };

    let merged = match &editor_ops {
        Some(ops) => {
            eprintln!(
                "[op-capture] offering {} captured editor op(s) to the merge for {} (#qnodemerge4wire)",
                ops.len(),
                doc.display()
            );
            logger(
                doc,
                &format!(
                    "editor_ops_for_base accepted=true ops={} base={} {} #qnodemerge4wire",
                    ops.len(),
                    agent_doc_hash::content_hash(&base_text)
                        .get(..12)
                        .unwrap_or_default(),
                    summarize_editor_ops_for_log(ops)
                ),
            );
            agent_doc_merge::crdt::merge_with_editor_ops(base_state, ours, theirs, Some(ops))
                .context("CRDT merge (with editor ops) failed")?
        }
        None => agent_doc_merge::merge_contents_crdt(base_state, ours, theirs)?.0,
    };

    if let Err(e) = agent_doc_op_capture_io::clear_op_capture(doc) {
        eprintln!(
            "[op-capture] failed to clear state row for {} after merge ({e})",
            doc.display()
        );
    }

    let crdt_doc = agent_doc_merge::crdt::CrdtDoc::from_text(&merged);
    let state = crdt_doc.encode_state();
    eprintln!("[write] CRDT merge successful - no conflicts possible.");
    Ok((merged, state))
}

fn summarize_editor_ops_for_log(ops: &[agent_doc_merge::crdt::EditorOp]) -> String {
    let mut offsets = Vec::new();
    let mut delete_bytes = 0usize;
    let mut insert_bytes = 0usize;
    let mut insert_non_ascii = false;
    for op in ops {
        match op {
            agent_doc_merge::crdt::EditorOp::Insert { offset, text } => {
                offsets.push(offset.to_string());
                insert_bytes = insert_bytes.saturating_add(text.len());
                insert_non_ascii |= !text.is_ascii();
            }
            agent_doc_merge::crdt::EditorOp::Delete { offset, len } => {
                offsets.push(offset.to_string());
                delete_bytes = delete_bytes.saturating_add(*len);
            }
        }
    }
    format!(
        "offsets={} delete_bytes={} insert_bytes={} insert_non_ascii={}",
        offsets.join(","),
        delete_bytes,
        insert_bytes,
        insert_non_ascii
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merge_contents_clean() {
        let base = "Line 1\nLine 2\n";
        let ours = "Line 1\nLine 2\nAgent added\n";
        let theirs = "Line 1\nLine 2\n";
        let result = merge_contents(base, ours, theirs).unwrap();
        assert!(result.contains("Agent added"));
    }

    #[test]
    fn merge_contents_both_append() {
        let base = "Line 1\n";
        let ours = "Line 1\nAgent response\n";
        let theirs = "Line 1\nUser edit\n";
        let result = merge_contents(base, ours, theirs).unwrap();
        assert!(result.contains("Agent response"));
        assert!(result.contains("User edit"));
        assert!(!result.contains("<<<<<<<"));
    }

    fn noop_log(_: &Path, _: &str) {}

    #[test]
    fn merge_with_ops_uses_aligned_editor_ops_and_consumes_state_row() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base = "# Doc\n\nBase line.\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let base_hash = agent_doc_hash::content_hash(base);
        agent_doc_op_capture_io::record_editor_op(
            &doc,
            &base_hash,
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: base.len(),
                text: "User edit.\n".to_string(),
            },
        )
        .unwrap();

        let ours = "# Doc\n\nBase line.\n\nAgent response.\n";
        let theirs = "# Doc\n\nBase line.\nUser edit.\n";

        let (merged, state) =
            merge_contents_crdt_with_ops(&doc, Some(&base_state), ours, theirs, noop_log).unwrap();
        assert!(
            merged.contains("Agent response."),
            "agent side lost:\n{merged}"
        );
        assert!(merged.contains("User edit."), "editor op lost:\n{merged}");
        assert!(!merged.contains("<<<<<<<"));
        assert!(!state.is_empty());
        assert!(
            agent_doc_op_capture_io::load_op_capture(&doc)
                .unwrap()
                .is_none(),
            "state row must be cleared after the merge consumes its epoch"
        );
    }

    #[test]
    fn merge_with_ops_logs_accepted_non_ascii_byte_offsets() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base = "cafe 日本 😀\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let base_hash = agent_doc_hash::content_hash(base);
        agent_doc_op_capture_io::record_editor_op(
            &doc,
            &base_hash,
            agent_doc_merge::crdt::EditorOp::Delete { offset: 5, len: 6 },
        )
        .unwrap();
        agent_doc_op_capture_io::record_editor_op(
            &doc,
            &base_hash,
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: 5,
                text: "世界".to_string(),
            },
        )
        .unwrap();

        let ours = "cafe 日本 😀\n\nAgent response.\n";
        let theirs = "cafe 世界 😀\n";
        let mut log_lines = Vec::new();
        let (merged, _state) =
            merge_contents_crdt_with_ops(&doc, Some(&base_state), ours, theirs, |_, msg| {
                log_lines.push(msg.to_string())
            })
            .unwrap();

        assert!(
            merged.contains("Agent response."),
            "agent side lost:\n{merged}"
        );
        assert!(
            merged.contains("世界"),
            "non-ASCII editor op lost:\n{merged}"
        );
        assert!(
            !merged.contains('�'),
            "merge introduced mojibake:\n{merged}"
        );
        assert!(!merged.contains("<<<<<<<"));
        let ops_log = log_lines.join("\n");
        assert!(
            ops_log.contains("editor_ops_for_base accepted=true ops=2")
                && ops_log.contains("offsets=5,5")
                && ops_log.contains("delete_bytes=6")
                && ops_log.contains("insert_bytes=6")
                && ops_log.contains("insert_non_ascii=true"),
            "merge acceptance evidence missing:\n{ops_log}"
        );
    }

    #[test]
    fn merge_with_ops_falls_back_to_plain_crdt_on_base_mismatch() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base = "# Doc\n\nBase line.\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();

        agent_doc_op_capture_io::record_editor_op(
            &doc,
            &agent_doc_hash::content_hash("a totally different base\n"),
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: 0,
                text: "x".to_string(),
            },
        )
        .unwrap();

        let ours = "# Doc\n\nBase line.\n\nAgent response.\n";
        let theirs = "# Doc\n\nBase line.\n\nUser addition.\n";

        let (with_ops, _) =
            merge_contents_crdt_with_ops(&doc, Some(&base_state), ours, theirs, noop_log).unwrap();
        let (plain, _) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert_eq!(
            with_ops, plain,
            "stale-base ops must not change the merge result"
        );
        assert!(
            agent_doc_op_capture_io::load_op_capture(&doc)
                .unwrap()
                .is_none()
        );
    }
}
