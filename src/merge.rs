//! 3-way merge with append-friendly conflict resolution.
//!
//! Uses `git merge-file --diff3` for the base merge, then post-processes
//! to auto-resolve append-only conflicts (where both sides added content
//! at the same position without modifying existing lines).

use anyhow::{Context, Result};
use std::process::Command;

/// CRDT-based merge: conflict-free merge using Yrs CRDT.
///
/// Returns (merged_text, new_crdt_state).
/// `base_state` is the CRDT state from the last write (None on first use).
pub fn merge_contents_crdt(
    base_state: Option<&[u8]>,
    ours: &str,
    theirs: &str,
) -> Result<(String, Vec<u8>)> {
    let merged = crate::crdt::merge(base_state, ours, theirs)
        .context("CRDT merge failed")?;
    // Build fresh CRDT state from the merged result
    let doc = crate::crdt::CrdtDoc::from_text(&merged);
    let state = doc.encode_state();
    eprintln!("[write] CRDT merge successful — no conflicts possible.");
    Ok((merged, state))
}

/// 3-way merge using `git merge-file --diff3`.
///
/// Returns merged content. Append-only conflicts are auto-resolved by
/// concatenating both additions (ours first, then theirs).
/// True conflicts (where existing content was modified differently)
/// retain standard conflict markers.
pub fn merge_contents(base: &str, ours: &str, theirs: &str) -> Result<String> {
    let tmp = tempfile::TempDir::new()
        .context("failed to create temp dir for merge")?;

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
            "-L", "agent-response",
            "-L", "original",
            "-L", "your-edits",
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
            eprintln!("[write] WARNING: True merge conflicts remain. Please resolve conflict markers manually.");
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

        let base_doc = crate::crdt::CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let (merged, _state) = merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
        assert!(merged.contains("Agent response."));
        assert!(merged.contains("User addition."));
        assert!(merged.contains("Base content."));
        assert!(!merged.contains("<<<<<<<"));
    }

    #[test]
    fn crdt_merge_concurrent_same_line() {
        let base = "Line 1\nLine 3\n";
        let ours = "Line 1\nAgent\nLine 3\n";
        let theirs = "Line 1\nUser\nLine 3\n";

        let base_doc = crate::crdt::CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let (merged, _state) = merge_contents_crdt(Some(&base_state), ours, theirs).unwrap();
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

        let (merged, state) = merge_contents_crdt(None, ours, theirs).unwrap();
        assert!(merged.contains("Agent content."));
        assert!(merged.contains("User content."));
        assert!(!state.is_empty());
    }

    #[test]
    fn crdt_merge_one_side_unchanged() {
        let base = "Original.\n";
        let base_doc = crate::crdt::CrdtDoc::from_text(base);
        let base_state = base_doc.encode_state();

        let ours = "Original.\nAgent added.\n";
        let (merged, _) = merge_contents_crdt(Some(&base_state), ours, base).unwrap();
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
}
