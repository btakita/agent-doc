//! # Module: merge
//!
//! ## Spec
//! - `agent_doc_merge::merge_contents_crdt(base_state, ours, theirs)`: conflict-free merge using Yrs CRDT.
//!   Delegates to `crdt::merge_by_component` (#hap7/#qcellmerge1) so each `agent:*`
//!   component — and each keyed child inside a list/exchange component — reconciles against
//!   its own base instead of merging the whole document as one blob; a concurrent operator
//!   edit to one queue item can no longer duplicate adjacent structure. Then encodes a fresh
//!   CRDT state from the merged result. Agent content (client_id=1) is ordered before human
//!   content (client_id=2) at the same insertion point by Yrs' native client-ID ordering — no
//!   post-merge reorder needed. Returns `(merged_text, new_crdt_state_bytes)`.
//! - `merge_contents(base, ours, theirs)`: 3-way merge via `git merge-file --diff3`.
//!   Auto-resolves append-only conflicts (empty original section) by concatenating ours then
//!   theirs. Preserves standard conflict markers only for true conflicts where existing lines
//!   were modified differently by both sides. Returns the merged content string.
//! - `resolve_append_conflicts(merged)` (private): post-processes `--diff3` conflict blocks.
//!   Scans each `<<<<<<< / ||||||| / ======= / >>>>>>>` block; if the `|||||||` section is
//!   empty/whitespace-only, auto-resolves by emitting ours then theirs. Otherwise keeps all
//!   markers intact. Returns `(resolved_content, has_remaining_conflicts)`.
//!
//! ## Agentic Contracts
//! - `agent_doc_merge::merge_contents_crdt` never returns conflict markers — CRDT guarantees a conflict-free
//!   result for all concurrent append/edit combinations.
//! - `agent_doc_merge::merge_contents_crdt` always returns a non-empty `state` vec usable as `base_state` in
//!   the next merge cycle; state encodes the full merged text, including user edits.
//! - `merge_contents` returns `Ok` even when conflicts remain — callers must inspect the
//!   content for `<<<<<<<` markers if they need to detect unresolved conflicts.
//! - When `base_state` is `None`, `agent_doc_merge::merge_contents_crdt` bootstraps from an empty CRDT doc
//!   and still produces a valid merged result.
//! - Agent content always appears before user content at the same insertion point.
//! - Returned CRDT state after a merge includes all user edits from the merge; using it as
//!   the base for the next cycle will not duplicate those edits.
//!
//! ## Evals
//! - `resolve_append_only_conflict`: both sides append at same point, original empty →
//!   markers removed, agent content before user content, no `<<<<<<<` in output.
//! - `preserve_true_conflict`: both sides modify same original line →
//!   conflict markers preserved, original section present in output.
//! - `mixed_append_and_true_conflicts`: one append-only block + one true-conflict block →
//!   append-only resolved, true-conflict keeps markers.
//! - `no_conflicts_passthrough`: no conflict markers in input → output identical to input.
//! - `multiline_append_conflict`: multi-line append-only block → all lines preserved,
//!   agent lines before user lines.
//! - `merge_contents_clean`: ours adds a line, theirs unchanged → agent addition present,
//!   no markers.
//! - `merge_contents_both_append`: both sides add different lines → both present, no markers.
//! - `crdt_merge_agent_and_user_append`: agent and user both append different content →
//!   both preserved, no conflict markers, base content intact.
//! - `crdt_merge_concurrent_same_line`: both sides insert at same position →
//!   both preserved, deterministic order, no conflict.
//! - `crdt_merge_no_base_state_bootstrap`: `None` base state → valid merge, non-empty state.
//! - `crdt_merge_one_side_unchanged`: only agent appended, theirs = base →
//!   merged equals ours exactly.
//! - `crdt_state_includes_user_edits_no_duplicates`: two consecutive merge cycles with
//!   concurrent user edit in cycle 1 → user edit appears exactly once in cycle 2 output.
//! - `crdt_multi_flush_no_duplicates`: multiple streaming flush cycles with concurrent user
//!   notes → each content piece appears exactly once across all flushes.

use anyhow::{Context, Result};
use std::process::Command;

/// Op-capture–aware CRDT merge (`#qnodemerge4wire` part 3).
///
/// Same contract and return shape as [`agent_doc_merge::merge_contents_crdt`], but for the
/// `theirs` (editor) side it prefers the editor's *real* captured operations
/// over a Myers diff-guess when they are available and provably aligned. This
/// is the live wiring that makes the op-capture supply chain
/// (`op_capture` sidecar ← editor reporters via FFI) actually change a merge.
///
/// Safety / no-regression invariant: the captured ops are used **only** when
/// both gates pass —
///   1. the sidecar's `base_hash` matches the resolved base text
///      ([`op_capture::editor_ops_for_base`]), and
///   2. replaying the ops onto that base reproduces `theirs` byte-for-byte
///      ([`agent_doc_merge::crdt::merge_with_editor_ops`]'s acceptance gate).
///      Otherwise the merge is byte-identical to [`agent_doc_merge::merge_contents_crdt`]: with no
///      sidecar (the production state until the editor reporters land), or with
///      stale/misaligned ops, this delegates to the exact same `crdt::merge` path.
///
/// The sidecar is **consumed**: it is cleared after the merge regardless of
/// whether the ops were used, so the next epoch starts clean and stale ops can
/// never leak into a later, unrelated merge.
pub fn merge_contents_crdt_with_ops(
    doc: &std::path::Path,
    base_state: Option<&[u8]>,
    ours: &str,
    theirs: &str,
) -> Result<(String, Vec<u8>)> {
    // Resolve the base text the ops must align to. `merge_with_editor_ops`
    // resolves the same base internally; we mirror it here only to gate which
    // ops (if any) to even offer.
    let base_text = match base_state {
        Some(bytes) => agent_doc_merge::crdt::CrdtDoc::decode_state(bytes)
            .map(|d| d.to_text())
            .unwrap_or_default(),
        None => String::new(),
    };

    let editor_ops = match crate::op_capture::editor_ops_for_base(doc, &base_text) {
        Ok(ops) => ops,
        Err(e) => {
            // A capture-load failure must never block a write — fall back to the
            // diff-guess path, but log it (never swallow).
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
            crate::ops_log::log_op(
                doc,
                &format!(
                    "editor_ops_for_base accepted=true ops={} base={} {} #qnodemerge4wire",
                    ops.len(),
                    crate::op_capture::content_hash(&base_text)
                        .get(..12)
                        .unwrap_or_default(),
                    summarize_editor_ops_for_log(ops)
                ),
            );
            agent_doc_merge::crdt::merge_with_editor_ops(base_state, ours, theirs, Some(ops))
                .context("CRDT merge (with editor ops) failed")?
        }
        // No captured editor ops (the common production path until the editor
        // reporters land): use the per-structural-node merge so the diff-guess
        // side can no longer duplicate structure across components/items
        // (#hap7/#qcellmerge1). The op-replay branch above stays whole-doc because
        // captured ops carry whole-document byte offsets.
        None => agent_doc_merge::merge_contents_crdt(base_state, ours, theirs)?.0,
    };

    // Consume the sidecar — the ops belonged to this base epoch (used or stale).
    if let Err(e) = crate::op_capture::clear_op_capture(doc) {
        eprintln!(
            "[op-capture] failed to clear sidecar for {} after merge ({e})",
            doc.display()
        );
    }

    let crdt_doc = agent_doc_merge::crdt::CrdtDoc::from_text(&merged);
    let state = crdt_doc.encode_state();
    eprintln!("[write] CRDT merge successful — no conflicts possible.");
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

/// 3-way merge using `git merge-file --diff3`.
///
/// Returns merged content. Append-only conflicts are auto-resolved by
/// concatenating both additions (ours first, then theirs).
/// True conflicts (where existing content was modified differently)
/// retain standard conflict markers.
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
        eprintln!("[write] Merge successful — user edits preserved.");
        return Ok(merged);
    }

    if output.status.code() == Some(1) {
        // Conflicts detected — try append-friendly resolution
        let (resolved, remaining_conflicts) = resolve_append_conflicts(&merged);
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

/// Resolve append-only conflicts in `git merge-file --diff3` output.
///
/// With `--diff3`, conflict blocks look like:
/// ```text
/// <<<<<<< agent-response
/// content added by agent
/// ||||||| original
/// (empty if both sides only appended)
/// =======
/// content added by user
/// >>>>>>> your-edits
/// ```
///
/// When the "original" section is empty (both sides added at the same
/// insertion point without modifying existing content), auto-resolve by
/// concatenating: ours (agent) first, then theirs (user).
///
/// Returns (resolved_content, has_remaining_conflicts).
fn resolve_append_conflicts(merged: &str) -> (String, bool) {
    let mut result = String::new();
    let mut has_remaining = false;
    let lines: Vec<&str> = merged.lines().collect();
    let len = lines.len();
    let mut i = 0;

    while i < len {
        if !lines[i].starts_with("<<<<<<< ") {
            result.push_str(lines[i]);
            result.push('\n');
            i += 1;
            continue;
        }

        // Parse conflict block
        let conflict_start = i;
        i += 1; // skip <<<<<<< marker

        // Collect "ours" section
        let mut ours_lines: Vec<&str> = Vec::new();
        while i < len && !lines[i].starts_with("||||||| ") && !lines[i].starts_with("=======") {
            ours_lines.push(lines[i]);
            i += 1;
        }

        // Collect "original" section (diff3)
        let mut original_lines: Vec<&str> = Vec::new();
        if i < len && lines[i].starts_with("||||||| ") {
            i += 1; // skip ||||||| marker
            while i < len && !lines[i].starts_with("=======") {
                original_lines.push(lines[i]);
                i += 1;
            }
        }

        // Skip ======= marker
        if i < len && lines[i].starts_with("=======") {
            i += 1;
        }

        // Collect "theirs" section
        let mut theirs_lines: Vec<&str> = Vec::new();
        while i < len && !lines[i].starts_with(">>>>>>> ") {
            theirs_lines.push(lines[i]);
            i += 1;
        }

        // Skip >>>>>>> marker
        if i < len && lines[i].starts_with(">>>>>>> ") {
            i += 1;
        }

        // Check if append-only: original section is empty or whitespace-only
        let is_append_only = original_lines.iter().all(|l| l.trim().is_empty());

        if is_append_only {
            // Auto-resolve: ours (agent) first, then theirs (user)
            for line in &ours_lines {
                result.push_str(line);
                result.push('\n');
            }
            for line in &theirs_lines {
                result.push_str(line);
                result.push('\n');
            }
        } else {
            // True conflict — preserve markers
            has_remaining = true;
            result.push_str(lines[conflict_start]);
            result.push('\n');
            for line in &ours_lines {
                result.push_str(line);
                result.push('\n');
            }
            // Reconstruct ||||||| section
            if !original_lines.is_empty() {
                result.push_str("||||||| original\n");
                for line in &original_lines {
                    result.push_str(line);
                    result.push('\n');
                }
            }
            result.push_str("=======\n");
            for line in &theirs_lines {
                result.push_str(line);
                result.push('\n');
            }
            result.push_str(">>>>>>> your-edits\n");
        }
    }

    // Handle trailing: if original didn't end with newline but we added one
    if !merged.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    (result, has_remaining)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_append_only_conflict() {
        let merged = "\
Before conflict
<<<<<<< agent-response
Agent added this line.
||||||| original
=======
User added this line.
>>>>>>> your-edits
After conflict
";
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(!has_remaining);
        assert!(resolved.contains("Agent added this line."));
        assert!(resolved.contains("User added this line."));
        assert!(!resolved.contains("<<<<<<<"));
        assert!(!resolved.contains(">>>>>>>"));
        // Agent content comes before user content
        let agent_pos = resolved.find("Agent added this line.").unwrap();
        let user_pos = resolved.find("User added this line.").unwrap();
        assert!(agent_pos < user_pos);
    }

    #[test]
    fn preserve_true_conflict() {
        let merged = "\
<<<<<<< agent-response
Agent changed this.
||||||| original
Original line that both sides modified.
=======
User changed this differently.
>>>>>>> your-edits
";
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(has_remaining);
        assert!(resolved.contains("<<<<<<<"));
        assert!(resolved.contains(">>>>>>>"));
        assert!(resolved.contains("Original line that both sides modified."));
    }

    #[test]
    fn mixed_append_and_true_conflicts() {
        let merged = "\
Clean line.
<<<<<<< agent-response
Agent appended here.
||||||| original
=======
User appended here.
>>>>>>> your-edits
Middle line.
<<<<<<< agent-response
Agent rewrote this.
||||||| original
Was originally this.
=======
User rewrote this differently.
>>>>>>> your-edits
End line.
";
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(has_remaining);
        // Append-only conflict was resolved
        assert!(resolved.contains("Agent appended here."));
        assert!(resolved.contains("User appended here."));
        // True conflict kept markers
        assert!(resolved.contains("<<<<<<<"));
        assert!(resolved.contains("Was originally this."));
    }

    #[test]
    fn no_conflicts_passthrough() {
        let merged = "Line one.\nLine two.\nLine three.\n";
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(!has_remaining);
        assert_eq!(resolved, merged);
    }

    #[test]
    fn multiline_append_conflict() {
        let merged = "\
<<<<<<< agent-response
Agent line 1.
Agent line 2.
Agent line 3.
||||||| original
=======
User line 1.
User line 2.
>>>>>>> your-edits
";
        let (resolved, has_remaining) = resolve_append_conflicts(merged);
        assert!(!has_remaining);
        assert!(resolved.contains("Agent line 1.\nAgent line 2.\nAgent line 3.\n"));
        assert!(resolved.contains("User line 1.\nUser line 2.\n"));
        // Agent before user
        assert!(resolved.find("Agent line 1.").unwrap() < resolved.find("User line 1.").unwrap());
    }

    #[test]
    fn merge_contents_clean() {
        let base = "Line 1\nLine 2\n";
        let ours = "Line 1\nLine 2\nAgent added\n";
        let theirs = "Line 1\nLine 2\n";
        let result = merge_contents(base, ours, theirs).unwrap();
        assert!(result.contains("Agent added"));
    }

    #[test]
    fn crdt_merge_agent_and_user_append() {
        let base = "# Doc\n\nBase content.\n";
        let ours = "# Doc\n\nBase content.\n\nAgent response.\n";
        let theirs = "# Doc\n\nBase content.\n\nUser addition.\n";

        let base_doc = agent_doc_merge::crdt::CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(merged.contains("Agent response."));
        assert!(merged.contains("User addition."));
        assert!(merged.contains("Base content."));
        assert!(!merged.contains("<<<<<<<"));
    }

    #[test]
    fn crdt_merge_preserves_user_backlog_id_rename() {
        // #backlog-id-user-rename-revert regression: when the operator renames a
        // backlog item's leading `[#id]` on disk (body unchanged), a CRDT merge
        // of the agent/snapshot side (still the OLD id) against the disk side
        // (the NEW id) must KEEP the user's renamed id — never revert it to the
        // snapshot's id. (Confirms the merge layer does not cause the historical
        // revert; the rename is authoritative.)
        let base = "<!-- agent:backlog -->\n- [ ] [#abcd] do the thing\n<!-- /agent:backlog -->\n";
        let ours = base; // agent/snapshot side unchanged — still the old id
        let theirs =
            "<!-- agent:backlog -->\n- [ ] [#abcd-2] do the thing\n<!-- /agent:backlog -->\n";

        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(
            merged.contains("[#abcd-2] do the thing"),
            "user's renamed id must survive the merge:\n{merged}"
        );
        assert!(
            !merged.contains("[#abcd] do the thing"),
            "merge must not revert to the snapshot-side old id:\n{merged}"
        );
    }

    #[test]
    fn fmreset_repro_operator_frontmatter_value_edit_preserved() {
        // #fmreset repro: the operator hand-edits a frontmatter value on disk
        // (claude_model: sonnet → opus) while the agent side (snapshot) still
        // carries the OLD value. The convergence merge must KEEP the operator's
        // edited value, never revert it to the snapshot/base value.
        let base = "---\nagent_doc_format: template\nclaude_model: sonnet\n---\n\n# Title\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let ours = base; // agent/snapshot side unchanged — still the old model
        let theirs = "---\nagent_doc_format: template\nclaude_model: opus\n---\n\n# Title\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";

        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(
            merged.contains("claude_model: opus"),
            "operator's edited frontmatter value must survive the merge:\n{merged}"
        );
        assert!(
            !merged.contains("claude_model: sonnet"),
            "merge must not revert to the snapshot-side old frontmatter value:\n{merged}"
        );
    }

    #[test]
    fn fmreset_repro_operator_frontmatter_value_edit_preserved_no_base_state() {
        // #fmreset repro (stale/None base epoch): same operator edit but the CRDT
        // base state is absent (first merge after a reset). The frontmatter value
        // change must still resolve to the operator's value, with no duplicate key.
        let ours = "---\nagent_doc_format: template\nclaude_model: sonnet\n---\n\n# Title\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let theirs = "---\nagent_doc_format: template\nclaude_model: opus\n---\n\n# Title\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";

        let (merged, _state) = agent_doc_merge::merge_contents_crdt(None, ours, theirs).unwrap();
        assert!(
            merged.contains("claude_model: opus"),
            "operator's edited frontmatter value must survive the no-base merge:\n{merged}"
        );
        assert_eq!(
            merged.matches("claude_model:").count(),
            1,
            "frontmatter key must not be duplicated by the merge:\n{merged}"
        );
        assert!(
            !merged.contains("sonnet"),
            "merge must not retain the snapshot-side old frontmatter value:\n{merged}"
        );
    }

    #[test]
    fn fmreset_repro_operator_fm_edit_while_agent_appends_response() {
        // #fmreset realistic finalize: the agent appends a `### Re:` response to
        // exchange (ours = snapshot baseline + response, OLD frontmatter) while the
        // operator concurrently edits frontmatter on disk (theirs = OLD body +
        // NEW frontmatter). The merge must keep BOTH the agent response AND the
        // operator's frontmatter edit.
        let base = "---\nagent_doc_format: template\nclaude_model: sonnet\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let ours = "---\nagent_doc_format: template\nclaude_model: sonnet\n---\n\n<!-- agent:exchange -->\n### Re: topic\nAgent response body.\n<!-- /agent:exchange -->\n";
        let theirs = "---\nagent_doc_format: template\nclaude_model: opus\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";

        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(
            merged.contains("### Re: topic"),
            "agent response must survive:\n{merged}"
        );
        assert!(
            merged.contains("claude_model: opus"),
            "operator frontmatter edit must survive:\n{merged}"
        );
        assert!(
            !merged.contains("claude_model: sonnet"),
            "merge must not revert frontmatter to snapshot value:\n{merged}"
        );
    }

    #[test]
    fn fmreset_repro_operator_multiline_preset_edit_preserved() {
        // #fmreset: operator edits a multi-line `prompt_presets` value. The merge
        // must keep the operator's edited preset body.
        let base = "---\nagent_doc_format: template\nprompt_presets:\n  \"#1\": |\n    old line\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let ours = base;
        let theirs = "---\nagent_doc_format: template\nprompt_presets:\n  \"#1\": |\n    new line\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";

        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(
            merged.contains("new line"),
            "operator's edited preset must survive:\n{merged}"
        );
        assert!(
            !merged.contains("old line"),
            "merge must not revert preset to snapshot value:\n{merged}"
        );
    }

    #[test]
    fn fmreset_repro_agent_managed_resume_write_still_applies() {
        // #fmreset guard: when the AGENT updates a managed key (resume) and the
        // operator did NOT touch frontmatter, the agent's write must still apply.
        let base = "---\nagent_doc_session: sid\nclaude_model: sonnet\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let ours = "---\nagent_doc_session: sid\nresume: conv-123\nclaude_model: sonnet\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let theirs = base; // operator untouched
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(
            merged.contains("resume: conv-123"),
            "agent-managed resume write must apply:\n{merged}"
        );
    }

    #[test]
    fn fmreset_repro_concurrent_operator_and_agent_field_merge() {
        // #fmreset field-wise: operator edits an operator-owned key (claude_model)
        // AND the agent writes a managed key (resume) in the SAME cycle. Both must
        // survive — neither overwrites the other.
        let base = "---\nagent_doc_session: sid\nclaude_model: sonnet\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let ours = "---\nagent_doc_session: sid\nresume: conv-123\nclaude_model: sonnet\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let theirs = "---\nagent_doc_session: sid\nclaude_model: opus\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(
            merged.contains("resume: conv-123"),
            "agent-managed resume write must apply:\n{merged}"
        );
        assert!(
            merged.contains("claude_model: opus") && !merged.contains("claude_model: sonnet"),
            "operator frontmatter edit must win for operator-owned key:\n{merged}"
        );
    }

    #[test]
    fn fmreset_repro_operator_unknown_key_preserved() {
        // #fmreset: operator adds an arbitrary YAML key not modeled by the typed
        // Frontmatter struct. It must survive the merge (no struct round-trip drop).
        let base = "---\nagent_doc_format: template\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let ours = base;
        let theirs = "---\nagent_doc_format: template\nmy_custom_key: hello\n---\n\n<!-- agent:exchange -->\n<!-- /agent:exchange -->\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(
            merged.contains("my_custom_key: hello"),
            "operator's custom frontmatter key must survive:\n{merged}"
        );
    }

    #[test]
    fn crdt_merge_concurrent_queue_insert_no_structural_duplication() {
        // #hap7/#qcellmerge1 regression at the orchestration wrapper: the operator
        // inserts a new queue item while the agent is appending a `### Re:`
        // response in the SAME cycle. The old whole-document merge could
        // splice/duplicate adjacent structure (operator-reported: a queue prompt
        // duplicated mid-turn). With per-structural-node scoping each component
        // reconciles against its own base, so the new queue item appears exactly
        // once and the response never splices into the queue.
        let base = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
                    <!-- agent:queue -->\n- do [#aaaa]\n<!-- /agent:queue -->\n";
        // Agent side: appended a response block to exchange; queue untouched.
        let ours = "<!-- agent:exchange -->\n### Re: topic\nAgent response body.\n<!-- /agent:exchange -->\n\n\
                    <!-- agent:queue -->\n- do [#aaaa]\n<!-- /agent:queue -->\n";
        // Operator side: exchange untouched; inserted a new queue item.
        let theirs = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
                    <!-- agent:queue -->\n- do [#aaaa]\n- do [#bbbb]\n<!-- /agent:queue -->\n";

        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();

        assert!(
            merged.contains("### Re: topic"),
            "response present:\n{merged}"
        );
        assert_eq!(
            merged.matches("do [#bbbb]").count(),
            1,
            "new operator queue item must appear exactly once:\n{merged}"
        );
        assert_eq!(
            merged.matches("do [#aaaa]").count(),
            1,
            "existing queue item must not be duplicated:\n{merged}"
        );
        let queue_start = merged.find("<!-- agent:queue -->").unwrap();
        assert!(
            !merged[queue_start..].contains("Agent response body."),
            "agent response must not splice into the queue component:\n{merged}"
        );
    }

    #[test]
    fn crdt_merge_operator_queue_delete_wins_over_agent_update() {
        let base = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- do [#aaaa] keep\n- do [#bbbb] delete me\n<!-- /agent:queue -->\n";
        // Agent side: stale lifecycle update to the item the operator removed.
        let ours = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- do [#aaaa] keep\n- ~~do [#bbbb] delete me~~\n<!-- /agent:queue -->\n";
        // Operator side: deleted the queue item from the live document.
        let theirs = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- do [#aaaa] keep\n<!-- /agent:queue -->\n";

        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();

        assert!(
            merged.contains("[#aaaa]"),
            "untouched queue item must survive:\n{merged}"
        );
        assert!(
            !merged.contains("[#bbbb]"),
            "operator-deleted queue item must not be resurrected:\n{merged}"
        );
    }

    #[test]
    fn crdt_merge_same_free_text_queue_node_extension_no_truncated_duplicate() {
        // #ftqpartialdup/#hap7 verification: a free-text queue item can be
        // partially present in the snapshot while the operator continues typing
        // that same queue node and the agent appends exchange content. The merge
        // must keep the completed operator line exactly once, not retain a
        // truncated duplicate sibling beside it.
        let base = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- :pushpin: All upload preview dialogs should be full\n<!-- /agent:queue -->\n";
        let ours = "<!-- agent:exchange -->\n### Re: topic\nAgent response body.\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- :pushpin: All upload preview dialogs should be full\n<!-- /agent:queue -->\n";
        let theirs = "<!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
<!-- agent:queue -->\n- :pushpin: All upload preview dialogs should be full screen. deploy\n<!-- /agent:queue -->\n";

        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();

        assert!(
            merged.contains("### Re: topic"),
            "response present:\n{merged}"
        );
        assert!(
            merged.contains("All upload preview dialogs should be full screen. deploy"),
            "completed operator queue line present:\n{merged}"
        );
        assert_eq!(
            merged
                .lines()
                .filter(|line| line.contains("All upload preview dialogs"))
                .count(),
            1,
            "partial and completed free-text queue lines must not both survive:\n{merged}"
        );
    }

    #[test]
    fn crdt_merge_concurrent_same_line() {
        let base = "Line 1\nLine 3\n";
        let ours = "Line 1\nAgent\nLine 3\n";
        let theirs = "Line 1\nUser\nLine 3\n";

        let base_doc = agent_doc_merge::crdt::CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let (merged, _state) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        // Both preserved, deterministic ordering, no conflict
        assert!(merged.contains("Agent"));
        assert!(merged.contains("User"));
        assert!(merged.contains("Line 1"));
        assert!(merged.contains("Line 3"));
    }

    #[test]
    fn crdt_merge_no_base_state_bootstrap() {
        let ours = "Agent content.\n";
        let theirs = "User content.\n";

        let (merged, state) = agent_doc_merge::merge_contents_crdt(None, ours, theirs).unwrap();
        assert!(merged.contains("Agent content."));
        assert!(merged.contains("User content."));
        assert!(!state.is_empty());
    }

    #[test]
    fn merge_with_ops_consumes_and_clears_base_keyed_sidecar() {
        // End-to-end #qnodemerge4wire wiring (part 3): a recorded editor op for
        // the merge base is offered to the merge, then the sidecar is cleared so a
        // later, unrelated merge can never replay stale ops.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base = "# Doc\n\nBase line.\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();

        // The editor appended "User edit.\n" — captured against the merge base.
        let base_hash = crate::op_capture::content_hash(base);
        crate::op_capture::record_editor_op(
            &doc,
            &base_hash,
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: base.len(),
                text: "User edit.\n".to_string(),
            },
        )
        .unwrap();
        assert!(
            crate::op_capture::load_op_capture(&doc).unwrap().is_some(),
            "precondition: sidecar recorded"
        );

        let ours = "# Doc\n\nBase line.\n\nAgent response.\n"; // agent side
        let theirs = "# Doc\n\nBase line.\nUser edit.\n"; // disk side (editor's real op)

        let (merged, state) =
            merge_contents_crdt_with_ops(&doc, Some(&base_state), ours, theirs).unwrap();
        assert!(
            merged.contains("Agent response."),
            "agent side preserved:\n{merged}"
        );
        assert!(
            merged.contains("User edit."),
            "editor op preserved:\n{merged}"
        );
        assert!(!merged.contains("<<<<<<<"));
        assert!(!state.is_empty());

        // The sidecar is consumed exactly once — a later merge starts clean.
        assert!(
            crate::op_capture::load_op_capture(&doc).unwrap().is_none(),
            "sidecar must be cleared after the merge consumes its epoch"
        );
    }

    #[test]
    fn merge_with_ops_logs_accepted_non_ascii_byte_offsets() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base = "café 日本 😀\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
        let base_hash = crate::op_capture::content_hash(base);
        crate::op_capture::record_editor_op(
            &doc,
            &base_hash,
            agent_doc_merge::crdt::EditorOp::Delete { offset: 6, len: 6 },
        )
        .unwrap();
        crate::op_capture::record_editor_op(
            &doc,
            &base_hash,
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: 6,
                text: "世界".to_string(),
            },
        )
        .unwrap();

        let ours = "café 日本 😀\n\nAgent response.\n";
        let theirs = "café 世界 😀\n";
        let (merged, _state) =
            merge_contents_crdt_with_ops(&doc, Some(&base_state), ours, theirs).unwrap();
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

        let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("editor_ops_for_base accepted=true ops=2")
                && ops_log.contains("offsets=6,6")
                && ops_log.contains("delete_bytes=6")
                && ops_log.contains("insert_bytes=6")
                && ops_log.contains("insert_non_ascii=true"),
            "merge acceptance evidence missing:\n{ops_log}"
        );
    }

    #[test]
    fn merge_with_ops_falls_back_to_diff_guess_on_base_mismatch() {
        // When the captured ops were recorded against a DIFFERENT base than the
        // merge resolves, they are disqualified — the merge degrades to the plain
        // CRDT path and is byte-identical to merge_contents_crdt.
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "# plan\n").unwrap();

        let base = "# Doc\n\nBase line.\n";
        let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();

        // Op recorded against a stale base that no longer matches the merge base.
        crate::op_capture::record_editor_op(
            &doc,
            &crate::op_capture::content_hash("a totally different base\n"),
            agent_doc_merge::crdt::EditorOp::Insert {
                offset: 0,
                text: "x".to_string(),
            },
        )
        .unwrap();

        let ours = "# Doc\n\nBase line.\n\nAgent response.\n";
        let theirs = "# Doc\n\nBase line.\n\nUser addition.\n";

        let (with_ops, _) =
            merge_contents_crdt_with_ops(&doc, Some(&base_state), ours, theirs).unwrap();
        let (plain, _) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert_eq!(
            with_ops, plain,
            "stale-base ops must not change the merge result"
        );
        // The stale sidecar is still cleared (its epoch is over).
        assert!(crate::op_capture::load_op_capture(&doc).unwrap().is_none());
    }

    #[test]
    fn crdt_merge_one_side_unchanged() {
        let base = "Original.\n";
        let base_doc = agent_doc_merge::crdt::CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let ours = "Original.\nAgent added.\n";
        let (merged, _) =
            agent_doc_merge::merge_contents_crdt(Some(&base_state), ours, base).unwrap();
        assert_eq!(merged, ours);
    }

    #[test]
    fn merge_contents_both_append() {
        let base = "Line 1\n";
        let ours = "Line 1\nAgent response\n";
        let theirs = "Line 1\nUser edit\n";
        let result = merge_contents(base, ours, theirs).unwrap();
        // Both should be present, no conflict markers
        assert!(result.contains("Agent response"));
        assert!(result.contains("User edit"));
        assert!(!result.contains("<<<<<<<"));
    }

    /// Regression test: CRDT state must include user edits from the merge.
    ///
    /// Bug: After a merge cycle where the user edited concurrently, the CRDT
    /// state was rebuilt from `content_ours` (agent-only) instead of the merged
    /// state. On the next cycle, the merge saw user edits as new insertions
    /// relative to the stale base, producing duplicate text.
    ///
    /// This test simulates two consecutive merge cycles:
    /// 1. Agent writes response while user edits concurrently → merge
    /// 2. Agent writes another response using the CRDT state from cycle 1
    ///
    /// With the bug, cycle 2 would duplicate the user's edit from cycle 1.
    #[test]
    fn crdt_state_includes_user_edits_no_duplicates() {
        // --- Cycle 1: Initial state, agent responds, user edits concurrently ---
        let initial = "Why were the videos not public?\n";
        let initial_doc = agent_doc_merge::crdt::CrdtDoc::from_text(initial);
        let initial_state = initial_doc.encode_state();

        // Agent appends a response
        let ours_cycle1 = "Why were the videos not public?\nAlways publish public videos.\n";
        // User also edits concurrently (adds a line)
        let theirs_cycle1 = "Why were the videos not public?\nuser-edit-abc\n";

        let (merged1, state1) =
            agent_doc_merge::merge_contents_crdt(Some(&initial_state), ours_cycle1, theirs_cycle1)
                .unwrap();

        // Both edits present after cycle 1
        assert!(
            merged1.contains("Always publish public videos."),
            "missing agent response"
        );
        assert!(merged1.contains("user-edit-abc"), "missing user edit");

        // --- Cycle 2: Agent writes another response, no concurrent user edits ---
        // The agent's new content_ours includes the full merged result + new text
        let ours_cycle2 = format!("{}...unless explicitly set to private.\n", merged1);
        // No user edits this time — theirs is the same as what was written to disk
        let theirs_cycle2 = merged1.clone();

        let (merged2, _state2) =
            agent_doc_merge::merge_contents_crdt(Some(&state1), &ours_cycle2, &theirs_cycle2)
                .unwrap();

        // The user's edit should appear exactly ONCE, not duplicated
        let edit_count = merged2.matches("user-edit-abc").count();
        assert_eq!(
            edit_count, 1,
            "User edit duplicated! Appeared {} times in:\n{}",
            edit_count, merged2
        );

        // Agent's content from both cycles should be present
        assert!(merged2.contains("Always publish public videos."));
        assert!(merged2.contains("...unless explicitly set to private."));
    }

    /// Regression test: Multiple flush cycles with concurrent user edits.
    ///
    /// Simulates the streaming checkpoint pattern where the agent flushes
    /// partial responses multiple times while the user keeps editing.
    #[test]
    fn crdt_multi_flush_no_duplicates() {
        let base = "# Doc\n\nQuestion here.\n";
        let base_doc = agent_doc_merge::crdt::CrdtDoc::from_text(base);
        let state0 = base_doc.encode_state();

        // Flush 1: Agent starts responding, user adds a note
        let ours1 = "# Doc\n\nQuestion here.\n\n### Re: Answer\n\nFirst paragraph.\n";
        let theirs1 = "# Doc\n\nQuestion here.\n\n> user note\n";
        let (merged1, state1) =
            agent_doc_merge::merge_contents_crdt(Some(&state0), ours1, theirs1).unwrap();
        assert!(merged1.contains("First paragraph."));
        assert!(merged1.contains("> user note"));

        // Flush 2: Agent continues, user adds another note
        let ours2 = format!("{}\nSecond paragraph.\n", merged1);
        let theirs2 = format!("{}\n> another note\n", merged1);
        let (merged2, _state2) =
            agent_doc_merge::merge_contents_crdt(Some(&state1), &ours2, &theirs2).unwrap();

        // Each piece of content appears exactly once
        assert_eq!(
            merged2.matches("First paragraph.").count(),
            1,
            "First paragraph duplicated in:\n{}",
            merged2
        );
        assert_eq!(
            merged2.matches("> user note").count(),
            1,
            "User note duplicated in:\n{}",
            merged2
        );
        assert!(merged2.contains("Second paragraph."));
        assert!(merged2.contains("> another note"));
    }
}
