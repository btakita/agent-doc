//! # Module: write
//!
//! All write paths for agent responses: inline append, template patch, stream
//! (CRDT), IPC-to-IDE-plugin, and recovery helpers. Each path follows the same
//! invariant: save pending → acquire lock → compute `content_ours` (baseline +
//! response) → merge with any concurrent user edits → atomic write → save
//! snapshot as `content_ours` (not the merged result) → clear pending.
//!
//! ## Write dedup (v0.28.2)
//!
//! All four write paths (`run`, `run_template`, `run_stream` disk, `run_stream`
//! IPC) skip the actual write when the merged/patched content is identical to
//! the current file on disk. Dedup events are logged to stderr and appended
//! (with backtrace) to `/tmp/agent-doc-write-dedup.log` for diagnosis.
//!
//! ## Pane ownership verification (v0.28.2)
//!
//! `verify_pane_ownership()` is called at the top of `run`, `run_template`, and
//! `run_stream`. It checks that the current tmux pane matches the session
//! registry entry for the document's `session` frontmatter field. If a
//! *different* pane definitively owns the session, the write is rejected with an
//! error suggesting `agent-doc claim`. The check is lenient: it passes silently
//! when not in tmux, when there is no session ID, or when the pane is
//! indeterminate.
//!
//! ## Spec
//!
//! - `run`: inline (User/Assistant) mode. Reads response from stdin, strips any
//!   leading `## Assistant` / trailing `## User` headings the agent may have
//!   echoed, then appends `## Assistant\n\n<response>\n\n## User\n\n` to the
//!   document. Saves a pre-response snapshot for undo. If the file changed
//!   since `baseline`, performs a 3-way git merge before writing.
//!
//! - `run_template`: template-component mode. Parses `patch:NAME` fence blocks
//!   from stdin, sanitizes any `<!-- agent:NAME -->` markers in patch content
//!   (prevents parser corruption), applies patches to the baseline via
//!   `template::apply_patches`, then performs the same lock/merge/atomic-write
//!   cycle as `run`.
//!
//! - `run_stream`: CRDT stream-flush mode. Like `run_template` but uses
//!   `merge::merge_contents_crdt` for conflict-free merge. Saves both a text
//!   snapshot and a CRDT state snapshot after every flush. Supports IPC-first
//!   writes: when `.agent-doc/patches/` exists and `--force-disk` is not set,
//!   tries `try_ipc` first; on timeout (exit code 75 / `EX_TEMPFAIL`) leaves a
//!   fallback patch file for the plugin to pick up later.
//!
//! - `run_ipc`: explicit IPC-only mode. Serialises patches as JSON to
//!   `.agent-doc/patches/<hash>.json`, polls for the plugin to delete the file
//!   as ACK (2 s timeout), then falls back to the direct CRDT disk path.
//!
//! - `try_ipc`: low-level IPC helper used by `run_stream`. Writes a JSON patch
//!   file (component patches + optional frontmatter + `reposition_boundary`
//!   flag) and polls for ACK. Returns `Ok(true)` on success, `Ok(false)` on
//!   timeout. Safe to call unconditionally — returns `false` immediately when
//!   `.agent-doc/patches/` does not exist. Synthesises a boundary-aware
//!   exchange patch when no explicit patches exist but unmatched content and a
//!   boundary marker are present.
//!
//! - `try_ipc_full_content`: like `try_ipc` but sends a full document
//!   replacement (`fullContent` field) instead of component patches. Used by
//!   inline-mode documents without component markers.
//!
//! - `try_ipc_reposition_boundary`: fire-and-forget IPC signal with empty
//!   patches and `reposition_boundary: true`. Moves the boundary marker to
//!   end-of-exchange without touching the working tree (preserves cursor/undo
//!   in the IDE). Non-fatal on timeout.
//!
//! - `apply_append_from_string`: recovery variant of `run` — takes response
//!   text directly instead of reading stdin. Used by `recover` to replay
//!   orphaned inline responses.
//!
//! - `apply_template_from_string`: recovery variant of `run_template`.
//!
//! - `apply_stream_from_string`: recovery variant of `run_stream` (CRDT merge).
//!
//! - `sanitize_component_tags`: escapes `<!-- agent:NAME -->` and
//!   `<!-- /agent:NAME -->` markers appearing in patch content to prevent the
//!   component parser from treating them as real delimiters.
//!
//! - `strip_assistant_heading`: strips a leading `## Assistant` heading and/or
//!   trailing `## User` heading from a response string. Prevents duplicate
//!   headings when the agent echoes them.
//!
//! - `atomic_write_pub`: public thin wrapper around the internal `atomic_write`
//!   (write to temp file + rename). Used by `compact` and other modules.
//!
//! ## Agentic Contracts
//!
//! - Snapshot invariant: the snapshot saved after every write contains
//!   `content_ours` (baseline + response), never the merged result. This
//!   ensures the next diff cycle sees concurrent user edits as a diff, not as
//!   already-committed content.
//! - Pending response is saved before any write attempt and cleared only after
//!   a successful write, so an interrupted write is recoverable.
//! - Pre-response snapshot is saved before acquiring the lock so `undo` can
//!   restore the document to its pre-response state regardless of merge
//!   outcome.
//! - All writes are atomic (temp file + rename). Partial writes never corrupt
//!   the document.
//! - Advisory file lock (`flock`) serialises concurrent writes to the same
//!   document; the lock is dropped immediately after `atomic_write`.
//! - `try_ipc` / `try_ipc_full_content` return `false` immediately (no I/O
//!   wait) when `.agent-doc/patches/` does not exist — callers may invoke them
//!   unconditionally without performance cost when no plugin is active.
//! - IPC writes include `reposition_boundary: true` so the plugin moves the
//!   boundary marker to end-of-exchange in the same Document API transaction as
//!   the patch, avoiding a second round-trip.
//! - CRDT snapshots are saved from the merged state (not from `content_ours`)
//!   so subsequent merges use the correct shared ancestor, preventing
//!   character-level duplication across cycles.
//! - `sanitize_component_tags` is applied to every patch block before any
//!   write path applies it, preventing agent-generated examples of component
//!   syntax from corrupting future parses.
//!
//! ## Evals
//!
//! - `write_appends_response`: inline write appends `## Assistant\n\n<text>` +
//!   `\n## User\n\n` to a document → both headings and content present in file.
//! - `write_updates_snapshot`: after a write the snapshot path resolves to
//!   `.agent-doc/snapshots/` and a roundtrip read/write is lossless.
//! - `write_preserves_user_edits_via_merge`: 3-way merge when user appends to
//!   `## User` block concurrently → merged result contains both response and
//!   user addition.
//! - `write_no_merge_when_unchanged`: when file equals baseline at lock time,
//!   `content_ours` is used directly (no merge invoked).
//! - `atomic_write_correct_content`: temp-rename write produces the exact bytes
//!   supplied.
//! - `concurrent_writes_no_corruption`: 20 threads racing on atomic_write →
//!   final file is one complete writer's content (no corruption or partial
//!   writes).
//! - `snapshot_excludes_concurrent_user_edits`: snapshot saved as
//!   `content_ours`; concurrent user edit is present in the file but absent
//!   from the snapshot, so the next diff detects it.
//! - `try_ipc_returns_false_when_no_patches_dir`: `try_ipc` with no
//!   `.agent-doc/patches/` → returns `false` immediately.
//! - `try_ipc_times_out_when_no_plugin`: `.agent-doc/patches/` exists but
//!   nothing consumes the file → returns `false` after 2 s; patch file cleaned
//!   up.
//! - `try_ipc_succeeds_when_plugin_consumes`: mock plugin thread deletes patch
//!   file within 2 s → `try_ipc` returns `true`.
//! - `try_ipc_full_content_returns_false_when_no_patches_dir`: full-content IPC
//!   with no patches dir → returns `false`.
//! - `sanitize_escapes_open_agent_tag`: `<!-- agent:exchange -->` inside patch
//!   content is escaped to `&lt;!-- agent:exchange --&gt;`.
//! - *(aspirational)* `run_stream_crdt_merge`: concurrent user keystroke during
//!   stream flush → CRDT merge produces text containing both agent response and
//!   user addition without character interleaving.
//! - *(aspirational)* `ipc_fallback_on_timeout`: `run_stream` with IPC timeout
//!   exits with code 75 and leaves a patch file for deferred plugin pickup.
//! - `normalize_user_prompts_new_line_gets_prefix`: user adds "Hello" to exchange
//!   → normalized content has "❯ Hello".
//! - `normalize_user_prompts_agent_response_not_prefixed`: agent response lines in content_ours
//!   must NOT get `❯ ` prefix — only user-added lines (snapshot→baseline diff) are prefixed.
//! - `normalize_user_prompts_blank_line_skipped`: blank line added → no prefix.
//! - `normalize_user_prompts_heading_skipped`: line starting with `#` → no prefix.
//! - `normalize_user_prompts_already_prefixed_skipped`: line already starts with `❯` → unchanged.
//! - `normalize_user_prompts_existing_content_unchanged`: lines from snapshot → unchanged (no double-prefix).
//! - `normalize_user_prompts_restores_prefix_lost_in_file`: snapshot has `❯ do`, baseline (file) has `do` → restored to `❯ do`.
//! - `normalize_user_prompts_no_exchange_passthrough`: document without exchange → returned unchanged.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::OpenOptions;
use std::io::Read;
use std::path::Path;

use crate::{component, frontmatter, merge, recover, sessions, snapshot, template};
use crate::snapshot::find_project_root;

/// Helper: extract boundary_id for a named component from the document.
///
/// Searches for `<!-- agent:boundary:UUID -->` inside the component's content,
/// skipping matches inside fenced code blocks and inline code spans.
fn find_boundary_id(doc: &str, component_name: &str) -> Option<String> {
    let components = component::parse(doc).ok()?;
    let comp = components.iter().find(|c| c.name == component_name)?;
    let content = &doc[comp.open_end..comp.close_start];
    let code_ranges = component::find_code_ranges(doc);

    // Scan for boundary marker in component content, skipping code blocks
    let prefix = "<!-- agent:boundary:";
    let suffix = " -->";
    let mut search_from = 0;
    while let Some(start) = content[search_from..].find(prefix) {
        let abs_start = comp.open_end + search_from + start;
        // Skip if inside a code block
        if code_ranges.iter().any(|&(cs, ce)| abs_start >= cs && abs_start < ce) {
            search_from += start + prefix.len();
            continue;
        }
        let id_start = search_from + start + prefix.len();
        if let Some(end) = content[id_start..].find(suffix) {
            let id = &content[id_start..id_start + end];
            if !id.is_empty() {
                return Some(id.to_string());
            }
        }
        break;
    }
    None
}

/// Check if a component is append-mode (needs boundary markers).
fn is_append_mode_component(name: &str) -> bool {
    matches!(name, "exchange" | "findings")
}

/// Extract lines that were normalized by `normalize_user_prompts_in_exchange`.
///
/// Compares `before` and `after` content for exchange components and returns
/// lines whose `❯ `-stripped version exists in `before` but the prefixed version
/// does not — i.e., lines that the normalization step added `❯ ` to.
///
/// These are passed to the IPC plugin so it can apply the same normalization
/// to the live editor document.
pub fn extract_normalization_targets(before: &str, after: &str) -> Vec<String> {
    let before_comps = component::parse(before).unwrap_or_default();
    let after_comps = component::parse(after).unwrap_or_default();

    let before_exc = before_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(before))
        .unwrap_or("");
    let after_exc = after_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|c| c.content(after))
        .unwrap_or("");

    if before_exc == after_exc {
        return vec![];
    }

    let before_lines: std::collections::HashSet<&str> = before_exc.lines().collect();
    let mut targets = Vec::new();

    for line in after_exc.lines() {
        if let Some(stripped) = line.strip_prefix("❯ ") {
            // Line has ❯  prefix in 'after': check if the original (no prefix) was in 'before'
            // and the prefixed version was NOT in 'before' (= normalization added it this cycle)
            if before_lines.contains(stripped) && !before_lines.contains(line) {
                targets.push(stripped.to_string());
            }
        }
    }

    targets
}

/// Add `❯ ` prefix to user-added lines in exchange components.
///
/// Compares the exchange content in `baseline` against `snapshot` to identify
/// lines the user typed this cycle (Insert lines in the diff). Those lines are
/// then prefixed with `❯ ` in `content` (content_ours = baseline + agent patches).
///
/// Using `baseline` (not `content_ours`) for the diff is critical: after
/// `apply_patches_with_overrides`, the boundary marker is repositioned to the end
/// of the exchange. Everything before it — including the agent's new response —
/// is the "user region". Diffing `snapshot → content_ours user_region` would
/// incorrectly mark agent response lines as Insert and prefix them. Diffing
/// `snapshot → baseline` identifies only genuine user additions.
///
/// Skips lines that: are blank, already start with `❯`, start with `<!--`,
/// or start with `#`. Non-destructive if no exchange component is present or
/// no new lines are found.
///
/// Both disk and IPC write paths call this after computing `content_ours` so the
/// snapshot and merged document consistently show `❯ ` on user input.
pub fn normalize_user_prompts_in_exchange(content: &str, baseline: &str, snapshot: &str) -> String {
    let Ok(content_comps) = component::parse(content) else {
        return content.to_string();
    };
    let baseline_comps = component::parse(baseline).unwrap_or_default();
    let snap_comps = component::parse(snapshot).unwrap_or_default();

    let Some(exchange) = content_comps.iter().find(|c| c.name == "exchange") else {
        return content.to_string();
    };

    let baseline_exc = baseline_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|e| e.content(baseline))
        .unwrap_or("");
    let snap_exc = snap_comps
        .iter()
        .find(|c| c.name == "exchange")
        .map(|e| e.content(snapshot))
        .unwrap_or("");

    let exc_content = exchange.content(content);

    // Find the boundary marker in content_ours — user region is before, agent region after.
    let boundary_prefix = "<!-- agent:boundary:";
    let boundary_pos = {
        let mut pos = exc_content.len();
        let mut offset = 0;
        for line in exc_content.lines() {
            if line.trim().starts_with(boundary_prefix) {
                pos = offset;
                break;
            }
            offset += line.len() + 1;
        }
        pos
    };
    let content_user_region = &exc_content[..boundary_pos];
    let content_agent_region = &exc_content[boundary_pos..];

    // Strip boundary markers from baseline and snapshot for diffing.
    // Preserves trailing newline if present in the original.
    let strip = |s: &str| -> String {
        let filtered: Vec<&str> = s.lines()
            .filter(|l| !l.trim().starts_with(boundary_prefix))
            .collect();
        let mut out = filtered.join("\n");
        if s.ends_with('\n') && !out.is_empty() {
            out.push('\n');
        }
        out
    };
    let baseline_stripped = strip(baseline_exc);
    let snap_stripped = strip(snap_exc);

    // Diff snapshot → baseline to find user-added lines (not agent lines).
    use similar::{ChangeTag, TextDiff};
    let diff = TextDiff::from_lines(snap_stripped.as_str(), baseline_stripped.as_str());
    let mut user_added = std::collections::HashSet::<String>::new();
    for change in diff.iter_all_changes() {
        if change.tag() == ChangeTag::Insert {
            let line = change.value().trim_end_matches('\n');
            let trimmed = line.trim();
            if !trimmed.is_empty()
                && !trimmed.starts_with('❯')
                && !trimmed.starts_with("<!-- ")
                && !trimmed.starts_with('#')
                && !trimmed.starts_with("```")
                && !trimmed.starts_with('"')
            {
                user_added.insert(line.to_string());
            }
        }
    }

    if user_added.is_empty() {
        return content.to_string();
    }

    // Apply ❯  prefix to user-added lines in content_user_region.
    // Agent response lines (not in user_added) pass through unchanged.
    let mut normalized_user = String::new();
    for line in content_user_region.lines() {
        if user_added.contains(line) {
            normalized_user.push_str("❯ ");
        }
        normalized_user.push_str(line);
        normalized_user.push('\n');
    }
    if !content_user_region.is_empty() && !content_user_region.ends_with('\n') {
        normalized_user.truncate(normalized_user.len() - 1);
    }
    if content_user_region.is_empty() {
        normalized_user.clear();
    }

    let new_exc_content = format!("{}{}", normalized_user, content_agent_region);
    exchange.replace_content(content, &new_exc_content)
}


/// Detect whether a baseline is stale relative to the current snapshot.
///
/// Only checks **append-mode** components (exchange, findings, etc.) — these grow
/// monotonically and must contain the snapshot's committed content. Replace-mode
/// components (status, pending) are freely user-editable and are skipped.
///
/// Returns `true` if the baseline is stale (missing committed snapshot content).
pub fn is_stale_baseline(baseline: &str, snapshot: &str) -> bool {
    let base_clean = strip_boundary_for_dedup(baseline);
    let snap_clean = strip_boundary_for_dedup(snapshot);

    // Fast path: identical content
    if base_clean == snap_clean {
        return false;
    }

    // Try structural comparison via components
    if let (Ok(snap_components), Ok(base_components)) = (
        component::parse(snapshot),
        component::parse(baseline),
    )
        && !snap_components.is_empty()
    {
        // Only check append-mode components — these grow monotonically and must
        // contain the snapshot's committed content. Replace-mode components
        // (status, pending) are user-editable and should be skipped.
        for snap_comp in &snap_components {
            let is_append = snap_comp.patch_mode()
                .map(|m| m == "append")
                .unwrap_or(is_append_mode_component(&snap_comp.name));
            if !is_append {
                continue;
            }
            let snap_content = strip_boundary_for_dedup(
                snap_comp.content(snapshot).trim(),
            );
            if snap_content.is_empty() {
                continue;
            }
            // Find matching component in baseline by name
            if let Some(base_comp) = base_components.iter().find(|c| c.name == snap_comp.name) {
                let base_content = strip_boundary_for_dedup(
                    base_comp.content(baseline).trim(),
                );
                // Baseline's append component must contain the snapshot's content
                if !base_content.contains(&snap_content) {
                    return true;
                }
            } else {
                // Snapshot has an append component that baseline lacks entirely
                return true;
            }
        }
        return false;
    }

    // Fallback for non-template docs: prefix check (original behavior)
    !base_clean.starts_with(&snap_clean)
}

/// Strip boundary markers for dedup comparison.
/// Boundary markers (`<!-- agent:boundary:XXXXXXXX -->`) get a fresh ID on each write,
/// so they must be excluded from content equality checks.
fn strip_boundary_for_dedup(content: &str) -> String {
    content.lines()
        .filter(|line| !line.trim().starts_with("<!-- agent:boundary:"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Log a write dedup event to both stderr and a persistent file for diagnosis.
fn log_dedup(file: &Path, context: &str) {
    let msg = format!("[write] dedup: {} — {}", file.display(), context);
    eprintln!("{}", msg);
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true).append(true)
        .open("/tmp/agent-doc-write-dedup.log")
    {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let bt = std::backtrace::Backtrace::force_capture();
        let _ = writeln!(f, "[{}] {} backtrace:\n{}", ts, msg, bt);
    }
}

/// Verify the current tmux pane owns the session for this document.
///
/// Returns `Ok(())` when the check passes or cannot be performed (not in tmux,
/// no session ID, session not registered, pane indeterminate). Returns `Err`
/// only when a *different* pane definitively owns the session.
fn verify_pane_ownership(file: &Path) -> Result<()> {
    if !sessions::in_tmux() {
        return Ok(());
    }
    let content = match std::fs::read_to_string(file) {
        Ok(c) => c,
        Err(_) => return Ok(()),
    };
    let session_id = match frontmatter::parse(&content) {
        Ok((fm, _)) => match fm.session {
            Some(s) => s,
            None => return Ok(()),
        },
        Err(_) => return Ok(()),
    };
    let entry = match sessions::lookup_entry(&session_id) {
        Ok(Some(e)) => e,
        _ => return Ok(()),
    };
    let current = match sessions::current_pane() {
        Ok(p) => p,
        Err(_) => return Ok(()),
    };
    if entry.pane != current {
        anyhow::bail!(
            "pane ownership mismatch: session {} owned by pane {}, current pane is {}. \
             Use `agent-doc claim` to reclaim.",
            session_id, entry.pane, current
        );
    }
    Ok(())
}

/// Run the write command: append assistant response to document.
///
/// `baseline` is the document content at the time the response was generated.
/// If omitted, the current document content is used (no merge needed).
pub fn run(file: &Path, baseline: Option<&str>) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    verify_pane_ownership(file)?;

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

    // Save pre-response snapshot for undo
    snapshot::save_pre_response(file, base)?;

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

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        drop(doc_lock);
        recover::clear_pending(file)?;
        return Ok(());
    }

    atomic_write(file, &final_content)?;

    // Save snapshot as content_ours (baseline + response), NOT final_content.
    // If the user edited during response generation, final_content includes their
    // edits via merge. Saving content_ours ensures the next diff detects those edits.
    snapshot::save(file, &content_ours)?;
    crate::ops_log::log_cycle(file, "write_inline", Some(&content_ours), Some(&final_content));
    crate::ops_log::log_op(file, &format!(
        "write_inline_done file={} snap_len={}",
        file.display(), content_ours.len()
    ));

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
    verify_pane_ownership(file)?;

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
    let (mut patches, unmatched) = template::parse_patches(&response)
        .context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    // Read document state
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let base = baseline.unwrap_or(&content_at_start);

    // Save pre-response snapshot for undo
    snapshot::save_pre_response(file, base)?;

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

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        drop(doc_lock);
        recover::clear_pending(file)?;
        return Ok(());
    }

    atomic_write(file, &final_content)?;

    // Save snapshot as content_ours (baseline + response), not final_content
    snapshot::save(file, &content_ours)?;
    crate::ops_log::log_cycle(file, "write_template", Some(&content_ours), Some(&final_content));
    crate::ops_log::log_op(file, &format!(
        "write_template_done file={} snap_len={} patches={}",
        file.display(), content_ours.len(), patches.len()
    ));

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
///
/// When `force_disk` is false and `.agent-doc/patches/` exists (plugin installed),
/// tries IPC first. On IPC timeout, leaves the patch file in place and exits
/// with code 75 (EX_TEMPFAIL) instead of falling back to disk write.
/// When `force_disk` is true, always uses direct disk write.
pub fn run_stream(file: &Path, baseline: Option<&str>, force_disk: bool) -> Result<()> {
    let t_total = std::time::Instant::now();

    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    verify_pane_ownership(file)?;

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
    let (mut patches, unmatched) = template::parse_patches(&response)
        .context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

    if patches.is_empty() && unmatched.trim().is_empty() {
        anyhow::bail!("no patch blocks or content found in response");
    }

    // Warn when patches target a file with no template components
    if patches.is_empty() && !unmatched.trim().is_empty() {
        let current = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        let comps = crate::component::parse(&current).unwrap_or_default();
        if comps.is_empty() {
            eprintln!(
                "[write] WARNING: {} bytes of content but file has no template components — \
                 content may not be applied correctly. Consider running `agent-doc init` \
                 with --mode template first.",
                unmatched.trim().len()
            );
        }
    }

    // Save pre-response snapshot for undo (before IPC or disk write)
    {
        let pre_content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {} for pre-response", file.display()))?;
        snapshot::save_pre_response(file, &pre_content)?;
    }

    // Try IPC when plugin is installed and --force-disk is not set
    if !force_disk {
        let canonical = file.canonicalize()?;
        let project_root = find_project_root(&canonical)
            .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
        let patches_dir = project_root.join(".agent-doc/patches");

        if patches_dir.exists() {
            // Compute content_ours (baseline + patches) for snapshot saving.
            // The IPC path sends patches to the plugin but we need a clean snapshot
            // that represents baseline+response WITHOUT user's concurrent edits.
            let content_at_start = std::fs::read_to_string(file)
                .with_context(|| format!("failed to read {}", file.display()))?;
            let base = baseline.unwrap_or(&content_at_start);
            let mode_overrides = std::collections::HashMap::new();
            let t_apply = std::time::Instant::now();
            let mut content_ours = template::apply_patches_with_overrides(
                base, &patches, &unmatched, file, &mode_overrides,
            ).context("failed to apply patches for snapshot")?;
            let elapsed_apply = t_apply.elapsed().as_millis();
            if elapsed_apply > 0 {
                eprintln!("[perf] apply_patches_with_overrides: {}ms", elapsed_apply);
            }

            // Guard: detect stale baseline by structural component comparison.
            // A baseline is stale when it's MISSING committed content from the snapshot
            // (e.g., a previous response was committed but the baseline predates it).
            // A baseline with EXTRA content beyond the snapshot is normal (user edits).
            //
            // IMPORTANT: Skip this check when an explicit baseline was provided via
            // --baseline-file. Streaming checkpoints intentionally use the original
            // document (before any response) as baseline so cumulative patch blocks
            // apply cleanly on each checkpoint. The snapshot will have content from
            // earlier checkpoints, causing is_stale_baseline to incorrectly fire and
            // apply patches on top of content_at_start (which already has earlier
            // checkpoint content) → duplicate response content.
            //
            // Compare component-by-component: for each component in the snapshot, check
            // that the baseline's corresponding component contains the snapshot content.
            // This handles user edits anywhere in the document (not just appended at end).
            if baseline.is_none()
                && let Ok(Some(current_snap)) = snapshot::load(file)
                && is_stale_baseline(base, &current_snap)
            {
                eprintln!(
                    "[write] WARNING: baseline missing snapshot content — stale baseline detected, using current file as baseline"
                );
                crate::ops_log::log_op(file, &format!(
                    "stale_baseline_detected file={} base_len={} snap_len={} file_len={}",
                    file.display(), base.len(), current_snap.len(), content_at_start.len()
                ));
                // Re-apply patches to the current file content instead of the stale baseline
                content_ours = template::apply_patches_with_overrides(
                    &content_at_start, &patches, &unmatched, file, &mode_overrides,
                ).context("failed to apply patches with fresh baseline")?;
            }

            // Normalize user input in exchange: add ❯  prefix to user-added lines.
            // Uses the snapshot (loaded above) to identify new lines.
            // Compute normalization targets for the IPC plugin so the editor also shows
            // the prefix immediately (not just the snapshot).
            let normalize_prefix_lines: Vec<String> =
                if let Ok(Some(ref snap)) = snapshot::load(file) {
                    let before = content_ours.clone();
                    content_ours = normalize_user_prompts_in_exchange(&content_ours, base, snap);
                    extract_normalization_targets(&before, &content_ours)
                } else {
                    vec![]
                };

            // Dedup: skip IPC if patches produce no changes (strip boundary markers)
            if strip_boundary_for_dedup(&content_ours) == strip_boundary_for_dedup(&content_at_start) {
                log_dedup(file, "no changes after merge, skipping write");
                recover::clear_pending(file)?;
                return Ok(());
            }

            // Plugin is installed — try IPC
            let t_ipc = std::time::Instant::now();
            let norm_lines_opt = if normalize_prefix_lines.is_empty() { None } else { Some(normalize_prefix_lines.as_slice()) };
            if try_ipc(file, &patches, &unmatched, None, baseline, Some(&content_ours), norm_lines_opt)? {
                let elapsed_ipc = t_ipc.elapsed().as_millis();
                if elapsed_ipc > 0 {
                    eprintln!("[perf] try_ipc: {}ms", elapsed_ipc);
                }
                let elapsed_total = t_total.elapsed().as_millis();
                if elapsed_total > 0 {
                    eprintln!("[perf] run_stream total: {}ms", elapsed_total);
                }
                // IPC succeeded — plugin applied patches
                crate::ops_log::log_op(file, &format!(
                    "ipc_write_consumed file={} patches={}",
                    file.display(), patches.len()
                ));
                // Fire post_write hook for cross-session coordination
                let session_id = frontmatter::read_session_id(file).unwrap_or_default();
                crate::hooks::fire_post_write(file, &session_id, patches.len());
                recover::clear_pending(file)?;
                return Ok(());
            }
            // IPC timeout — patch file was already cleaned up by try_ipc,
            // but we want to leave a NEW patch file in place for the plugin
            // to pick up later. Re-write it.
            let hash = snapshot::doc_hash(file)?;
            let patch_file = patches_dir.join(format!("{}.json", hash));

            // Read current document and reposition boundary (same as primary IPC path)
            let raw_doc = std::fs::read_to_string(file).unwrap_or_default();
            let current_doc_for_boundary = template::reposition_boundary_to_end_with_summary(&raw_doc, file.file_stem().and_then(|s| s.to_str()));

            let ipc_patches: Vec<serde_json::Value> = patches
                .iter()
                .filter(|p| p.name != "frontmatter")
                .map(|p| {
                    let mut patch_json = serde_json::json!({
                        "component": p.name,
                        "content": p.content,
                    });
                    if let Some(bid) = find_boundary_id(&current_doc_for_boundary, &p.name) {
                        patch_json["boundary_id"] = serde_json::Value::String(bid);
                    } else if is_append_mode_component(&p.name) {
                        patch_json["ensure_boundary"] = serde_json::Value::Bool(true);
                    }
                    patch_json
                })
                .collect();

            let mut ipc_payload = serde_json::json!({
                "file": canonical.to_string_lossy(),
                "patches": ipc_patches,
                "unmatched": unmatched.trim(),
                "baseline": baseline.unwrap_or(""),
            });

            // Include frontmatter if present
            let frontmatter_yaml: Option<String> = patches
                .iter()
                .find(|p| p.name == "frontmatter")
                .map(|p| p.content.trim().to_string());
            if let Some(ref yaml) = frontmatter_yaml {
                ipc_payload["frontmatter"] = serde_json::Value::String(yaml.clone());
            }

            atomic_write(
                &patch_file,
                &serde_json::to_string_pretty(&ipc_payload)?,
            )?;

            eprintln!("[write] IPC timeout — response saved as patch, awaiting plugin");
            std::process::exit(75); // EX_TEMPFAIL
        }
    }

    // No plugin installed or --force-disk — direct disk write
    // When --force-disk is set, clean up any pending IPC patch files to prevent
    // the plugin from applying them later (which would cause double-write).
    if force_disk
        && let Ok(canonical) = file.canonicalize() {
            let project_root = find_project_root(&canonical)
                .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());
            let patches_dir = project_root.join(".agent-doc/patches");
            if let Ok(hash) = snapshot::doc_hash(file) {
                let patch_file = patches_dir.join(format!("{}.json", hash));
                if patch_file.exists() {
                    eprintln!("[write] cleaning stale IPC patch file to prevent double-write");
                    let _ = std::fs::remove_file(&patch_file);
                }
            }
        }
    let t_disk = std::time::Instant::now();

    // Read document state
    let content_at_start = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let base = baseline.unwrap_or(&content_at_start);

    // Apply patches using the mode resolution chain:
    // inline attr (patch=append on tag) > components.toml > built-in default.
    // The skill sends delta content for append-mode components.
    let mode_overrides = std::collections::HashMap::new();
    let t_apply2 = std::time::Instant::now();
    let mut content_ours = template::apply_patches_with_overrides(
        base, &patches, &unmatched, file, &mode_overrides,
    ).context("failed to apply template patches")?;
    let elapsed_apply2 = t_apply2.elapsed().as_millis();
    if elapsed_apply2 > 0 {
        eprintln!("[perf] apply_patches_with_overrides (disk): {}ms", elapsed_apply2);
    }

    // Apply frontmatter patch if present (fixes #16 — disk write path was missing this)
    if let Some(fm_patch) = patches.iter().find(|p| p.name == "frontmatter") {
        content_ours = crate::frontmatter::merge_fields(&content_ours, &fm_patch.content)
            .context("failed to merge frontmatter patch")?;
    }

    // Normalize user input in exchange: add ❯  prefix to user-added lines.
    // Load snapshot to identify which lines are new (user-typed this cycle).
    if let Ok(Some(snap)) = snapshot::load(file) {
        content_ours = normalize_user_prompts_in_exchange(&content_ours, base, &snap);
    }

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
        // Use baseline as CRDT base instead of stored state from previous cycle.
        // The baseline is the exact content both sides (ours and theirs) diverged
        // from, giving clean diffs. Using a stale stored state causes character-level
        // interleaving when the agent replaces component content while the user
        // appends within the same region (lazily-rs.md corruption bug).
        let base_state = crate::crdt::CrdtDoc::from_text(base).encode_state();
        // Agent=client_id(2) gives native correct ordering — no skip_reorder needed.
        merge::merge_contents_crdt(Some(&base_state), &content_ours, &content_current)?
    };

    // Dedup: skip write if merged content is identical to current file (strip boundary markers)
    if strip_boundary_for_dedup(&final_content) == strip_boundary_for_dedup(&content_current) {
        log_dedup(file, "no changes after merge, skipping write");
        drop(doc_lock);
        recover::clear_pending(file)?;
        let elapsed_total = t_total.elapsed().as_millis();
        if elapsed_total > 0 {
            eprintln!("[perf] run_stream total: {}ms", elapsed_total);
        }
        return Ok(());
    }

    atomic_write(file, &final_content)?;

    // Save snapshot as content_ours (baseline + response), not final_content.
    // If the user edited concurrently, final_content includes their edits via CRDT merge.
    // Saving content_ours ensures the next diff detects those concurrent edits.
    snapshot::save(file, &content_ours)?;
    // Save the merged CRDT state — NOT a fresh state from content_ours.
    // Using content_ours would lose user edits from the merge, causing
    // the next merge cycle to re-insert them as duplicates.
    snapshot::save_crdt(file, &crdt_state)?;
    crate::ops_log::log_cycle(file, "write_stream", Some(&content_ours), Some(&final_content));
    crate::ops_log::log_op(file, &format!(
        "write_stream_done file={} snap_len={}",
        file.display(), content_ours.len()
    ));

    drop(doc_lock);

    // Clear pending response after successful write
    recover::clear_pending(file)?;

    let elapsed_disk = t_disk.elapsed().as_millis();
    if elapsed_disk > 0 {
        eprintln!("[perf] disk_write_path: {}ms", elapsed_disk);
    }
    let elapsed_total = t_total.elapsed().as_millis();
    if elapsed_total > 0 {
        eprintln!("[perf] run_stream total: {}ms", elapsed_total);
    }

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
    let (mut patches, unmatched) = template::parse_patches(&response)
        .context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

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

    // Read current document and reposition boundary to end of exchange.
    // This matches the pre-patch step in template::apply_patches_with_overrides():
    // remove stale boundaries, insert fresh one at end. Without this, the IPC
    // path would use the old boundary position (above the user's new prompt),
    // causing responses to appear before the prompt instead of after.
    let raw_doc = std::fs::read_to_string(file).unwrap_or_default();
    let current_doc_for_boundary = template::reposition_boundary_to_end_with_summary(&raw_doc, file.file_stem().and_then(|s| s.to_str()));

    // Separate frontmatter patch from component patches
    let mut frontmatter_yaml: Option<String> = None;
    let ipc_patches: Vec<serde_json::Value> = patches
        .iter()
        .filter_map(|p| {
            if p.name == "frontmatter" {
                frontmatter_yaml = Some(p.content.trim().to_string());
                None
            } else {
                let mut patch_json = serde_json::json!({
                    "component": p.name,
                    "content": p.content,
                });
                if let Some(bid) = find_boundary_id(&current_doc_for_boundary, &p.name) {
                    patch_json["boundary_id"] = serde_json::Value::String(bid);
                } else if is_append_mode_component(&p.name) {
                    patch_json["ensure_boundary"] = serde_json::Value::Bool(true);
                }
                Some(patch_json)
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

    let (mut patches, unmatched) = template::parse_patches(response)
        .context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

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

    let (mut patches, unmatched) = template::parse_patches(response)
        .context("failed to parse patch blocks from response")?;

    // Sanitize component tags in patch content to prevent parser corruption
    sanitize_patches(&mut patches);

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

/// Attempt to write via IPC (socket-first, file-based fallback).
///
/// First tries socket IPC via `ipc_socket::send_message()` for lowest latency.
/// Falls back to file-based IPC (JSON patch in `.agent-doc/patches/`) if socket
/// is unavailable. Returns `Ok(true)` if either path succeeded, `Ok(false)` if
/// no plugin is active.
pub fn try_ipc(
    file: &Path,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
    frontmatter_yaml: Option<&str>,
    baseline: Option<&str>,
    content_ours: Option<&str>,
    normalize_prefix_lines: Option<&[String]>,
) -> Result<bool> {
    let canonical = file.canonicalize()?;
    let hash = snapshot::doc_hash(file)?;
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());

    // Try socket IPC first (lower latency, no inotify)
    if crate::ipc_socket::is_listener_active(&project_root) {
        // Clean up any stale patch file from a previous timeout before socket send.
        // Without this, the file watcher could pick up and apply the stale file
        // concurrently with the socket delivery, causing double-apply.
        let patches_dir_for_socket = project_root.join(".agent-doc/patches");
        if patches_dir_for_socket.exists() {
            let stale_patch_file = patches_dir_for_socket.join(format!("{}.json", hash));
            if stale_patch_file.exists() {
                eprintln!("[write] cleaning stale patch file before socket send (prevent double-apply)");
                if let Err(e) = std::fs::remove_file(&stale_patch_file) {
                    eprintln!("[write] WARNING: failed to clean stale patch file: {}", e);
                }
            }
        }
        let ipc_patches_json = build_ipc_patches_json(file, patches, unmatched)?;
        // When unmatched content was synthesized into a patch (no explicit patch blocks),
        // don't also send it as "unmatched" — the plugin would apply both and duplicate.
        let effective_unmatched_socket = if patches.is_empty() && !ipc_patches_json.is_empty() {
            eprintln!("[write] synthesis consumed unmatched content — clearing from socket payload (prevent double-apply)");
            ""
        } else {
            unmatched.trim()
        };
        let mut socket_payload = serde_json::json!({
            "type": "patch",
            "file": canonical.to_string_lossy(),
            "patches": ipc_patches_json,
            "unmatched": effective_unmatched_socket,
            "baseline": baseline.unwrap_or(""),
            "reposition_boundary": true,
        });
        if let Some(yaml) = frontmatter_yaml {
            socket_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
        }
        if let Some(lines) = normalize_prefix_lines
            && !lines.is_empty()
        {
            socket_payload["normalize_prefix_lines"] = serde_json::Value::Array(
                lines.iter().map(|l| serde_json::Value::String(l.clone())).collect()
            );
            // Include full normalized content ONLY when there are no component patches.
            // When patches are present, the plugin applies normalize_prefix_lines before
            // component patches — fullContent would conflict by replacing the document
            // before patches run, causing duplicates on the next cycle.
            // fullContent is only safe as a fallback for append-mode (no-component) docs.
            if ipc_patches_json.is_empty() && let Some(ours) = content_ours {
                socket_payload["fullContent"] = serde_json::Value::String(ours.to_string());
            }
        }
        match crate::ipc_socket::send_message(&project_root, &socket_payload) {
            Ok(Some(_ack)) => {
                eprintln!("[write] socket IPC patch delivered");
                // Save snapshot — use content_ours (baseline + response) when available.
                // Bug 2A fix: snapshot save failure after IPC success is non-fatal.
                // The plugin already has the correct content; the snapshot can be
                // recovered by commit's divergence detection (Bug 2B fix).
                let snap_content = if let Some(ours) = content_ours {
                    ours.to_string()
                } else {
                    std::fs::read_to_string(file)
                        .with_context(|| format!("failed to read {} after socket IPC", file.display()))?
                };
                if let Err(e) = snapshot::save(file, &snap_content) {
                    eprintln!(
                        "[write] WARNING: IPC write succeeded but snapshot save failed: {}. \
                         Commit will auto-recover via divergence detection.",
                        e
                    );
                    crate::ops_log::log_op(file, &format!(
                        "snapshot_save_failed_after_ipc file={} error={}",
                        file.display(), e
                    ));
                } else {
                    let crdt_doc = crate::crdt::CrdtDoc::from_text(&snap_content);
                    if let Err(e) = snapshot::save_crdt(file, &crdt_doc.encode_state()) {
                        eprintln!("[write] WARNING: CRDT state save failed: {}", e);
                    }
                }
                return Ok(true);
            }
            Ok(None) => {
                eprintln!("[write] socket IPC sent but no ack — falling back to file IPC");
            }
            Err(e) => {
                eprintln!("[write] socket IPC failed: {} — falling back to file IPC", e);
            }
        }
    }

    let patches_dir = project_root.join(".agent-doc/patches");

    // Only attempt file-based IPC if the patches directory exists (plugin has started)
    if !patches_dir.exists() {
        return Ok(false);
    }

    let patch_file = patches_dir.join(format!("{}.json", hash));

    // Build patches using shared helper (same logic as socket path)
    let ipc_patches = build_ipc_patches_json(file, patches, unmatched)?;

    // Same dedup guard as socket path: don't send unmatched when it was synthesized into a patch.
    let effective_unmatched_file = if patches.is_empty() && !ipc_patches.is_empty() {
        ""
    } else {
        unmatched.trim()
    };

    let mut ipc_payload = serde_json::json!({
        "file": canonical.to_string_lossy(),
        "patches": ipc_patches,
        "unmatched": effective_unmatched_file,
        "baseline": baseline.unwrap_or(""),
        "reposition_boundary": true,
    });

    if let Some(yaml) = frontmatter_yaml {
        ipc_payload["frontmatter"] = serde_json::Value::String(yaml.to_string());
    }
    if let Some(lines) = normalize_prefix_lines
        && !lines.is_empty()
    {
        ipc_payload["normalize_prefix_lines"] = serde_json::Value::Array(
            lines.iter().map(|l| serde_json::Value::String(l.clone())).collect()
        );
        // Include full normalized content ONLY when there are no component patches.
        // When patches are present, normalize_prefix_lines + patches apply correctly
        // without fullContent. Sending fullContent alongside patches causes the plugin
        // to apply fullContent (full replacement) and skip patches → duplicate on next cycle.
        if ipc_patches.is_empty() && let Some(ours) = content_ours {
            ipc_payload["fullContent"] = serde_json::Value::String(ours.to_string());
        }
    }

    // Log IPC write details for debugging cross-contamination
    crate::ops_log::log_op(file, &format!(
        "ipc_write_attempt file={} hash={} patches={} ipc_patches={} unmatched_len={}",
        file.display(), hash, patches.len(), ipc_patches.len(), unmatched.trim().len()
    ));

    // Warn when unmatched content exists but no IPC patches were synthesized —
    // this means content will be silently dropped by the plugin
    if ipc_patches.is_empty() && !unmatched.trim().is_empty() {
        eprintln!(
            "[write] WARNING: {} bytes of unmatched content with no IPC patches — content will be dropped. \
             Does the target file have template components (<!-- agent:exchange -->)?",
            unmatched.trim().len()
        );
        crate::ops_log::log_op(file, &format!(
            "ipc_unmatched_content_dropped file={} unmatched_len={}",
            file.display(), unmatched.trim().len()
        ));
    }

    write_ipc_and_poll(&patch_file, &ipc_payload, file, patches.len(), content_ours)
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
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());

    // Try socket IPC first
    if crate::ipc_socket::is_listener_active(&project_root) {
        let socket_payload = serde_json::json!({
            "type": "patch",
            "file": canonical.to_string_lossy(),
            "patches": [],
            "unmatched": "",
            "fullContent": content,
        });
        match crate::ipc_socket::send_message(&project_root, &socket_payload) {
            Ok(Some(_ack)) => {
                eprintln!("[write] socket IPC full content delivered");
                snapshot::save(file, content)?;
                let crdt_doc = crate::crdt::CrdtDoc::from_text(content);
                snapshot::save_crdt(file, &crdt_doc.encode_state())?;
                return Ok(true);
            }
            Ok(None) => {
                eprintln!("[write] socket IPC full content sent but no ack — falling back to file IPC");
            }
            Err(e) => {
                eprintln!("[write] socket IPC full content failed: {} — falling back to file IPC", e);
            }
        }
    }

    let hash = snapshot::doc_hash(file)?;
    let patches_dir = project_root.join(".agent-doc/patches");

    // Only attempt file-based IPC if the patches directory exists (plugin has started)
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

    write_ipc_and_poll(&patch_file, &ipc_payload, file, 0, Some(content))
}

/// Send a reposition-only IPC signal to the plugin.
///
/// No content changes — just tells the plugin to move the boundary marker
/// to the end of the exchange component. Used by `commit()` to keep the
/// boundary at end-of-exchange without writing to the working tree
/// (which would cause keystroke loss if the user is typing).
///
/// Returns `true` if the plugin consumed the signal, `false` on timeout
/// or if no plugin is active.
pub fn try_ipc_reposition_boundary(file: &Path) -> bool {
    let canonical = match file.canonicalize() {
        Ok(c) => c,
        Err(_) => return false,
    };
    let project_root = find_project_root(&canonical)
        .unwrap_or_else(|| canonical.parent().unwrap_or(Path::new(".")).to_path_buf());

    if !crate::ipc_socket::is_listener_active(&project_root) {
        return false;
    }

    match crate::ipc_socket::send_reposition(&project_root, &canonical.to_string_lossy()) {
        Ok(true) => {
            eprintln!("[commit] IPC reposition boundary signal sent");
            true
        }
        Ok(false) => {
            eprintln!("[commit] IPC reposition: no ack (non-fatal)");
            false
        }
        Err(e) => {
            eprintln!("[commit] IPC reposition failed (non-fatal): {}", e);
            false
        }
    }
}

/// Write an IPC patch file and poll for plugin ACK (file deletion).
///
/// Returns `Ok(true)` if consumed, `Ok(false)` on timeout.
fn write_ipc_and_poll(
    patch_file: &Path,
    payload: &serde_json::Value,
    doc_file: &Path,
    patch_count: usize,
    content_ours: Option<&str>,
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
            // Plugin consumed the patch — verify it was actually applied.
            // Wait briefly for the plugin's Document API write to flush to disk,
            // then check that the file has changed from the baseline.
            std::thread::sleep(std::time::Duration::from_millis(200));
            let current_on_disk = std::fs::read_to_string(doc_file).unwrap_or_default();
            let baseline_content = payload.get("baseline")
                .and_then(|v| v.as_str())
                .unwrap_or("");

            if !baseline_content.is_empty() && current_on_disk == baseline_content {
                // File on disk hasn't changed — plugin likely failed to apply the patch.
                // Don't save snapshot with content that was never applied.
                eprintln!(
                    "[write] IPC patch consumed but file unchanged on disk — plugin may have failed to apply. Falling back to disk write."
                );
                return Ok(false);
            }

            // Verify patch content is present in the file (catches partial application).
            // Check that at least one non-empty patch's content appears in the result.
            let patch_list = payload.get("patches")
                .and_then(|v| v.as_array());
            if let Some(patches) = patch_list {
                let has_content_patch = patches.iter().any(|p| {
                    let content = p.get("content").and_then(|c| c.as_str()).unwrap_or("");
                    !content.trim().is_empty()
                });
                if has_content_patch {
                    let any_present = patches.iter().any(|p| {
                        let content = p.get("content").and_then(|c| c.as_str()).unwrap_or("");
                        if content.trim().is_empty() { return true; }
                        // Check first meaningful line of content appears in file
                        content.lines()
                            .find(|l| !l.trim().is_empty())
                            .is_none_or(|first_line| current_on_disk.contains(first_line.trim()))
                    });
                    if !any_present {
                        eprintln!(
                            "[write] IPC patch consumed but response content not found in file — plugin may have partially failed. Falling back to disk write."
                        );
                        return Ok(false);
                    }
                }
            }

            // Plugin applied the patch — update snapshot.
            // Use content_ours (baseline + response) when available, NOT the current
            // file. The current file may include user edits typed after the boundary,
            // which would be absorbed into the snapshot and lost to the next diff.
            // Bug 2A fix: snapshot save failure after IPC success is non-fatal.
            let snap_content = if let Some(ours) = content_ours {
                ours.to_string()
            } else {
                std::fs::read_to_string(doc_file)
                    .with_context(|| format!("failed to read {} after IPC", doc_file.display()))?
            };
            if let Err(e) = snapshot::save(doc_file, &snap_content) {
                eprintln!(
                    "[write] WARNING: IPC write succeeded but snapshot save failed: {}. \
                     Commit will auto-recover via divergence detection.",
                    e
                );
                crate::ops_log::log_op(doc_file, &format!(
                    "snapshot_save_failed_after_ipc file={} error={}",
                    doc_file.display(), e
                ));
            } else {
                let crdt_doc = crate::crdt::CrdtDoc::from_text(&snap_content);
                if let Err(e) = snapshot::save_crdt(doc_file, &crdt_doc.encode_state()) {
                    eprintln!("[write] WARNING: CRDT state save failed: {}", e);
                }
                eprintln!("[write] IPC patch consumed by plugin — snapshot updated");
            }
            return Ok(true);
        }
        std::thread::sleep(poll_interval);
    }

    // Timeout — clean up unconsumed patch file
    eprintln!("[write] IPC timeout ({}s) — falling back to direct write", timeout.as_secs());
    let _ = std::fs::remove_file(patch_file);
    Ok(false)
}

/// Build the IPC patches JSON array (shared between socket and file-based paths).
///
/// Reads the document to find boundary IDs, filters frontmatter patches,
/// synthesizes exchange patches for unmatched content.
fn build_ipc_patches_json(
    file: &Path,
    patches: &[crate::template::PatchBlock],
    unmatched: &str,
) -> Result<Vec<serde_json::Value>> {
    let raw_doc = std::fs::read_to_string(file).unwrap_or_default();
    let current_doc = template::reposition_boundary_to_end_with_summary(
        &raw_doc,
        file.file_stem().and_then(|s| s.to_str()),
    );

    let mut ipc_patches: Vec<serde_json::Value> = patches
        .iter()
        .filter(|p| p.name != "frontmatter")
        .map(|p| {
            let mut patch_json = serde_json::json!({
                "component": p.name,
                "content": p.content,
            });
            if let Some(bid) = find_boundary_id(&current_doc, &p.name) {
                patch_json["boundary_id"] = serde_json::Value::String(bid);
            } else if is_append_mode_component(&p.name) {
                patch_json["ensure_boundary"] = serde_json::Value::Bool(true);
            }
            patch_json
        })
        .collect();

    let effective_unmatched = unmatched.trim().to_string();
    if ipc_patches.is_empty() && !effective_unmatched.is_empty() {
        // Dedup guard: parse components once, check before synthesizing.
        let parsed_comps = crate::component::parse(&current_doc).unwrap_or_default();
        for target in &["exchange", "output"] {
            // Skip synthesis if the content already exists in the target component.
            // This makes the write idempotent even when called twice with the same content.
            let already_present = parsed_comps.iter().any(|c| {
                c.name == *target && {
                    let body = &current_doc[c.open_end..c.close_start];
                    body.contains(effective_unmatched.as_str())
                }
            });
            if already_present {
                eprintln!(
                    "[write] dedup: content already present in {} — skipping synthesis",
                    target
                );
                break;
            }
            if let Some(bid) = find_boundary_id(&current_doc, target) {
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (boundary {})",
                    target, &bid[..8.min(bid.len())]
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": &effective_unmatched,
                    "boundary_id": bid,
                }));
                break;
            } else if is_append_mode_component(target) {
                eprintln!(
                    "[write] synthesizing {} patch for unmatched content (ensure_boundary)",
                    target
                );
                ipc_patches.push(serde_json::json!({
                    "component": target,
                    "content": &effective_unmatched,
                    "ensure_boundary": true,
                }));
                break;
            }
        }
    }

    Ok(ipc_patches)
}

// ---------------------------------------------------------------------------
// Internal helpers (same patterns as submit.rs)
// ---------------------------------------------------------------------------

/// Sanitize component tags in patch block content to prevent parser corruption.
///
/// When an agent response mentions component tags like `<!-- agent:NAME -->` in its
/// text, those raw HTML comments would be matched as real markers on subsequent
/// operations (compact, write). This escapes them to `&lt;!-- agent:NAME --&gt;`
/// so the component parser won't match them.
///
/// Only sanitizes `<!-- agent:NAME -->` and `<!-- /agent:NAME -->` patterns where
/// NAME is a valid component name (`[a-zA-Z0-9][a-zA-Z0-9-]*`).
pub fn sanitize_component_tags(content: &str) -> String {
    let bytes = content.as_bytes();
    let len = bytes.len();
    let mut result = String::with_capacity(len);
    let mut pos = 0;

    while pos + 4 <= len {
        if &bytes[pos..pos + 4] != b"<!--" {
            // Advance by one UTF-8 character (not one byte) to preserve multi-byte sequences
            let ch_len = utf8_char_len(bytes[pos]);
            result.push_str(&content[pos..pos + ch_len]);
            pos += ch_len;
            continue;
        }

        // Find closing -->
        let close = match find_comment_close(bytes, pos + 4) {
            Some(c) => c, // position after -->
            None => {
                result.push_str("<!--");
                pos += 4;
                continue;
            }
        };

        let inner = &content[pos + 4..close - 3];
        let trimmed = inner.trim();

        if component::is_agent_marker(trimmed) {
            // Escape the entire comment: <!-- ... --> -> &lt;!-- ... --&gt;
            let original = &content[pos..close];
            result.push_str(&original.replace('<', "&lt;").replace('>', "&gt;"));
        } else {
            // Not an agent marker — keep as-is
            result.push_str(&content[pos..close]);
        }
        pos = close;
    }

    // Append remaining content (as a str slice to preserve UTF-8)
    if pos < len {
        result.push_str(&content[pos..]);
    }

    result
}

/// Return the byte length of the UTF-8 character starting with `first_byte`.
fn utf8_char_len(first_byte: u8) -> usize {
    match first_byte {
        0x00..=0x7F => 1,
        0xC0..=0xDF => 2,
        0xE0..=0xEF => 3,
        0xF0..=0xFF => 4,
        _ => 1, // continuation byte — shouldn't happen at a char boundary
    }
}

/// Find the end of an HTML comment (position after `-->`), starting search from `start`.
fn find_comment_close(bytes: &[u8], start: usize) -> Option<usize> {
    let len = bytes.len();
    let mut i = start;
    while i + 3 <= len {
        if &bytes[i..i + 3] == b"-->" {
            return Some(i + 3);
        }
        i += 1;
    }
    None
}

/// Sanitize the content of each patch block in-place.
fn sanitize_patches(patches: &mut [template::PatchBlock]) {
    for patch in patches.iter_mut() {
        patch.content = sanitize_component_tags(&patch.content);
    }
}

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
        let result = try_ipc(&doc, &patches, "", None, None, None, None).unwrap();
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

        let patch = crate::template::PatchBlock::new("exchange", "new content");

        // This will timeout after 2s — patch file is written but never consumed
        let result = try_ipc(&doc, &[patch], "", None, None, None, None).unwrap();
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

        let patch = crate::template::PatchBlock::new("exchange", "new content");

        // Spawn "plugin" thread that watches for patch files, writes content, then deletes
        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            // Simulate plugin applying the patch by modifying the doc
                            let _ = fs::write(&doc_for_watcher,
                                "---\nsession: test\n---\n\n<!-- agent:exchange -->\nnew content\n<!-- /agent:exchange -->\n");
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let result = try_ipc(&doc, &[patch], "", None, None, None, None).unwrap();
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

    // --- sanitize_component_tags tests ---

    #[test]
    fn sanitize_escapes_open_agent_tag() {
        let input = "Here is an example: <!-- agent:exchange --> marker.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "open agent tag should be escaped, got: {}",
            result
        );
        assert!(
            !result.contains("<!-- agent:exchange -->"),
            "raw open agent tag should not remain"
        );
    }

    #[test]
    fn sanitize_escapes_close_agent_tag() {
        let input = "End marker: <!-- /agent:pending --> done.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("&lt;!-- /agent:pending --&gt;"),
            "close agent tag should be escaped, got: {}",
            result
        );
        assert!(
            !result.contains("<!-- /agent:pending -->"),
            "raw close agent tag should not remain"
        );
    }

    #[test]
    fn sanitize_does_not_escape_patch_markers() {
        let input = "<!-- patch:exchange -->\nsome content\n<!-- /patch:exchange -->\n";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "patch markers must not be escaped");
    }

    #[test]
    fn sanitize_passes_normal_content_through() {
        let input = "Just some normal markdown content.\n\nWith paragraphs and **bold**.";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "normal content should pass through unchanged");
    }

    #[test]
    fn sanitize_preserves_utf8_em_dash() {
        // Em dash U+2014 is 3 bytes in UTF-8: 0xE2, 0x80, 0x94
        let input = "This is a test \u{2014} with em dashes \u{2014} in content.";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "em dashes must survive sanitization unchanged");

        // Verify at the byte level
        assert_eq!(
            result.as_bytes(),
            input.as_bytes(),
            "byte-level content must be identical"
        );
    }

    #[test]
    fn sanitize_preserves_mixed_utf8_and_agent_tags() {
        // Content with UTF-8 characters AND agent tags that need escaping
        let input = "Response with \u{2014} em dash and <!-- agent:exchange --> tag reference.";
        let result = sanitize_component_tags(input);
        assert!(
            result.contains("\u{2014}"),
            "em dash must be preserved, got: {:?}",
            result
        );
        assert!(
            result.contains("&lt;!-- agent:exchange --&gt;"),
            "agent tag must be escaped"
        );
    }

    #[test]
    fn sanitize_preserves_various_unicode() {
        // Test various multi-byte UTF-8 characters
        let input = "Caf\u{00E9} \u{2019}quotes\u{2019} \u{2014} \u{2026} \u{1F600}";
        let result = sanitize_component_tags(input);
        assert_eq!(result, input, "all unicode must survive sanitization");
    }

    #[test]
    fn try_ipc_snapshot_saves_content_ours() {
        // Verify that after IPC succeeds, the snapshot contains content_ours
        // (baseline + response), NOT whatever is currently in the working tree file.
        // This is critical: if we snapshot the file on disk, user edits typed after
        // the boundary would be absorbed and lost to the next diff.
        let dir = TempDir::new().unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        fs::create_dir_all(agent_doc_dir.join("patches")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();

        let doc = dir.path().join("test.md");
        let original = "---\nsession: test\n---\n\n<!-- agent:exchange -->\noriginal content\n<!-- agent:boundary:test-boundary-123 -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, original).unwrap();

        let patch = crate::template::PatchBlock::new("exchange", "agent response content");

        // content_ours = baseline with patches applied (what the snapshot should contain)
        let content_ours = "---\nsession: test\n---\n\n<!-- agent:exchange -->\nagent response content\n<!-- /agent:exchange -->\n";

        // Simulate user editing the file AFTER write began (working tree differs from content_ours)
        let user_edited = "---\nsession: test\n---\n\n<!-- agent:exchange -->\noriginal content\nuser typed something new\n<!-- agent:boundary:test-boundary-123 -->\n<!-- /agent:exchange -->\n";
        fs::write(&doc, user_edited).unwrap();

        // Spawn "plugin" thread that watches for patch files, writes content, then deletes
        let patches_dir = agent_doc_dir.join("patches");
        let watcher_dir = patches_dir.clone();
        let doc_for_watcher = doc.clone();
        let _watcher = std::thread::spawn(move || {
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(50));
                if let Ok(entries) = fs::read_dir(&watcher_dir) {
                    for entry in entries.flatten() {
                        if entry.path().extension().is_some_and(|e| e == "json") {
                            // Simulate plugin applying patch + user edits
                            let _ = fs::write(&doc_for_watcher,
                                "---\nsession: test\n---\n\n<!-- agent:exchange -->\nagent response content\nuser typed something new\n<!-- /agent:exchange -->\n");
                            let _ = fs::remove_file(entry.path());
                            return;
                        }
                    }
                }
            }
        });

        let result = try_ipc(
            &doc,
            &[patch],
            "",
            None,
            Some(original),     // baseline
            Some(content_ours), // content_ours — what snapshot should save
            None,               // normalize_prefix_lines
        )
        .unwrap();
        assert!(result, "IPC should succeed when plugin consumes patch");

        // KEY ASSERTION: snapshot must contain content_ours, not the working tree file
        let snap = snapshot::load(&doc).unwrap().unwrap();
        assert!(
            snap.contains("agent response content"),
            "snapshot must contain content_ours (agent response), got: {}",
            snap
        );
        assert!(
            !snap.contains("user typed something new"),
            "snapshot must NOT contain working tree edits — \
             it should save content_ours, not the current file"
        );
        assert_eq!(
            snap, content_ours,
            "snapshot must exactly match content_ours"
        );

        // Working tree file should still have the user's edits (untouched by IPC snapshot)
        let on_disk = fs::read_to_string(&doc).unwrap();
        assert!(
            on_disk.contains("user typed something new"),
            "working tree file should still contain user edits"
        );
    }

    #[test]
    fn ipc_json_preserves_utf8_em_dash() {
        // Verify that serde_json serialization preserves em dashes in IPC payloads
        let content = "Response with \u{2014} em dash.";
        let payload = serde_json::json!({
            "file": "/tmp/test.md",
            "patches": [{
                "component": "exchange",
                "content": content,
            }],
            "unmatched": "",
            "baseline": "",
        });

        let json_str = serde_json::to_string_pretty(&payload).unwrap();
        // Parse it back and verify the content is preserved
        let parsed: serde_json::Value = serde_json::from_str(&json_str).unwrap();
        let parsed_content = parsed["patches"][0]["content"].as_str().unwrap();
        assert_eq!(
            parsed_content, content,
            "em dash must survive JSON round-trip"
        );

        // Also verify the raw JSON contains the UTF-8 bytes, not escaped sequences
        assert!(
            json_str.contains("\u{2014}"),
            "JSON should contain raw UTF-8 em dash"
        );
    }

    // --- is_append_mode_component tests ---

    #[test]
    fn append_mode_component_exchange() {
        assert!(is_append_mode_component("exchange"));
        assert!(is_append_mode_component("findings"));
    }

    #[test]
    fn replace_mode_components_not_append() {
        assert!(!is_append_mode_component("pending"));
        assert!(!is_append_mode_component("status"));
        assert!(!is_append_mode_component("output"));
        assert!(!is_append_mode_component("todo"));
    }

    #[test]
    fn find_boundary_id_skips_code_blocks() {
        // Boundary-looking text inside a fenced code block must not be returned
        let content = "<!-- agent:exchange -->\n```\n<!-- agent:boundary:fake-id -->\n```\n<!-- /agent:exchange -->\n";
        let result = find_boundary_id(content, "exchange");
        assert!(
            result.is_none(),
            "boundary inside code block must not be found, got: {:?}",
            result
        );
    }

    #[test]
    fn find_boundary_id_finds_real_marker() {
        let content = "<!-- agent:exchange -->\nSome text.\n<!-- agent:boundary:real-uuid-5678 -->\nMore text.\n<!-- /agent:exchange -->\n";
        let result = find_boundary_id(content, "exchange");
        assert_eq!(result, Some("real-uuid-5678".to_string()));
    }

    #[test]
    fn stale_baseline_guard_prefix_check() {
        // Baseline that starts with snapshot content (user added text) = NOT stale
        let snapshot = "## Exchange\nResponse here.\n";
        let baseline_with_user_edit = "## Exchange\nResponse here.\nNew user question\n";
        let snap_clean = strip_boundary_for_dedup(snapshot);
        let base_clean = strip_boundary_for_dedup(baseline_with_user_edit);
        assert!(
            base_clean.starts_with(&snap_clean),
            "baseline with user edits should start with snapshot content"
        );

        // Baseline that doesn't contain snapshot content = STALE
        let stale_baseline = "## Exchange\nOld content only.\n";
        let stale_clean = strip_boundary_for_dedup(stale_baseline);
        assert!(
            !stale_clean.starts_with(&snap_clean),
            "stale baseline should not start with snapshot content"
        );
    }

    // --- is_stale_baseline tests ---

    #[test]
    fn stale_baseline_identical_content_not_stale() {
        let doc = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(doc, doc));
    }

    #[test]
    fn stale_baseline_user_appended_text_not_stale() {
        let snapshot = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\nUser question\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn stale_baseline_user_edited_replace_component_not_stale() {
        // User edits replace-mode component (status) — should NOT trigger stale guard
        let snapshot = "<!-- agent:status patch=replace -->\nOld status\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:status patch=replace -->\nEdited status by user\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\nNew question\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "user editing replace-mode status component should NOT trigger stale guard"
        );
    }

    #[test]
    fn stale_baseline_missing_committed_content_is_stale() {
        let snapshot = "<!-- agent:exchange patch=append -->\nCommitted response from agent.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nOld content only.\n<!-- /agent:exchange -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "baseline missing committed content should be stale"
        );
    }

    #[test]
    fn stale_baseline_missing_append_component_is_stale() {
        // Missing an append-mode component = stale
        let snapshot = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:other patch=append -->\nDifferent.\n<!-- /agent:other -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "baseline missing an append-mode component should be stale"
        );
    }

    #[test]
    fn stale_baseline_missing_replace_component_not_stale() {
        // Missing a replace-mode component is fine — user can delete it
        let snapshot = "<!-- agent:status patch=replace -->\nActive\n<!-- /agent:status -->\n\
                         <!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "missing replace-mode component should NOT trigger stale guard"
        );
    }

    #[test]
    fn stale_baseline_boundary_markers_ignored() {
        let snapshot = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nResponse.\n<!-- agent:boundary:xyz -->\nUser edit\n<!-- /agent:exchange -->\n";
        assert!(
            !is_stale_baseline(baseline, snapshot),
            "different boundary marker IDs should not cause false stale detection"
        );
    }

    #[test]
    fn stale_baseline_non_template_fallback_to_prefix() {
        // Non-template (no components) falls back to prefix check
        let snapshot = "## Exchange\nResponse.\n";
        let baseline = "## Exchange\nResponse.\nNew question\n";
        assert!(!is_stale_baseline(baseline, snapshot));

        let stale = "## Exchange\nDifferent content.\n";
        assert!(is_stale_baseline(stale, snapshot));
    }

    #[test]
    fn stale_baseline_empty_snapshot_component_skipped() {
        // Empty append components in snapshot should not cause false positives
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nUser added content\n<!-- /agent:exchange -->\n";
        assert!(!is_stale_baseline(baseline, snapshot));
    }

    #[test]
    fn stale_baseline_default_exchange_is_append() {
        // exchange without explicit patch attr defaults to append via is_append_mode_component
        let snapshot = "<!-- agent:exchange -->\nResponse.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange -->\nOld stuff.\n<!-- /agent:exchange -->\n";
        assert!(
            is_stale_baseline(baseline, snapshot),
            "exchange without patch attr should default to append-mode check"
        );
    }

    #[test]
    fn strip_boundary_for_dedup_removes_markers() {
        let with_boundary = "Hello\n<!-- agent:boundary:abc123 -->\nWorld\n";
        let without = strip_boundary_for_dedup(with_boundary);
        assert!(!without.contains("agent:boundary"));
        assert!(without.contains("Hello"));
        assert!(without.contains("World"));
    }

    // --- build_ipc_patches_json / synthesis dedup tests ---

    #[test]
    fn synthesis_dedup_skips_when_content_already_present() {
        // If the unmatched content already exists in the target component,
        // synthesis should be skipped (idempotent write guard).
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let existing = "This is the agent response.";
        let doc_content = format!(
            "<!-- agent:exchange patch=append -->\n{}\n<!-- /agent:exchange -->\n",
            existing
        );
        fs::write(&doc, &doc_content).unwrap();

        // No explicit patches (simulates skill sending raw content)
        let patches: Vec<crate::template::PatchBlock> = vec![];
        // Unmatched content is identical to what's already in the exchange
        let result = build_ipc_patches_json(&doc, &patches, existing).unwrap();

        assert!(
            result.is_empty(),
            "synthesis should be skipped when content already exists in target component, \
             got {} patches: {:?}",
            result.len(),
            result
        );
    }

    #[test]
    fn synthesis_proceeds_when_content_is_new() {
        // When unmatched content is NOT present in the target component,
        // synthesis should create an IPC patch.
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let doc_content =
            "<!-- agent:exchange patch=append -->\nExisting content.\n<!-- /agent:exchange -->\n";
        fs::write(&doc, doc_content).unwrap();

        let patches: Vec<crate::template::PatchBlock> = vec![];
        let new_content = "Completely new agent response.";
        let result = build_ipc_patches_json(&doc, &patches, new_content).unwrap();

        assert_eq!(result.len(), 1, "synthesis should produce one patch for new content");
        assert_eq!(
            result[0]["component"].as_str().unwrap(),
            "exchange",
            "synthesized patch should target exchange"
        );
        assert_eq!(
            result[0]["content"].as_str().unwrap(),
            new_content,
            "synthesized patch content should match unmatched"
        );
    }

    #[test]
    fn effective_unmatched_cleared_when_synthesis_consumes_content() {
        // When synthesis consumes the unmatched content (patches input was empty,
        // ipc_patches output is non-empty), effective_unmatched should be "".
        // This prevents the plugin from applying the content twice (IPC duplicate bug).
        let patches: Vec<crate::template::PatchBlock> = vec![];
        let unmatched = "some response content";

        // Case 1: synthesis happened (patches empty → ipc_patches non-empty)
        let ipc_patches: Vec<serde_json::Value> = vec![serde_json::json!({
            "component": "exchange",
            "content": unmatched,
        })];
        let effective = if patches.is_empty() && !ipc_patches.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective, "",
            "effective_unmatched must be empty when synthesis consumed content"
        );

        // Case 2: explicit patches (no synthesis) — unmatched passes through
        let explicit_patch = crate::template::PatchBlock::new("exchange", "response");
        let patches_explicit = vec![explicit_patch];
        let ipc_explicit: Vec<serde_json::Value> = vec![serde_json::json!({
            "component": "exchange",
            "content": "response",
        })];
        let effective2 = if patches_explicit.is_empty() && !ipc_explicit.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective2,
            unmatched.trim(),
            "effective_unmatched should pass through when explicit patches exist"
        );

        // Case 3: no patches, no synthesis (empty doc or dedup skipped it) — unmatched passes through
        let ipc_empty: Vec<serde_json::Value> = vec![];
        let effective3 = if patches.is_empty() && !ipc_empty.is_empty() {
            ""
        } else {
            unmatched.trim()
        };
        assert_eq!(
            effective3,
            unmatched.trim(),
            "effective_unmatched should pass through when no synthesis occurred"
        );
    }

    // ── normalize_user_prompts_in_exchange ──────────────────────────────────

    #[test]
    fn normalize_user_prompts_new_line_gets_prefix() {
        let snapshot = "<!-- agent:exchange patch=append -->\nOld content.\n<!-- /agent:exchange -->\n";
        // baseline = user added "Hello" but agent hasn't responded yet
        let baseline = "<!-- agent:exchange patch=append -->\nOld content.\nHello\n<!-- /agent:exchange -->\n";
        // content_ours = baseline + agent response appended (boundary at end after pre-patch)
        let content = "<!-- agent:exchange patch=append -->\nOld content.\nHello\n<!-- agent:boundary:abc123 -->\n### Re: response\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(result.contains("❯ Hello"), "user line should get ❯  prefix: {}", result);
        assert!(result.contains("Old content."), "old content should be preserved");
        assert!(result.contains("### Re: response"), "agent response should be preserved");
        assert!(!result.contains("❯ ###"), "agent heading should not get prefix: {}", result);
    }

    #[test]
    fn normalize_user_prompts_agent_response_not_prefixed() {
        // Regression: agent response lines in content_ours (before boundary) must NOT get ❯  prefix.
        // Before the fix, apply_patches_with_overrides moves the boundary to the end of exchange,
        // so the agent's response lines ended up in the "user region" and were incorrectly prefixed.
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        // baseline: user added "My question"
        let baseline = "<!-- agent:exchange patch=append -->\nOld.\nMy question\n<!-- /agent:exchange -->\n";
        // content_ours: boundary at end (after pre-patch), agent response before it
        let content = "<!-- agent:exchange patch=append -->\nOld.\nMy question\nAgent answer here.\n<!-- agent:boundary:xyz -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(result.contains("❯ My question"), "user question should get prefix: {}", result);
        assert!(!result.contains("❯ Agent answer"), "agent response should NOT get prefix: {}", result);
        assert!(result.contains("Agent answer here."), "agent response should be preserved: {}", result);
    }

    #[test]
    fn normalize_user_prompts_blank_line_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\nOld.\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nOld.\n\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nOld.\n\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        // blank line should not get prefix
        assert!(!result.contains("❯ \n"), "blank line should not be prefixed: {}", result);
    }

    #[test]
    fn normalize_user_prompts_heading_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n### Re: answer\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n### Re: answer\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(!result.contains("❯ ###"), "heading should not get prefix: {}", result);
    }

    #[test]
    fn normalize_user_prompts_already_prefixed_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Already prefixed\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Already prefixed\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(!result.contains("❯ ❯"), "should not double-prefix: {}", result);
        assert!(result.contains("❯ Already prefixed"), "prefix should be preserved");
    }

    #[test]
    fn normalize_user_prompts_existing_content_unchanged() {
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\nNew question\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ Previous question\n### Re: answer\nNew question\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        // Previous question already prefixed — should not double-prefix
        assert!(!result.contains("❯ ❯"), "should not double-prefix existing content: {}", result);
        // New question should get prefix
        assert!(result.contains("❯ New question"), "new line should get prefix: {}", result);
    }

    #[test]
    fn normalize_user_prompts_code_fence_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\nSome text.\n```bash\necho hello\n```\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\nSome text.\n```bash\necho hello\n```\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(!result.contains("❯ ```"), "code fence should not get prefix: {}", result);
        assert!(result.contains("❯ Some text."), "regular user line should get prefix: {}", result);
    }

    #[test]
    fn normalize_user_prompts_quoted_string_skipped() {
        let snapshot = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n\"Merge conflict with external write\"\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n\"Merge conflict with external write\"\n<!-- agent:boundary:abc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(!result.contains("❯ \""), "quoted string should not get prefix: {}", result);
    }

    #[test]
    fn normalize_user_prompts_no_exchange_passthrough() {
        let content = "No exchange here.\n";
        let baseline = "No exchange here.\n";
        let snapshot = "";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert_eq!(result, content, "document without exchange should pass through unchanged");
    }

    #[test]
    fn normalize_user_prompts_restores_prefix_lost_in_file() {
        // Regression: snapshot has ❯ do but the editor file (baseline) has do without prefix.
        // This happens when the IPC normalization fails to update the editor file.
        // The binary must restore ❯  so the snapshot stays correct and the
        // next IPC write delivers fullContent with the correct prefix.
        let snapshot = "<!-- agent:exchange patch=append -->\n❯ done\n❯ do\n- [ ] task\n<!-- /agent:exchange -->\n";
        let baseline = "<!-- agent:exchange patch=append -->\n❯ done\ndo\n- [ ] task\n<!-- /agent:exchange -->\n";
        let content = "<!-- agent:exchange patch=append -->\n❯ done\ndo\n- [ ] task\n<!-- agent:boundary:abc123:doc -->\n<!-- /agent:exchange -->\n";
        let result = normalize_user_prompts_in_exchange(content, baseline, snapshot);
        assert!(result.contains("❯ do"), "❯  prefix must be restored when snapshot had it but file lost it: {}", result);
        assert!(!result.contains("\ndo\n"), "bare do line must not remain without prefix: {}", result);
        // ❯ done must not be double-prefixed
        assert!(!result.contains("❯ ❯"), "no double-prefix: {}", result);
    }
}
