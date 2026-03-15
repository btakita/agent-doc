//! `agent-doc write` — Append an assistant response to a session document.
//!
//! Usage: echo "response text" | agent-doc write <file.md>
//!
//! Atomically appends `## Assistant\n\n<content>\n\n## User\n\n` to the document,
//! handling concurrent edits via 3-way merge. Updates the snapshot after writing.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;

use crate::{merge, recover, snapshot, template};
use crate::snapshot::find_project_root;

/// Run the write command: append assistant response to document.
///
/// `baseline` is the document content at the time the response was generated.
/// If omitted, the current document content is used (no merge needed).
pub fn run(file: &Path, baseline: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

    if response.trim().is_empty() {
        anyhow::bail!("empty response — nothing to write");
    }

    // Strip leading "## Assistant" heading if present — the write command adds its own
    let response = strip_assistant_heading(&response);

    // Read document state before lock (for baseline)
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let base = baseline.unwrap_or(&content_at_start);

    // Save response to pending store (survives context compaction)
    recover::save_pending(file, &response)?;

    // Build "ours": baseline + response appended
    let mut content_ours = base.to_string();
    // Ensure trailing newline before appending
    if !content_ours.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("## Assistant\n\n");
    content_ours.push_str(&response);
    if !response.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("\n## User\n\n");

    // Acquire advisory lock
    let doc_lock = acquire_doc_lock(file)?;

    // Re-read file to check for user edits
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let final_content = if content_current == base {
        // No edits — use our version directly
        content_ours.clone()
    } else {
        eprintln!("[write] File was modified during response generation. Merging...");
        merge::merge_contents(base, &content_ours, &content_current)?
    };

    atomic_write(file, &final_content)?;

    // Save snapshot as content_ours (baseline + response), NOT final_content.
    // If the user edited during response generation, final_content includes their
    // edits via merge. Saving content_ours ensures the next diff detects those edits.
    snapshot::save(file, &content_ours)?;

    drop(doc_lock);

    // Clear pending response after successful write
    recover::clear_pending(file)?;

    eprintln!("[write] Response appended to {}", file.display());
    Ok(())
}

/// Run the template write command: parse patch blocks and apply to components.
///
/// `baseline` is the document content at the time the response was generated.
pub fn run_template(file: &Path, baseline: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

    if response.trim().is_empty() {
        anyhow::bail!("empty response — nothing to write");
    }

    // Save response to pending store (survives context compaction)
    recover::save_pending(file, &response)?;

    // Parse patch blocks from response
    let (patches, unmatched) = template::parse_patches(&response)
        .context("failed to parse patch blocks from response")?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    // Read document state
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let base = baseline.unwrap_or(&content_at_start);

    // Apply patches to baseline
    let content_ours = template::apply_patches(base, &patches, &unmatched, file)
        .context("failed to apply template patches")?;

    // Acquire advisory lock
    let doc_lock = acquire_doc_lock(file)?;

    // Re-read file to check for user edits
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let final_content = if content_current == base {
        content_ours.clone()
    } else {
        eprintln!("[write] File was modified during response generation. Merging...");
        merge::merge_contents(base, &content_ours, &content_current)?
    };

    atomic_write(file, &final_content)?;

    // Save snapshot as content_ours (baseline + response), not final_content
    snapshot::save(file, &content_ours)?;

    drop(doc_lock);

    // Clear pending response after successful write
    recover::clear_pending(file)?;

    eprintln!(
        "[write] Template patches applied to {} ({} components patched)",
        file.display(),
        patches.len()
    );
    Ok(())
}

/// Run the stream write command: template patches with CRDT merge (conflict-free).
///
/// Like `run_template`, but uses CRDT merge instead of git merge-file.
/// `baseline` is the document content at the time the response was generated.
pub fn run_stream(file: &Path, baseline: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

    if response.trim().is_empty() {
        anyhow::bail!("empty response — nothing to write");
    }

    // Save response to pending store (survives context compaction)
    recover::save_pending(file, &response)?;

    // Parse patch blocks from response
    let (patches, unmatched) = template::parse_patches(&response)
        .context("failed to parse patch blocks from response")?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    // Read document state
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let base = baseline.unwrap_or(&content_at_start);

    // Apply patches to baseline with replace mode for exchange component.
    // The --stream path receives the complete intended exchange content
    // (not a delta), so append mode would duplicate the user's prompt
    // that already exists in the baseline's exchange component.
    let mut mode_overrides = std::collections::HashMap::new();
    mode_overrides.insert("exchange".to_string(), "replace".to_string());
    let content_ours = template::apply_patches_with_overrides(
        base, &patches, &unmatched, file, &mode_overrides,
    ).context("failed to apply template patches")?;

    // Acquire advisory lock
    let doc_lock = acquire_doc_lock(file)?;

    // Re-read file to check for user edits
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let (final_content, crdt_state) = if content_current == base {
        // No edits — build CRDT state from result
        let doc = crate::crdt::CrdtDoc::from_text(&content_ours);
        (content_ours.clone(), doc.encode_state())
    } else {
        eprintln!("[write] File was modified during response generation. CRDT merging...");
        eprintln!("[write] CRDT merge successful — no conflicts possible.");
        let crdt_state = snapshot::load_crdt(file)?;
        merge::merge_contents_crdt(crdt_state.as_deref(), &content_ours, &content_current)?
    };

    atomic_write(file, &final_content)?;

    // Save snapshot as content_ours (baseline + response), not final_content.
    // If the user edited concurrently, final_content includes their edits via CRDT merge.
    // Saving content_ours ensures the next diff detects those concurrent edits.
    snapshot::save(file, &content_ours)?;
    // Save the merged CRDT state — NOT a fresh state from content_ours.
    // Using content_ours would lose user edits from the merge, causing
    // the next merge cycle to re-insert them as duplicates.
    snapshot::save_crdt(file, &crdt_state)?;

    drop(doc_lock);

    // Clear pending response after successful write
    recover::clear_pending(file)?;

    eprintln!(
        "[write] Stream patches applied to {} ({} components patched, CRDT)",
        file.display(),
        patches.len()
    );
    Ok(())
}

/// IPC mode: write a JSON patch file for IDE plugin consumption.
///
/// Instead of modifying the document directly, writes a JSON file to
/// `.agent-doc/patches/<hash>.json`. The IDE plugin picks it up, applies
/// patches via Document API (no external file change dialog), and deletes
/// the file as ACK. Falls back to direct stream write on timeout.
pub fn run_ipc(file: &Path, baseline: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    // Read response from stdin
    let mut response = String::new();
    std::io::stdin()
        .read_to_string(&mut response)
        .context("failed to read response from stdin")?;

    if response.trim().is_empty() {
        anyhow::bail!("empty response — nothing to write");
    }

    // Save response to pending store (survives context compaction)
    recover::save_pending(file, &response)?;

    // Parse patch blocks from response
    let (patches, unmatched) = template::parse_patches(&response)
        .context("failed to parse patch blocks from response")?;

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    // Build IPC patch file
    let canonical = file.canonicalize()?;
    let hash = snapshot::doc_hash(file)?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let patches_dir = project_root.join(".agent-doc/patches");
    std::fs::create_dir_all(&patches_dir)?;
    let patch_file = patches_dir.join(format!("{}.json", hash));

    // Separate frontmatter patch from component patches
    let mut frontmatter_yaml: Option<String> = None;
    let ipc_patches: Vec<serde_json::Value> = patches
        .iter()
        .filter_map(|p| {
            if p.name == "frontmatter" {
                frontmatter_yaml = Some(p.content.trim().to_string());
                None
            } else {
                Some(serde_json::json!({
                    "component": p.name,
                    "content": p.content,
                }))
            }
        })
        .collect();

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
        "unmatched": unmatched.trim(),
        "baseline": baseline.unwrap_or(""),
    });

    if let Some(ref yaml) = frontmatter_yaml {
        ipc_payload["frontmatter"] = serde_json::Value::String(yaml.clone());
    }

    // Atomic write of patch file
    atomic_write(
        &patch_file,
        &serde_json::to_string_pretty(&ipc_payload)?,
    )?;

    eprintln!(
        "[write] IPC patch written to {} ({} components)",
        patch_file.display(),
        patches.len()
    );

    // Poll for ACK (plugin deletes file after applying)
    let timeout = std::time::Duration::from_secs(2);
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if !patch_file.exists() {
            // Plugin consumed the patch — update snapshot from current file
            let content = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {} after IPC", file.display()))?;
            snapshot::save(file, &content)?;
            let crdt_doc = crate::crdt::CrdtDoc::from_text(&content);
            snapshot::save_crdt(file, &crdt_doc.encode_state())?;
            recover::clear_pending(file)?;
            eprintln!("[write] IPC patch consumed by plugin — snapshot updated");
            return Ok(());
        }
        std::thread::sleep(poll_interval);
    }

    // Timeout — fall back to direct stream write
    eprintln!("[write] IPC timeout ({}s) — falling back to direct write", timeout.as_secs());
    // Clean up the unconsumed patch file
    let _ = std::fs::remove_file(&patch_file);

    // Fall back to stream write logic
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;
    let base = baseline.unwrap_or(&content_at_start);
    let mut content_ours = template::apply_patches(base, &patches, &unmatched, file)
        .context("failed to apply template patches")?;

    // Apply frontmatter patch if present
    if let Some(ref yaml) = frontmatter_yaml {
        content_ours = crate::frontmatter::merge_fields(&content_ours, yaml)
            .context("failed to apply frontmatter patch")?;
    }
    let doc_lock = acquire_doc_lock(file)?;
    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;
    let (final_content, crdt_state) = if content_current == base {
        let doc = crate::crdt::CrdtDoc::from_text(&content_ours);
        (content_ours.clone(), doc.encode_state())
    } else {
        eprintln!("[write] File was modified during response generation. CRDT merging...");
        let crdt_state = snapshot::load_crdt(file)?;
        merge::merge_contents_crdt(crdt_state.as_deref(), &content_ours, &content_current)?
    };
    atomic_write(file, &final_content)?;
    snapshot::save(file, &content_ours)?;
    snapshot::save_crdt(file, &crdt_state)?;
    drop(doc_lock);
    recover::clear_pending(file)?;
    eprintln!(
        "[write] Stream patches applied to {} ({} components patched, CRDT fallback)",
        file.display(),
        patches.len()
    );
    Ok(())
}

/// Apply stream-mode patches from a string (not stdin).
/// Used by `recover` to apply orphaned stream responses.
#[allow(dead_code)] // Wired by recover module when stream mode recovery is added
pub fn apply_stream_from_string(file: &Path, response: &str) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (patches, unmatched) = template::parse_patches(response)
        .context("failed to parse patch blocks from response")?;

    let content_ours = template::apply_patches(&content, &patches, &unmatched, file)
        .context("failed to apply template patches")?;

    let doc_lock = acquire_doc_lock(file)?;

    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let (final_content, crdt_state) = if content_current == content {
        let doc = crate::crdt::CrdtDoc::from_text(&content_ours);
        (content_ours.clone(), doc.encode_state())
    } else {
        let crdt_state = snapshot::load_crdt(file)?;
        merge::merge_contents_crdt(crdt_state.as_deref(), &content_ours, &content_current)?
    };

    atomic_write(file, &final_content)?;
    // Save snapshot as content_ours, not final_content
    snapshot::save(file, &content_ours)?;
    snapshot::save_crdt(file, &crdt_state)?;
    drop(doc_lock);
    eprintln!("[write] Stream patches applied to {}", file.display());
    Ok(())
}

/// Apply an append-mode response from a string (not stdin).
/// Used by `recover` to apply orphaned responses.
pub fn apply_append_from_string(file: &Path, response: &str) -> Result<()> {
    let response = strip_assistant_heading(response);
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let mut content_ours = content.clone();
    if !content_ours.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("## Assistant\n\n");
    content_ours.push_str(&response);
    if !response.ends_with('\n') {
        content_ours.push('\n');
    }
    content_ours.push_str("\n## User\n\n");

    let doc_lock = acquire_doc_lock(file)?;

    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let final_content = if content_current == content {
        content_ours.clone()
    } else {
        merge::merge_contents(&content, &content_ours, &content_current)?
    };

    atomic_write(file, &final_content)?;
    // Save snapshot as content_ours, not final_content
    snapshot::save(file, &content_ours)?;
    drop(doc_lock);
    eprintln!("[write] Response appended to {}", file.display());
    Ok(())
}

/// Apply template-mode patches from a string (not stdin).
/// Used by `recover` to apply orphaned template responses.
pub fn apply_template_from_string(file: &Path, response: &str) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (patches, unmatched) = template::parse_patches(response)
        .context("failed to parse patch blocks from response")?;

    let content_ours = template::apply_patches(&content, &patches, &unmatched, file)
        .context("failed to apply template patches")?;

    let doc_lock = acquire_doc_lock(file)?;

    let content_current = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {}", file.display()))?;

    let final_content = if content_current == content {
        content_ours.clone()
    } else {
        merge::merge_contents(&content, &content_ours, &content_current)?
    };

    atomic_write(file, &final_content)?;
    // Save snapshot as content_ours, not final_content
    snapshot::save(file, &content_ours)?;
    drop(doc_lock);
    eprintln!("[write] Template patches applied to {}", file.display());
    Ok(())
}

/// Attempt to write via IPC (file-based patch for IDE plugin consumption).
///
/// Writes a JSON patch file to `.agent-doc/patches/` and polls for the plugin
/// to consume it (delete the file as ACK). Returns `Ok(true)` if the plugin
/// consumed the patch, `Ok(false)` if it timed out (caller should fall back
/// to direct write).
///
/// This is safe to call unconditionally — if no plugin is active, it simply
/// returns `false` after a short timeout.
pub fn try_ipc(
    file: &Path,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
    frontmatter_yaml: Option<&str>,
    baseline: Option<&str>,
) -> Result<bool> {
    let canonical = file.canonicalize()?;
    let hash = snapshot::doc_hash(file)?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let patches_dir = project_root.join(".agent-doc/patches");

    // Only attempt IPC if the patches directory exists (plugin has started)
    if !patches_dir.exists() {
        return Ok(false);
    }

    let patch_file = patches_dir.join(format!("{}.json", hash));

    // Separate frontmatter patch from component patches
    let ipc_patches: Vec<serde_json::Value> = patches
        .iter()
        .filter(|p| p.name != "frontmatter")
        .map(|p| {
            serde_json::json!({
                "component": p.name,
                "content": p.content,
            })
        })
        .collect();

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
        "unmatched": unmatched.trim(),
        "baseline": baseline.unwrap_or(""),
    });

    if let Some(yaml) = frontmatter_yaml {
        ipc_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
    }

    write_ipc_and_poll(&patch_file, &ipc_payload, file, patches.len())
}

/// Attempt to write full document content via IPC.
///
/// Like `try_ipc()` but replaces the entire document content instead of
/// applying component patches. Used by append-mode documents that don't
/// have `<!-- agent:name -->` component markers.
///
/// Returns `Ok(true)` if the plugin consumed the patch, `Ok(false)` on timeout.
pub fn try_ipc_full_content(
    file: &Path,
    content: &str,
) -> Result<bool> {
    let canonical = file.canonicalize()?;
    let hash = snapshot::doc_hash(file)?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
    let patches_dir = project_root.join(".agent-doc/patches");

    // Only attempt IPC if the patches directory exists (plugin has started)
    if !patches_dir.exists() {
        return Ok(false);
    }

    let patch_file = patches_dir.join(format!("{}.json", hash));

    let ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": [],
        "unmatched": "",
        "baseline": "",
        "fullContent": content,
    });

    write_ipc_and_poll(&patch_file, &ipc_payload, file, 0)
}

/// Write an IPC patch file and poll for plugin ACK (file deletion).
///
/// Returns `Ok(true)` if consumed, `Ok(false)` on timeout.
fn write_ipc_and_poll(
    patch_file: &Path,
    payload: &serde_json::Value,
    doc_file: &Path,
    patch_count: usize,
) -> Result<bool> {
    // Atomic write of patch file
    atomic_write(
        patch_file,
        &serde_json::to_string_pretty(payload)?,
    )?;

    eprintln!(
        "[write] IPC patch written to {} ({} components)",
        patch_file.display(),
        patch_count
    );

    // Poll for ACK (plugin deletes file after applying)
    let timeout = std::time::Duration::from_secs(2);
    let poll_interval = std::time::Duration::from_millis(100);
    let start = std::time::Instant::now();

    while start.elapsed() < timeout {
        if !patch_file.exists() {
            // Plugin consumed the patch — update snapshot from current file
            let content = std::fs::read_to_string(doc_file)
                .with_context(|| format!("failed to read {} after IPC", doc_file.display()))?;
            snapshot::save(doc_file, &content)?;
            let crdt_doc = crate::crdt::CrdtDoc::from_text(&content);
            snapshot::save_crdt(doc_file, &crdt_doc.encode_state())?;
            eprintln!("[write] IPC patch consumed by plugin — snapshot updated");
            return Ok(true);
        }
        std::thread::sleep(poll_interval);
    }

    // Timeout — clean up unconsumed patch file
    eprintln!("[write] IPC timeout ({}s) — falling back to direct write", timeout.as_secs());
    let _ = std::fs::remove_file(patch_file);
    Ok(false)
}

// ---------------------------------------------------------------------------
// Internal helpers (same patterns as submit.rs)
// ---------------------------------------------------------------------------

/// Strip leading `## Assistant` and trailing `## User` headings from response text.
///
/// The `agent-doc write` command adds its own `## Assistant\n\n` prefix and
/// `\n## User\n\n` suffix, so if the agent response includes these headings,
/// we'd get duplicates. This strips them to prevent that.
pub fn strip_assistant_heading(response: &str) -> String {
    let mut result = response.to_string();

    // Strip leading ## Assistant
    let trimmed = result.trim_start();
    if let Some(rest) = trimmed.strip_prefix("## Assistant") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        result = rest.to_string();
    }

    // Strip trailing ## User (with optional whitespace/newlines after)
    let trimmed_end = result.trim_end();
    if let Some(before) = trimmed_end.strip_suffix("## User") {
        result = before.trim_end_matches('\n').to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
    }

    result
}

fn acquire_doc_lock(path: &Path) -> Result<std::fs::File> {
    let lock_path = crate::snapshot::lock_path_for(path)?;
    if let Some(parent) = lock_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let file = OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(&lock_path)
        .with_context(|| format!("failed to open doc lock {}", lock_path.display()))?;
    file.lock_exclusive()
        .with_context(|| format!("failed to acquire doc lock on {}", lock_path.display()))?;
    Ok(file)
}

/// Atomic write: write to temp file then rename. Public for use by compact.
pub fn atomic_write_pub(path: &Path, content: &str) -> Result<()> {
    atomic_write(path, content)
}

fn atomic_write(path: &Path, content: &str) -> Result<()> {
    use std::io::Write;
    let parent = path.parent().unwrap_or(Path::new("."));
    let mut tmp = tempfile::NamedTempFile::new_in(parent)
        .with_context(|| format!("failed to create temp file in {}", parent.display()))?;
    tmp.write_all(content.as_bytes())
        .with_context(|| "failed to write temp file")?;
    tmp.persist(path)
        .with_context(|| format!("failed to rename temp file to {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn write_appends_response() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello\n").unwrap();

        // Simulate stdin by calling run logic directly
        let base = fs::read_to_string(&doc).unwrap();
        let response = "This is the assistant response.";

        let mut content_ours = base.clone();
        if !content_ours.ends_with('\n') {
            content_ours.push('\n');
        }
        content_ours.push_str("## Assistant\n\n");
        content_ours.push_str(response);
        content_ours.push('\n');
        content_ours.push_str("\n## User\n\n");

        atomic_write(&doc, &content_ours).unwrap();

        let result = fs::read_to_string(&doc).unwrap();
        assert!(result.contains("## Assistant\n\nThis is the assistant response."));
        assert!(result.contains("\n\n## User\n\n"));
        assert!(result.contains("## User\n\nHello"));
    }

    #[test]
    fn write_updates_snapshot() {
        // Use a direct snapshot write/read to avoid CWD dependency.
        // The snapshot module uses relative paths (.agent-doc/snapshots/),
        // so we verify the pattern works via snapshot::path_for + direct I/O.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let content = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nResponse\n\n## User\n\n";
        fs::write(&doc, content).unwrap();

        // Verify snapshot path computation works
        let snap_path = snapshot::path_for(&doc).unwrap();
        assert!(snap_path.to_string_lossy().contains(".agent-doc/snapshots/"));

        // Verify atomic_write + read roundtrip (the core of snapshot save)
        let snap_abs = dir.path().join(&snap_path);
        fs::create_dir_all(snap_abs.parent().unwrap()).unwrap();
        fs::write(&snap_abs, content).unwrap();
        let loaded = fs::read_to_string(&snap_abs).unwrap();
        assert_eq!(loaded, content);
    }

    #[test]
    fn write_preserves_user_edits_via_merge() {
        let base = "---\nsession: test\n---\n\n## User\n\nOriginal question\n";
        let response = "My response";

        // "ours" = base + response
        let mut ours = base.to_string();
        ours.push_str("\n## Assistant\n\n");
        ours.push_str(response);
        ours.push_str("\n\n## User\n\n");

        // "theirs" = user added a follow-up to the User block
        let theirs = "---\nsession: test\n---\n\n## User\n\nOriginal question\nAnd a follow-up!\n";

        let merged = merge::merge_contents(base, &ours, theirs).unwrap();

        // Both the response and the user's follow-up should be in the merge
        assert!(merged.contains("My response"), "response missing from merge");
        assert!(merged.contains("And a follow-up!"), "user edit missing from merge");
    }

    #[test]
    fn write_no_merge_when_unchanged() {
        let base = "---\nsession: test\n---\n\n## User\n\nHello\n";
        let response = "Response here";

        let mut ours = base.to_string();
        ours.push_str("\n## Assistant\n\n");
        ours.push_str(response);
        ours.push_str("\n\n## User\n\n");

        // theirs == base (no edit)
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, base).unwrap();

        let doc_lock = acquire_doc_lock(&doc).unwrap();
        let content_current = fs::read_to_string(&doc).unwrap();

        let final_content = if content_current == base {
            ours.clone()
        } else {
            merge::merge_contents(base, &ours, &content_current).unwrap()
        };

        drop(doc_lock);
        assert_eq!(final_content, ours);
    }

    #[test]
    fn atomic_write_correct_content() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("atomic.md");
        atomic_write(&path, "hello world").unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), "hello world");
    }

    #[test]
    fn concurrent_writes_no_corruption() {
        use std::sync::{Arc, Barrier};

        let dir = TempDir::new().unwrap();
        let path = dir.path().join("concurrent.md");
        fs::write(&path, "initial").unwrap();

        let n = 20;
        let barrier = Arc::new(Barrier::new(n));
        let mut handles = Vec::new();

        for i in 0..n {
            let p = path.clone();
            let parent = dir.path().to_path_buf();
            let bar = Arc::clone(&barrier);
            let content = format!("writer-{}-content", i);
            handles.push(std::thread::spawn(move || {
                bar.wait();
                let mut tmp = tempfile::NamedTempFile::new_in(&parent).unwrap();
                std::io::Write::write_all(&mut tmp, content.as_bytes()).unwrap();
                tmp.persist(&p).unwrap();
            }));
        }

        for h in handles {
            h.join().unwrap();
        }

        let final_content = fs::read_to_string(&path).unwrap();
        assert!(
            final_content.starts_with("writer-") && final_content.ends_with("-content"),
            "unexpected content: {}",
            final_content
        );
    }

    #[test]
    fn snapshot_excludes_concurrent_user_edits() {
        // Regression test: when the user edits during response generation,
        // the snapshot should contain baseline + response ONLY (content_ours),
        // NOT the merged content that includes user edits.
        // This ensures the next diff detects the user's concurrent edits.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc").join("snapshots");
        fs::create_dir_all(&agent_doc_dir).unwrap();

        let doc = dir.path().join("test.md");
        let base = "---\nsession: test\n---\n\n## User\n\nOriginal question\n";
        fs::write(&doc, base).unwrap();

        // Build content_ours = baseline + response
        let response = "Agent response here";
        let mut content_ours = base.to_string();
        content_ours.push_str("\n## Assistant\n\n");
        content_ours.push_str(response);
        content_ours.push_str("\n\n## User\n\n");

        // Simulate user editing the file concurrently (adding a follow-up)
        let user_edited = format!("{}Follow-up question\n", base);
        fs::write(&doc, &user_edited).unwrap();

        // Merge: content_ours + user edits
        let merged = merge::merge_contents(base, &content_ours, &user_edited).unwrap();

        // Write merged content (includes both response and user edit)
        atomic_write(&doc, &merged).unwrap();
        assert!(merged.contains(response), "response missing from merged");
        assert!(merged.contains("Follow-up question"), "user edit missing from merged");

        // KEY: Save snapshot as content_ours (NOT merged)
        snapshot::save(&doc, &content_ours).unwrap();

        // Verify: snapshot should NOT contain user's concurrent edit
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(snap.contains(response), "snapshot should have response");
        assert!(
            !snap.contains("Follow-up question"),
            "snapshot must NOT contain concurrent user edit — \
             otherwise the next diff won't detect it"
        );

        // Verify: diff between snapshot and current file should detect user's edit
        let current = fs::read_to_string(&doc).unwrap();
        assert_ne!(snap, current, "snapshot and file should differ (user edit not in snapshot)");
        assert!(
            current.contains("Follow-up question"),
            "current file should contain user's edit"
        );
    }

    #[test]
    fn try_ipc_returns_false_when_no_patches_dir() {
        // Without .agent-doc/patches/, IPC should return false immediately
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let patches: Vec<crate::template::PatchBlock> = vec![];
        let result = try_ipc(&doc, &patches, "", None, None).unwrap();
        assert!(!result, "should return false when patches dir doesn't exist");
    }

    #[test]
    fn try_ipc_times_out_when_no_plugin() {
        // With .agent-doc/patches/ existing but no plugin consuming, should timeout
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n").unwrap();

        let patch = crate::template::PatchBlock {
            name: "exchange".to_string(),
            content: "new content".to_string(),
        };

        // This will timeout after 2s — patch file is written but never consumed
        let result = try_ipc(&doc, &[patch], "", None, None).unwrap();
        assert!(!result, "should return false on timeout (no plugin)");

        // Patch file should be cleaned up after timeout
        let patches_dir = agent_doc_dir.join("patches");
        let entries: Vec<_> = fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .collect();
        assert!(entries.is_empty(), "patch file should be cleaned up after timeout");
    }

    #[test]
    fn try_ipc_succeeds_when_plugin_consumes() {
        // Simulate plugin by spawning a thread that deletes the patch file
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        fs::write(&doc, "---\nsession: test\n---\n\n<!-- agent:exchange -->\ncontent\n<!-- /agent:exchange -->\n").unwrap();

        let patch = crate::template::PatchBlock {
            name: "exchange".to_string(),
            content: "new content".to_string(),
        };

        // Spawn "plugin" thread that watches for and deletes patch files
        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let result = try_ipc(&doc, &[patch], "", None, None).unwrap();
        assert!(result, "should return true when plugin consumes patch");
    }

    #[test]
    fn try_ipc_full_content_returns_false_when_no_patches_dir() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "content").unwrap();

        let result = try_ipc_full_content(&doc, "new content").unwrap();
        assert!(!result, "should return false when patches dir doesn't exist");
    }
}
