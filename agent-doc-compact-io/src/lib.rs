//! # Module: compact
//!
//! ## Spec
//! - Reduces document size by archiving old content to `.agent-doc/archives/<hash>-<timestamp>.md`.
//! - **Inline/append mode:** parses `## User` / `## Assistant` exchange pairs from the document
//!   body; archives all but the `keep` most-recent complete pairs; rebuilds the document with an
//!   archive summary line and a trailing `## User` prompt block.
//!   - Trailing `## User` blocks without an assistant reply are never counted or archived.
//!   - Code blocks containing `## User` / `## Assistant` headings are not treated as section
//!     boundaries.
//! - **Template/stream mode (full compact):** when `keep` is `None`, archives the full content of
//!   a named component (default `exchange`) and replaces it with a summary marker; optionally
//!   accepts a custom `--message`.
//!   - For `exchange`, unresolved text after the boundary marker is preserved as live drift in the
//!     visible document and omitted from the archive, summary digest, saved snapshot, and commit.
//! - **Template/stream mode (partial compact):** when `keep` is `Some(N)`, archives all but the
//!   last N `### Re:` topic sections; rebuilds the component with an archive summary + kept topics.
//!   - The preamble (content before the first `### Re:` heading) is always preserved; use
//!     `--message` to replace it with a fresh session summary.
//!   - If section count ≤ N, logs to stderr and exits `Ok(())` without modifying the document.
//!   - If the document uses CRDT write strategy, the CRDT state is compacted (GC tombstones)
//!     before the component replacement.
//! - Archive filenames are derived from the snapshot hash + a UTC timestamp computed without
//!   the `chrono` crate.
//! - Document replacement uses the same visible-buffer idle and compare-and-swap guard as direct
//!   response writes. Compact exchange must not emit full-document editor IPC because a whole-file
//!   editor replacement can race with the operator drafting the next prompt.
//! - `commit: true` closes out compaction through the binary-owned `agent-doc commit` path and
//!   verifies the VCS refresh signal when that channel exists.
//!
//! ## Agentic Contracts
//! - `run(file, keep, component_name, message, tag, commit) -> Result<()>` — entry point;
//!   dispatches to component compact (template/stream) or exchange compact (inline) based on
//!   frontmatter mode.
//! - `keep: None` in template mode → full compact (archive all).
//! - `keep: Some(N)` in template mode → partial compact (archive all but last N `### Re:` sections).
//! - `keep: None` in inline mode → uses default of 2.
//! - If `exchanges.len() <= keep` in inline mode, logs to stderr and exits `Ok(())` without
//!   modifying the document.
//! - Archive path is always under `.agent-doc/archives/` relative to the project root (found by
//!   walking up to the directory containing `.agent-doc/`).
//! - Returns `Err` if the file does not exist, the named component is not found (template mode),
//!   or any I/O operation fails.
//! - `tag: Some(name)` creates a lightweight git tag at HEAD before compaction (pre-compact checkpoint).
//!   `tag: None` auto-generates `agent-doc/<doc-name>/pre-compact-N` where N is the next ordinal.
//! - `message: Some("-")` reads from stdin (standard Unix convention for CLIs that accept
//!   file-or-string arguments).
//! - `commit: true` uses `git::commit_with_outcome` after a successful mutation. If the commit
//!   path reports that a VCS refresh signal target existed but writing it failed, compact fails
//!   closed instead of silently accepting the closeout.
//! - `commit: true` stages the AUTHORITATIVE in-memory compacted content, not a re-loaded snapshot
//!   or the working-tree re-read: a stale-supervisor CRDT overlay replay can revert the snapshot,
//!   and an editor-IPC convergence can leave the working tree lagging, either of which would leave
//!   HEAD at the pre-compact content (`#jb-compact-commit-left-uncommitted`). `commit_compacted_authoritative`
//!   re-asserts the compacted snapshot immediately before the commit and then FAILS CLOSED (with a
//!   `reset --from-current` recovery command) if the post-commit HEAD did not actually land the
//!   compacted content, so `--commit` can never silently leave uncommitted compaction drift.
//! - Before an editor-IPC `--commit` closeout, `flush_editor_buffer_to_disk_after_compact` asks the
//!   live editor to save its converged buffer to disk (`save_document` IPC) so the working-tree file
//!   converges to the compacted content. The plugin applies convergence patches to the in-memory
//!   Document and never saves, so without this flush HEAD can hold the summary while the disk file
//!   still holds the pre-compact content — the "JB Compact Exchange left an uncommitted summary"
//!   defect (`#jb-compact-editor-buffer-flush`). Fail-open: the authoritative snapshot is still
//!   committed and verified.
//! - `apply_compacted_document` is the single replacement boundary used by inline, full component,
//!   and partial component compaction.
//!
//! ## Evals
//! - parse_basic: two complete exchanges + trailing User → 2 exchanges parsed, trailing skipped
//! - parse_code_blocks: `## User` inside fenced block → not treated as section boundary
//! - keep_threshold: exchange count ≤ keep → no-op, `Ok(())` returned
//! - archive_format: archived content contains `archived_from: compact`, session ID, exchange text
//! - compacted_format: result has archive summary line, kept exchanges, trailing `## User\n\n`
//! - timestamp_format: `chrono_timestamp()` → 15-char string matching `YYYYMMDD-HHMMSS`
//! - component_archive_format: template-mode archive contains component name, session ID, content
//! - partial_compact_keep_threshold: sections ≤ keep → no-op
//! - partial_compact_archive_format: archive contains preamble + archived sections
//! - partial_compact_result_format: result has archive pointer + preamble (or message) + kept sections
//! - exchange_compact_default_summary_includes_archived_content_digest: default full compact summary
//!   includes an archive pointer and compacted exchange digest
//! - compact_with_commit_writes_vcs_refresh_signal: `--commit` closeout creates an `agent-doc`
//!   commit and updates `.agent-doc/patches/vcs-refresh.signal` when that refresh channel exists
//! - compact_dirty_treats_diverged_snapshot_as_committable: a diverged snapshot with an unflushed
//!   working-tree file is still committable (`#jb-compact-commit-editor-ipc-async`)
//! - compact_commit_lands_head_when_snapshot_replayed_stale: authoritative-content commit lands the
//!   compacted content in HEAD even when a CRDT replay reverted the on-disk snapshot to pre-compact
//! - compact_with_commit_flushes_editor_buffer_to_disk: an editor-IPC `--commit` compaction against a
//!   buffer-only live editor (applies to the buffer, never saves) leaves the working-tree file equal
//!   to HEAD because the closeout flushes the editor buffer to disk (`#jb-compact-editor-buffer-flush`)
//! - compact_commit_fails_closed_when_head_cannot_land: `--commit` fails closed with a recovery
//!   command instead of silently leaving uncommitted compaction drift (`#jb-compact-commit-left-uncommitted`)
//! - component_compact_uses_guarded_direct_write_when_patches_dir_exists: compact does not emit a
//!   fullContent IPC patch for template exchange compaction and uses the guarded disk path instead
//! - message_dash_reads_stdin: `--message -` reads from stdin instead of using literal "-"

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::OnceLock;

use agent_doc_document::compact_archive::{
    CompactArchiveMetadata, build_component_archive_content, build_exchange_compact_summary,
    build_inline_exchange_archive_content, compact_archive_session,
    format_compact_timestamp_from_unix_secs,
};
use agent_doc_document::compact_projection::{
    CompactExchange, build_inline_compacted_document, changed_non_exchange_opening_markers,
    malformed_compact_summary_lines, parse_inline_exchanges, split_component_content_at_boundary,
};
use agent_doc_element::element;
use agent_doc_frontmatter::frontmatter;
use agent_doc_sqlite::archive_index;

use agent_doc_topic::parse_topic_sections_with_tail;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CompactCommitOutcome {
    pub did_commit: bool,
    pub vcs_refresh_signaled: Option<bool>,
}

pub trait CompactRuntimeEffects: Sync {
    fn commit_with_outcome(&self, file: &Path) -> Result<CompactCommitOutcome>;
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;
    fn try_editor_converge(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<bool>;
    fn guard_no_stale_snapshot_reset_drift(
        &self,
        file: &Path,
        projected: Option<&str>,
        visible: &str,
        stage: &str,
    ) -> Result<bool>;
}

static RUNTIME_EFFECTS: OnceLock<&'static dyn CompactRuntimeEffects> = OnceLock::new();

pub fn install_runtime_effects(effects: &'static dyn CompactRuntimeEffects) {
    let _ = RUNTIME_EFFECTS.set(effects);
}

fn runtime_effects() -> Result<&'static dyn CompactRuntimeEffects> {
    if let Some(effects) = RUNTIME_EFFECTS.get().copied() {
        return Ok(effects);
    }
    #[cfg(test)]
    {
        Ok(&TEST_RUNTIME_EFFECTS)
    }
    #[cfg(not(test))]
    anyhow::bail!("agent-doc compact runtime effects are not installed")
}

#[cfg(test)]
struct TestCompactRuntimeEffects;

#[cfg(test)]
impl CompactRuntimeEffects for TestCompactRuntimeEffects {
    fn commit_with_outcome(&self, file: &Path) -> Result<CompactCommitOutcome> {
        let outcome = agent_doc_commit_io::commit_with_outcome(file)?;
        Ok(CompactCommitOutcome {
            did_commit: outcome.did_commit,
            vcs_refresh_signaled: outcome.vcs_refresh_signaled,
        })
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn try_editor_converge(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<bool> {
        agent_doc_write_converge_io::try_editor_converge(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            target_content,
            source_content,
            reason,
        )
    }

    fn guard_no_stale_snapshot_reset_drift(
        &self,
        file: &Path,
        projected: Option<&str>,
        visible: &str,
        stage: &str,
    ) -> Result<bool> {
        agent_doc_write_converge_io::guard_no_stale_snapshot_reset_drift(
            file, projected, visible, stage,
        )
    }
}

#[cfg(test)]
static TEST_RUNTIME_EFFECTS: TestCompactRuntimeEffects = TestCompactRuntimeEffects;

#[cfg(test)]
pub(crate) struct PipelineFrontmatterEffects;

#[cfg(test)]
pub(crate) const PIPELINE_FRONTMATTER_EFFECTS: PipelineFrontmatterEffects =
    PipelineFrontmatterEffects;

#[cfg(test)]
impl agent_doc_cycle_state_io::pipeline_frontmatter::PipelineFrontmatterEffects
    for PipelineFrontmatterEffects
{
    fn converge_or_disk_write(
        &self,
        file: &Path,
        current_content: &str,
        target_content: &str,
        reason: &str,
    ) -> Result<()> {
        runtime_effects()?
            .try_editor_converge(file, target_content, current_content, reason)
            .map(|_| ())
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

#[cfg(test)]
pub(crate) mod test_support {
    use std::path::Path;

    pub(crate) fn wait_for_live_prompt_drift_listener(project_root: &Path) {
        for _ in 0..100 {
            if agent_doc_ipc_io::is_listener_active(project_root) {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(10));
        }
        panic!("fake socket listener did not start within 1s");
    }
}

/// Run the compact command.
///
/// - `keep`: number of recent exchanges/topics to keep.
///   - Inline mode: `None` defaults to 2.
///   - Template mode: `None` archives all (full compact); `Some(N)` archives all but last N `### Re:` sections.
/// - `component_name`: targets a specific component in template/stream mode.
/// - `message`: summary marker text (default: auto-generated).
/// - `tag`: git tag to create at HEAD before compaction. `None` auto-generates
///   `agent-doc/<doc-name>/pre-compact-N`. Pass `Some("skip")` to skip tagging entirely.
pub fn run(
    file: &Path,
    keep: Option<usize>,
    component_name: Option<&str>,
    message: Option<&str>,
    tag: Option<&str>,
    commit: bool,
    force_disk: bool,
) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let stdin_message;
    let message = if message == Some("-") {
        use std::io::Read;
        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("failed to read --message from stdin")?;
        stdin_message = buf;
        Some(stdin_message.as_str())
    } else {
        message
    };

    // Create a pre-compact git tag at HEAD before modifying the document.
    // Skipped if tag == Some("skip").
    if tag != Some("skip")
        && let Err(e) =
            agent_doc_git_io::checkpoint::create_pre_mutation_tag(file, "pre-compact", tag)
    {
        eprintln!("[compact] Warning: could not create pre-compact tag: {}", e);
    }

    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let (fm, body) = frontmatter::parse(&content)?;

    let resolved = fm.resolve_mode();
    // `authoritative` is the compacted snapshot content the compaction just
    // produced. It is the single source of truth for the `--commit` closeout —
    // never a re-loaded snapshot (a concurrent stale-supervisor CRDT overlay
    // replay can revert it, `#compact-overlay-crdt-staleness`) nor the working-tree
    // re-read (it lags an editor-IPC convergence, `#jb-compact-commit-editor-ipc-async`).
    let authoritative: String = if resolved.is_template() {
        // NOTE (#compact-overlay-crdt-staleness): the legacy CRDT/overlay refresh
        // is deferred to `apply_compacted_document(..., refresh_crdt=true)`, which
        // is the single authoritative CRDT writer. An earlier
        // `save_document_crdt(file, &compact(&crdt_state), &content)` here was a
        // defect: it saved the overlay CRDT with the PRE-compaction (large)
        // `&content` markdown. Later cycles re-projected that overlay
        // (`load_overlay_crdt`→`to_markdown`) and reproduced snapshot(large) >
        // visible(small), tripping `guard_no_stale_snapshot_reset_drift`'s
        // "looks like a manual cleanup" refusal. Because
        // `apply_compacted_document` rebuilds the CRDT from the COMPACTED text
        // (`CrdtDoc::from_text` / `OverlayCrdtDoc::from_markdown`, fresh docs with
        // no carried tombstones), that rebuild both refreshes the overlay with the
        // correct compacted markdown and supersedes the old tombstone-GC step.
        let target = component_name.unwrap_or("exchange");
        let is_crdt = resolved.is_crdt();
        match keep {
            Some(n) => run_component_compact_partial(
                file, &content, target, n, message, is_crdt, force_disk,
            ),
            None => run_component_compact_with_options(
                file, &content, target, message, is_crdt, force_disk,
            ),
        }?
    } else {
        let keep_n = keep.unwrap_or(2);

        // Parse exchanges from the body
        let exchanges = parse_inline_exchanges(body);

        if exchanges.len() <= keep_n {
            eprintln!(
                "[compact] Only {} exchange(s) found, keeping all (threshold: {})",
                exchanges.len(),
                keep_n
            );
            return Ok(());
        }

        let to_archive = &exchanges[..exchanges.len() - keep_n];
        let to_keep = &exchanges[exchanges.len() - keep_n..];

        // Build archive content
        let archive_content = build_archive(file, &content, to_archive);

        // Save archive
        let archive_path = save_archive(file, &archive_content)?;

        // Build compacted document
        let mut compacted = build_inline_compacted_document(
            &content,
            body,
            to_keep,
            &archive_path.display().to_string(),
            to_archive.len(),
        );
        if let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(&compacted)?
        {
            compacted = reconciled;
        }

        apply_compacted_document(file, &compacted, &compacted, &content, false, force_disk)?;
        discard_archived_captures(file, &archive_content);

        eprintln!(
            "[compact] Archived {} exchange(s) to {}",
            to_archive.len(),
            archive_path.display()
        );
        eprintln!(
            "[compact] {} exchange(s) remain in {}",
            to_keep.len(),
            file.display()
        );
        compacted
    };

    // `#jb-compact-editor-buffer-flush`: the editor-IPC convergence in
    // `apply_compacted_document` updated only the live editor's in-memory buffer;
    // the plugin never saves. Flush it to disk NOW — before the re-read below and
    // the commit — so the working-tree file holds the compacted content. Otherwise
    // the selective commit compares the stale pre-compact working tree against the
    // compacted snapshot, treats the snapshot as historical exchange drift, and
    // repairs it back to HEAD, leaving HEAD and disk pre-compact (the "JB Compact
    // Exchange left an uncommitted summary" defect). Fail-open: if the editor
    // cannot flush, `commit_compacted_authoritative` / `compact_dirty` still stage
    // the authoritative snapshot and verify HEAD.
    if commit && !force_disk {
        let disk_is_pre_compact = std::fs::read_to_string(file)
            .map(|disk| disk == content)
            .unwrap_or(false);
        if disk_is_pre_compact {
            flush_editor_buffer_to_disk_after_compact(file, &authoritative);
        }
    }

    if component_name.is_none() || component_name == Some("exchange") {
        agent_doc_session_accretion_io::record_recent_exchange_compaction(file)?;
    }

    let updated = std::fs::read_to_string(file)
        .with_context(|| format!("failed to re-read {} after compact", file.display()))?;
    // `#compactdropitem`: re-verify against the document actually on disk so a
    // concurrent stale-supervisor CRDT merge that interleaved over the written
    // file (dropping a non-exchange item) fails closed before commit instead of
    // staging the corrupted snapshot into HEAD.
    assert_non_exchange_items_preserved(file, &content, &updated, "post_write")?;
    let changed = updated != content;
    // `#jb-compact-commit-editor-ipc-async`: the post-compact on-disk re-read
    // (`updated`) can lag the editor-converged buffer when the compaction wrote
    // through the live editor IPC (`transport=editor_ipc`): the editor owns the
    // buffer and flushes to disk on its own schedule, so `updated` still equals
    // the pre-compact `content` and `changed` is false even though the compaction
    // produced committable state. `apply_compacted_document` saved the compacted
    // SNAPSHOT synchronously and `commit_with_outcome` stages that snapshot, so
    // treat a snapshot that diverges from HEAD as committable/dirty too. Without
    // this, `--commit` is silently skipped and the compacted document is left
    // uncommitted (HEAD stale, editor/visible buffer compacted) — the exact
    // "JB Compact Exchange left uncommitted changes" defect.
    let snapshot_status = agent_doc_snapshot_io::verify_snapshot_committed(file)?;
    let dirty = compact_dirty(changed, &snapshot_status);
    if commit {
        if dirty {
            commit_compacted_authoritative(file, &authoritative)?;
        }
    } else if dirty {
        // #jb-compact-repair-left-uncommitted: a compact/repair that rewrites the
        // exchange but does not cross a commit boundary leaves the document dirty
        // (corrected visible content, stale HEAD). Later JetBrains/route actions
        // then see mixed visible/HEAD/snapshot state. Surface it explicitly with
        // the exact recovery command instead of silently leaving it dirty.
        agent_doc_ops_log_io::log_op(
            file,
            &format!("compact_left_uncommitted file={}", file.display()),
        );
        eprintln!(
            "[compact] WARNING: {} was compacted but NOT committed (--commit not set). The corrected document is dirty (visible content differs from HEAD); later actions may route/queue from a stale surface. Commit it with `agent-doc compact {} --commit` or `agent-doc write --commit {}` before continuing (#jb-compact-repair-left-uncommitted).",
            file.display(),
            file.display(),
            file.display()
        );
    }

    Ok(())
}

/// Decide whether a compaction left committable/dirty state.
///
/// `changed_on_disk` is the pre-commit on-disk re-read comparison
/// (`updated != content`). It is authoritative when the compaction wrote through
/// the guarded disk path, but it lags the editor-converged buffer when the write
/// went through the live editor IPC (`transport=editor_ipc`). In that window the
/// disk file still equals the pre-compact content, so `changed_on_disk` is false
/// even though the synchronously-saved compacted snapshot already diverges from
/// HEAD. Treat that snapshot divergence as dirty so `--commit` still stages the
/// compacted snapshot (`#jb-compact-commit-editor-ipc-async`). A `NotInGitRepo`
/// document has no HEAD to diverge from, so it falls back to `changed_on_disk`.
fn compact_dirty(
    changed_on_disk: bool,
    snapshot_status: &agent_doc_snapshot_io::SnapshotCommitStatus,
) -> bool {
    changed_on_disk
        || matches!(
            snapshot_status,
            agent_doc_snapshot_io::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
        )
}

/// Commit a compaction, staging the authoritative in-memory compacted content.
///
/// `#jb-compact-commit-left-uncommitted`: the live "JB Compact Exchange left
/// uncommitted changes" defect had two desync mechanisms that both left HEAD at
/// the pre-compact content while the editor/visible buffer held the compacted
/// document:
///  1. a concurrent stale-supervisor CRDT overlay replay reverted the on-disk
///     snapshot back to pre-compact before the selective commit staged it
///     (`#compact-overlay-crdt-staleness`); and
///  2. the working-tree file lagged an editor-IPC convergence, so the selective
///     commit misclassified the newer snapshot as stale historical exchange drift
///     and repaired it back to HEAD (`#jb-compact-commit-editor-ipc-async`).
///
/// Defeat both by re-asserting the authoritative compacted snapshot immediately
/// before the commit (inside the commit lock window), then FAIL CLOSED if HEAD did
/// not actually land the compacted content — turning a silent "left uncommitted
/// changes" into a loud, recoverable error with the exact recovery command.
fn commit_compacted_authoritative(file: &Path, authoritative_snapshot: &str) -> Result<()> {
    // Re-assert the authoritative snapshot so a replay/lag between
    // `apply_compacted_document` and here cannot leave a pre-compact snapshot for
    // the selective commit to stage.
    agent_doc_snapshot_io::save(file, authoritative_snapshot, agent_doc_ops_log_io::log_op)?;
    closeout_compact_with_commit(file)?;
    verify_compact_head_landed(file, authoritative_snapshot)
}

/// Fail closed if the post-commit HEAD does not hold the compacted content.
///
/// Comparing HEAD directly against the authoritative compacted content (not merely
/// snapshot==HEAD) is what distinguishes a real compact commit from a selective
/// commit that no-oped / repaired the snapshot back to the pre-compact HEAD.
fn verify_compact_head_landed(file: &Path, authoritative_snapshot: &str) -> Result<()> {
    use agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;
    let head = agent_doc_git_io::revision::show_head(file)?;
    let landed = head.as_deref().is_some_and(|head| {
        normalize_transient_agent_doc_markers(head)
            == normalize_transient_agent_doc_markers(authoritative_snapshot)
    });
    if !landed {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "compact_commit_head_mismatch file={} head_len={} authoritative_len={}",
                file.display(),
                head.as_deref().map(str::len).unwrap_or(0),
                authoritative_snapshot.len()
            ),
        );
        anyhow::bail!(
            "compact --commit did not land the compacted content in HEAD for {} (a stale-supervisor CRDT replay or an unflushed editor buffer desynced the commit surface). The document is left with uncommitted compaction drift. Recover with `agent-doc reset --from-current {}` then `agent-doc compact {} --component exchange --commit` (#jb-compact-commit-left-uncommitted).",
            file.display(),
            file.display(),
            file.display()
        );
    }
    Ok(())
}

/// `#jb-compact-editor-buffer-flush`: converge the working-tree disk file with
/// the compacted editor buffer before an editor-IPC compaction commit.
///
/// The editor-IPC convergence in `apply_compacted_document` replaces the live
/// editor's in-memory buffer through the plugin's Document API; the plugin does
/// **not** save (unlike a normal response append, a compaction has no `(HEAD)`
/// markers to preserve in the working tree). So after the `--commit` closeout
/// stages the compacted snapshot into HEAD, the working-tree file on disk still
/// holds the pre-compact content and `git status` reports the document dirty —
/// the operator sees "Compact Exchange left an uncommitted summary" even though
/// HEAD is already correct.
///
/// Ask the live editor to flush its buffer to disk with the same `save_document`
/// IPC that `preflight` uses to resolve `live_prompt_drift`, then wait (bounded)
/// for the working-tree file to stop matching `pre_compact`. The plugin saves the
/// buffer it already converged, so disk converges before the commit stages it.
/// Returns `true` once disk matches the compacted content. Fail-open:
/// `commit_compacted_authoritative` still verifies HEAD after the commit and fails
/// closed if the compacted content did not land.
fn flush_editor_buffer_to_disk_after_compact(file: &Path, expected_content: &str) -> bool {
    let canonical = match file.canonicalize() {
        Ok(canonical) => canonical,
        Err(e) => {
            eprintln!(
                "[compact] warning: could not resolve {} to flush the editor buffer after compact: {e}",
                file.display()
            );
            return false;
        }
    };
    let project_root = agent_doc_project_root_io::resolve_ipc_project_root(&canonical);
    let path_str = canonical.to_string_lossy().to_string();
    let patch_id = format!("compact-flush-{}", uuid::Uuid::new_v4());

    if compact_disk_matches_expected(&canonical, expected_content) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "compact_editor_buffer_flush file={} patch_id={} transport=already_disk",
                file.display(),
                patch_id
            ),
        );
        return true;
    }

    let flushed = if agent_doc_ipc_io::is_listener_active(&project_root) {
        agent_doc_ipc_io::send_save_document(&project_root, &path_str, &patch_id)
    } else if project_root.join(".agent-doc").join("patches").is_dir() {
        // Socket down but the plugin is installed: signal a save through the
        // file-IPC patches directory (the degraded editor path).
        agent_doc_ipc_io::send_save_document_file_signal(&project_root, &path_str, &patch_id)
    } else {
        // No live editor owns the buffer; the guarded disk write already made the
        // working tree authoritative, so there is nothing to flush.
        return false;
    };

    if let Err(e) = flushed {
        eprintln!(
            "[compact] warning: editor buffer flush after compact failed for {} (working tree may lag HEAD until the editor saves): {e}",
            file.display()
        );
        return false;
    }

    // The socket `save_document` responds after saving; a file-IPC signal is applied
    // asynchronously, so poll the working tree until the flush lands (or time out).
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1000);
    loop {
        if compact_disk_matches_expected(&canonical, expected_content) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "compact_editor_buffer_flush file={} patch_id={} transport=save_document",
                    file.display(),
                    patch_id
                ),
            );
            return true;
        }
        if std::time::Instant::now() >= deadline {
            eprintln!(
                "[compact] warning: editor buffer flush requested for {} but the working tree still lags after 1s; the commit will fall back to the authoritative snapshot",
                file.display()
            );
            return false;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
}

fn compact_disk_matches_expected(file: &Path, expected_content: &str) -> bool {
    std::fs::read_to_string(file).is_ok_and(|disk| {
        agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&disk)
            == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                expected_content,
            )
    })
}

fn closeout_compact_with_commit(file: &Path) -> Result<()> {
    let outcome = runtime_effects()?.commit_with_outcome(file)?;
    if outcome.did_commit && outcome.vcs_refresh_signaled == Some(false) {
        anyhow::bail!(
            "compact closeout committed {} but failed to write vcs-refresh.signal",
            file.display()
        );
    }
    eprintln!(
        "[compact] note: --commit persists only the compacted document state now in HEAD; any later console explanation still needs its own `agent-doc finalize` or `agent-doc write --commit` cycle to land in `exchange`"
    );
    Ok(())
}

/// `#stale-capture-after-compaction-blocks-route`: best-effort discard of the
/// capture sidecars whose response body was just archived. Never fails the
/// compaction — a discard error is logged and ignored.
fn discard_archived_captures(file: &Path, archived_text: &str) {
    if let Err(e) =
        agent_doc_capture_io::discard_captures_for_archived_responses(file, archived_text)
    {
        eprintln!(
            "[compact] warning: failed to discard captures for archived responses in {}: {}",
            file.display(),
            e
        );
    }
}

fn validate_compacted_exchange(file: &Path, compacted: &str) -> Result<()> {
    let malformed = malformed_compact_summary_lines(compacted);
    if malformed.is_empty() {
        return Ok(());
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "compact_malformed_summary_rejected file={} count={}",
            file.display(),
            malformed.len()
        ),
    );
    anyhow::bail!(
        "[compact] INTERRUPTED: post-compact exchange is malformed — compact summary line(s) rendered as a user prompt (`❯ ` prefix): {}. The compact output was not committed (see #jb-compact-malformed-response-commit). Re-run compaction; if it recurs, the archived prompt tail is leaking its `❯` marker onto the summary and the source exchange needs manual repair before compacting.",
        malformed.join(" | ")
    )
}

/// `#compactdropitem`: fail closed if a compaction rewrite dropped a whole item
/// from any non-exchange singleton list component.
///
/// `before` is the pre-compact document; `after` is the rebuilt/written
/// document. Compaction legitimately rewrites `exchange` (archived/truncated)
/// and may reconcile the generated `status` top-backlog sentence, but it must
/// never reduce the item count of `backlog`/`review`/`done`/`queue`/`icebox`.
/// A decrease means either a deterministic regression in the rebuild or a
/// concurrent stale-supervisor CRDT merge interleaving over the written file —
/// either way the corrupt document must not reach the snapshot/HEAD.
fn assert_non_exchange_items_preserved(
    file: &Path,
    before: &str,
    after: &str,
    stage: &str,
) -> Result<()> {
    let dropped =
        agent_doc_element_backlog::backlog::dropped_tracked_component_items(before, after)
            .into_iter()
            .map(|drop| format!("{} {}→{}", drop.component, drop.before, drop.after))
            .collect::<Vec<_>>();

    if dropped.is_empty() {
        return Ok(());
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "compact_dropped_non_exchange_item file={} stage={} dropped={}",
            file.display(),
            stage,
            dropped.join(",")
        ),
    );
    anyhow::bail!(
        "[compact] INTERRUPTED: compaction dropped item(s) from non-exchange component(s) [{}] in {} (stage={}). Compaction must only archive/truncate the exchange; a singleton list component lost a whole tracked item (#compactdropitem — worse sibling of #compactqattr). The compact output was NOT committed. Re-run compaction; if it recurs, a concurrent stale-supervisor CRDT merge is interleaving over the written file — refresh the stale supervisor without discarding the live turn (`agent-doc admin recycle` or `agent-doc session restart-supervisor {}`) before retrying.",
        dropped.join(", "),
        file.display(),
        stage,
        file.display()
    )
}

/// `#compactqattr`: fail closed if a compaction rewrite altered any non-exchange
/// component's opening marker — most importantly dropping inline attributes
/// (`priority`, `preset="..."`, `go`) from an `agent:queue` marker. Operator
/// observed (sampleorders.md) post-compaction wiping all `agent:queue`
/// attributes ("too much blast radius").
///
/// The deterministic compaction rebuild (`Component::replace_content`, byte-offset
/// based) preserves these markers verbatim — proven by
/// `compact_preserves_queue_marker_inline_attributes`. This guard is
/// defense-in-depth: it makes the "every non-exchange opening marker stays
/// byte-identical" contract a hard fail-closed invariant so a future rebuild
/// regression (or a re-render step added to the compact path) cannot silently
/// strip attributes into the snapshot/HEAD. It is the marker-level sibling of
/// `assert_non_exchange_items_preserved` (#compactdropitem).
fn assert_non_exchange_markers_preserved(
    file: &Path,
    before: &str,
    after: &str,
    stage: &str,
) -> Result<()> {
    let changed = changed_non_exchange_opening_markers(before, after)
        .into_iter()
        .map(|change| change.describe())
        .collect::<Vec<_>>();

    if changed.is_empty() {
        return Ok(());
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "compact_altered_non_exchange_marker file={} stage={} changed={}",
            file.display(),
            stage,
            changed.join(" ; ")
        ),
    );
    anyhow::bail!(
        "[compact] INTERRUPTED: compaction altered the opening marker of non-exchange component(s) [{}] in {} (stage={}). Compaction must only archive/truncate the exchange and leave every other component opening marker byte-identical, including inline attributes like `priority`/`preset`/`go` (#compactqattr). The compact output was NOT committed. If this recurs from a non-wedged session it is a deterministic rebuild regression; if a stale-supervisor CRDT merge is interleaving over the written file, refresh the stale supervisor without discarding the live turn (`agent-doc admin recycle` or `agent-doc session restart-supervisor {}`) before retrying.",
        changed.join(", "),
        file.display(),
        stage,
        file.display()
    )
}

fn apply_compacted_document(
    file: &Path,
    compacted: &str,
    snapshot_content: &str,
    source_content: &str,
    refresh_crdt: bool,
    force_disk: bool,
) -> Result<()> {
    // Fail closed before any write if the rebuilt exchange is structurally
    // malformed (#jb-compact-malformed-response-commit).
    validate_compacted_exchange(file, compacted)?;

    // Fail closed before any write if the rebuild dropped a whole item from a
    // non-exchange singleton list component (#compactdropitem).
    assert_non_exchange_items_preserved(file, source_content, compacted, "apply")?;

    // Fail closed before any write if the rebuild altered a non-exchange
    // opening marker (dropped inline attributes like preset/priority/go)
    // (#compactqattr).
    assert_non_exchange_markers_preserved(file, source_content, compacted, "apply")?;

    if force_disk {
        runtime_effects()?.atomic_write(file, compacted)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "compact_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                compacted.len(),
                agent_doc_hash::content_hash(compacted)
            ),
        );
    } else {
        // `#w42v`: converge the compacted content through the editor path so it does
        // not diverge from the visible buffer and raise a `File Cache Conflict`.
        // If no live editor owns the document, `try_editor_converge` may use the
        // guarded DetachedDisk path, but only after rechecking that the current
        // visible file still matches the compact input.
        runtime_effects()?.try_editor_converge(file, compacted, source_content, "compact")?;
    }

    agent_doc_snapshot_io::save(file, snapshot_content, agent_doc_ops_log_io::log_op)?;

    if refresh_crdt {
        let new_crdt = agent_doc_merge::crdt::CrdtDoc::from_text(compacted).encode_state();
        agent_doc_merge_io::save_document_crdt(file, &new_crdt, compacted)?;
        eprintln!("[compact] CRDT state refreshed from post-compact content");
    }

    Ok(())
}

/// Compact a named component in a template/stream-mode document.
///
/// Archives the component content and replaces it with a summary marker.
/// Single atomic write — no intermediate state.
#[cfg(test)]
fn run_component_compact(
    file: &Path,
    content: &str,
    target: &str,
    message: Option<&str>,
    is_crdt: bool,
) -> Result<String> {
    run_component_compact_with_options(file, content, target, message, is_crdt, false)
}

#[cfg(test)]
fn run_component_compact_force_disk(
    file: &Path,
    content: &str,
    target: &str,
    message: Option<&str>,
    is_crdt: bool,
) -> Result<String> {
    run_component_compact_with_options(file, content, target, message, is_crdt, true)
}

/// Returns the authoritative compacted snapshot content that was written, so the
/// `--commit` closeout can stage it directly instead of re-loading a snapshot that
/// a concurrent CRDT replay may have reverted. When the component is already empty
/// (nothing to compact), returns the original `content` unchanged.
fn run_component_compact_with_options(
    file: &Path,
    content: &str,
    target: &str,
    message: Option<&str>,
    is_crdt: bool,
    force_disk: bool,
) -> Result<String> {
    let components = element::parse(content)?;
    let comp = components
        .iter()
        .find(|c| c.name == target)
        .ok_or_else(|| anyhow::anyhow!("component '{}' not found in document", target))?;

    let old_content = comp.content(content);
    let (archive_content, trailing) = if target == "exchange" {
        split_component_content_at_boundary(old_content)
    } else {
        (old_content.to_string(), String::new())
    };
    let trimmed = archive_content.trim();

    if trimmed.is_empty() {
        eprintln!("[compact] Component '{}' is already empty", target);
        return Ok(content.to_string());
    }

    // Archive old content
    let archive_path = save_archive(
        file,
        &build_component_archive(file, content, target, &archive_content),
    )?;

    // Build summary marker
    let summary = match message {
        Some(msg) => format!("{}\n", msg),
        None if target == "exchange" => {
            let summary_source = comp.replace_content(content, &archive_content);
            build_exchange_compact_summary(&summary_source, &archive_path.display().to_string())
        }
        None => format!(
            "*Compacted. Content archived to `{}`*\n",
            archive_path.display()
        ),
    };

    let mut visible_content = summary.clone();
    if !trailing.trim().is_empty() {
        if !visible_content.ends_with('\n') {
            visible_content.push('\n');
        }
        visible_content.push_str(trailing.trim_end());
        visible_content.push('\n');
    }

    let compacted = comp.replace_content(content, &visible_content);
    let mut compacted = agent_doc_template::repair_conversation_tail_outside_exchange(&compacted)?
        .unwrap_or(compacted);
    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_top_backlog_status_content(&compacted)?
    {
        compacted = reconciled;
    }
    let mut snapshot_compacted = if trailing.trim().is_empty() {
        compacted.clone()
    } else {
        let snapshot_content = comp.replace_content(content, &summary);
        agent_doc_template::repair_conversation_tail_outside_exchange(&snapshot_content)?
            .unwrap_or(snapshot_content)
    };
    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_top_backlog_status_content(
            &snapshot_compacted,
        )?
    {
        snapshot_compacted = reconciled;
    }
    apply_compacted_document(
        file,
        &compacted,
        &snapshot_compacted,
        content,
        is_crdt,
        force_disk,
    )?;
    discard_archived_captures(file, &archive_content);

    let line_count = archive_content.lines().count();
    eprintln!(
        "[compact] Archived {} lines from component '{}' to {}",
        line_count,
        target,
        archive_path.display()
    );

    Ok(snapshot_compacted)
}

/// Partial compact a named component in a template/stream-mode document.
///
/// Archives all but the last `keep` `### Re:` topic sections; rebuilds the component
/// with an archive pointer + preamble (or `message`) + kept sections.
fn run_component_compact_partial(
    file: &Path,
    content: &str,
    target: &str,
    keep: usize,
    message: Option<&str>,
    is_crdt: bool,
    force_disk: bool,
) -> Result<String> {
    let components = element::parse(content)?;
    let comp = components
        .iter()
        .find(|c| c.name == target)
        .ok_or_else(|| anyhow::anyhow!("component '{}' not found in document", target))?;

    let old_content = comp.content(content);

    let parsed = parse_topic_sections_with_tail(old_content);
    let preamble = parsed.preamble;
    let sections = parsed.sections;
    let trailing = parsed.trailing;

    if sections.len() <= keep {
        eprintln!(
            "[compact] Only {} topic section(s) found, keeping all (threshold: {})",
            sections.len(),
            keep
        );
        return Ok(content.to_string());
    }

    let to_archive = &sections[..sections.len() - keep];
    let to_keep = &sections[sections.len() - keep..];

    // Build archive: preamble + archived sections
    let mut archive_body = String::new();
    if !preamble.trim().is_empty() {
        archive_body.push_str(preamble.trim_end());
        archive_body.push_str("\n\n");
    }
    for section in to_archive {
        archive_body.push_str(section.trim_end());
        archive_body.push_str("\n\n");
    }

    let archive_path = save_archive(
        file,
        &build_component_archive(file, content, target, &archive_body),
    )?;

    // Build new component content. The committed snapshot intentionally omits
    // unresolved trailing prompt text after the boundary so compact+commit can
    // reduce history without marking that prompt as answered.
    let mut base_new_content = String::new();

    // Preamble: use --message if provided, else keep original preamble
    match message {
        Some(msg) => {
            base_new_content.push_str(msg.trim_end());
            base_new_content.push('\n');
        }
        None => {
            if !preamble.trim().is_empty() {
                base_new_content.push_str(preamble.trim_end());
                base_new_content.push('\n');
            }
        }
    }

    // Archive pointer
    base_new_content.push_str(&format!(
        "\n*{} earlier topic(s) archived to `{}`*\n",
        to_archive.len(),
        archive_path.display()
    ));

    // Kept sections
    for section in to_keep {
        base_new_content.push('\n');
        base_new_content.push_str(section.trim_end());
        base_new_content.push('\n');
    }
    let mut new_content = base_new_content.clone();
    if !trailing.trim().is_empty() {
        if !new_content.ends_with('\n') {
            new_content.push('\n');
        }
        new_content.push_str(trailing.trim_end());
        new_content.push('\n');
    }

    let compacted = comp.replace_content(content, &new_content);
    let mut compacted = agent_doc_template::repair_conversation_tail_outside_exchange(&compacted)?
        .unwrap_or(compacted);
    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_top_backlog_status_content(&compacted)?
    {
        compacted = reconciled;
    }
    let mut snapshot_compacted = if trailing.trim().is_empty() {
        compacted.clone()
    } else {
        let snapshot_content = comp.replace_content(content, &base_new_content);
        agent_doc_template::repair_conversation_tail_outside_exchange(&snapshot_content)?
            .unwrap_or(snapshot_content)
    };
    if let Some(reconciled) =
        agent_doc_document::status_projection::reconcile_top_backlog_status_content(
            &snapshot_compacted,
        )?
    {
        snapshot_compacted = reconciled;
    }
    apply_compacted_document(
        file,
        &compacted,
        &snapshot_compacted,
        content,
        is_crdt,
        force_disk,
    )?;
    discard_archived_captures(file, &archive_body);

    eprintln!(
        "[compact] Archived {} topic(s) from component '{}' to {}",
        to_archive.len(),
        target,
        archive_path.display()
    );
    eprintln!(
        "[compact] {} topic(s) remain in {}",
        to_keep.len(),
        file.display()
    );

    Ok(snapshot_compacted)
}

/// Build archive content from a component.
fn build_component_archive(
    doc: &Path,
    original: &str,
    component_name: &str,
    content: &str,
) -> String {
    build_component_archive_content(
        &compact_archive_metadata(doc, original, component_name, None),
        content,
    )
}

/// Build archive file content from exchanges.
fn build_archive(doc: &Path, original: &str, exchanges: &[CompactExchange]) -> String {
    build_inline_exchange_archive_content(
        &compact_archive_metadata(doc, original, "exchange", Some(exchanges.len())),
        exchanges,
    )
}

fn compact_archive_metadata(
    doc: &Path,
    original: &str,
    component_name: &str,
    exchange_count: Option<usize>,
) -> CompactArchiveMetadata {
    let mut metadata = CompactArchiveMetadata::component(chrono_timestamp(), component_name)
        .with_document(archive_document_value(doc).ok())
        .with_session(compact_archive_session(original));
    if let Some(exchange_count) = exchange_count {
        metadata = metadata.with_exchange_count(exchange_count);
    }
    metadata
}

/// Save archive to `.agent-doc/archives/<hash>-<timestamp>.md`.
fn save_archive(doc: &Path, content: &str) -> Result<std::path::PathBuf> {
    let snap_path = agent_doc_fs::snapshot_path_for(doc)?;
    // Extract the hash from snapshot path (filename without .md)
    let hash = snap_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("unknown");

    // Build archive dir relative to project root
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(doc)?;
    let archive_dir = project_root.join(".agent-doc/archives");
    std::fs::create_dir_all(&archive_dir)
        .with_context(|| format!("failed to create {}", archive_dir.display()))?;

    let timestamp = chrono_timestamp();
    let archive_path = archive_dir.join(format!("{}-{}.md", hash, timestamp));

    std::fs::write(&archive_path, content)
        .with_context(|| format!("failed to write {}", archive_path.display()))?;
    if let Err(err) = archive_index::index_archive(doc, &archive_path) {
        eprintln!(
            "[compact] warning: archive index update failed for {}: {}",
            archive_path.display(),
            err
        );
    }

    Ok(archive_path)
}

fn archive_document_value(doc: &Path) -> Result<String> {
    let project_root = agent_doc_project_root_io::project_root_or_file_parent(doc)?;
    let canonical = doc
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", doc.display()))?;
    Ok(canonical
        .strip_prefix(&project_root)
        .unwrap_or(&canonical)
        .to_string_lossy()
        .replace('\\', "/"))
}

/// Generate a compact timestamp for archive filenames.
fn chrono_timestamp() -> String {
    use std::time::SystemTime;
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    format_compact_timestamp_from_unix_secs(now.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_element::element::is_backlog_component;
    use agent_doc_topic::parse_topic_sections;
    use std::process::Command;

    const COMPACTDROPITEM_DOC: &str = concat!(
        "---\nagent_doc_session: drop-test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: topic one\n\nResponse one.\n",
        "<!-- /agent:exchange -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a1] item one\n",
        "- [ ] [#a2] item two\n",
        "- [ ] [#a3] item three\n",
        "<!-- /agent:backlog -->\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "- [/] [#r1] review one\n",
        "<!-- /agent:review -->\n",
    );

    fn assert_stale_compact_source_refusal(err: &anyhow::Error, ops_log: &str) {
        let err = format!("{err:#}");
        assert!(
            err.contains("document changed after the response merge was computed"),
            "compact must fail closed when the visible file changed after compaction input: {err}"
        );
        assert!(
            ops_log.contains("visible_write_deferred_current_changed")
                && ops_log.contains("source=compact")
                && !ops_log.contains("transport=disk_detached"),
            "stale compact-source refusal should be logged without detached disk write:\n{ops_log}"
        );
    }

    fn assert_editor_capability_compact_refusal(err: &anyhow::Error, ops_log: &str) {
        let err = err.to_string();
        assert!(
            err.contains("lacks required capability")
                && err.contains(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY),
            "compact with an under-capable live editor buffer must fail closed: {err}"
        );
        assert!(
            ops_log.contains("compact_writeback")
                && ops_log.contains("transport=blocked")
                && ops_log.contains("reason=editor_capability_missing")
                && ops_log.contains(agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY),
            "under-capable compact refusal should be logged:\n{ops_log}"
        );
    }

    #[test]
    fn assert_non_exchange_items_preserved_passes_when_counts_stable() {
        // Only the exchange changed; backlog/review item counts are identical.
        let after = COMPACTDROPITEM_DOC.replace("Response one.", "*Compacted.*");
        assert!(
            super::assert_non_exchange_items_preserved(
                Path::new("/tmp/compactdropitem.md"),
                COMPACTDROPITEM_DOC,
                &after,
                "test"
            )
            .is_ok()
        );
    }

    #[test]
    fn assert_non_exchange_items_preserved_fails_closed_on_dropped_backlog_item() {
        // Simulate the live #compactdropitem regression: a concurrent CRDT merge
        // drops one whole backlog item from the written document.
        let after = COMPACTDROPITEM_DOC.replace("- [ ] [#a2] item two\n", "");
        let err = super::assert_non_exchange_items_preserved(
            Path::new("/tmp/compactdropitem.md"),
            COMPACTDROPITEM_DOC,
            &after,
            "post_write",
        )
        .expect_err("dropping a backlog item must fail closed");
        let msg = err.to_string();
        assert!(msg.contains("#compactdropitem"), "message: {msg}");
        assert!(msg.contains("backlog 3→2"), "message: {msg}");
        assert!(msg.contains("agent-doc admin recycle"), "message: {msg}");
        assert!(!msg.contains("--force"), "message: {msg}");
        assert!(!msg.contains("interrupt-clear"), "message: {msg}");
    }

    #[test]
    fn run_component_compact_does_not_drop_backlog_items() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("drop.md");
        std::fs::write(&file, COMPACTDROPITEM_DOC).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, COMPACTDROPITEM_DOC, agent_doc_ops_log_io::log_op)
            .unwrap();

        // Full exchange compact must leave backlog (3) and review (1) intact and
        // must NOT trip the #compactdropitem guard.
        run_component_compact_force_disk(
            &file,
            COMPACTDROPITEM_DOC,
            "exchange",
            Some("Compacted."),
            false,
        )
        .unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let counts = agent_doc_element_backlog::backlog::tracked_component_item_counts(&result);
        assert_eq!(counts.get("backlog").copied(), Some(3));
        assert_eq!(counts.get("review").copied(), Some(1));
    }

    #[test]
    fn build_archive_format() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc_path = dir.path().join("session.md");
        std::fs::write(&doc_path, "---\nsession: test\n---\n").unwrap();
        let exchanges = vec![CompactExchange {
            user: "Hello".to_string(),
            assistant: "Hi there".to_string(),
        }];
        let archive = build_archive(&doc_path, "---\nsession: test\n---\n", &exchanges);
        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("session: test"));
        assert!(archive.contains("## User\n\nHello"));
        assert!(archive.contains("## Assistant\n\nHi there"));
    }

    #[test]
    fn chrono_timestamp_format() {
        let ts = chrono_timestamp();
        // Should be YYYYMMDD-HHMMSS format
        assert_eq!(ts.len(), 15);
        assert_eq!(&ts[8..9], "-");
    }

    #[test]
    fn build_component_archive_format() {
        let doc = "---\nagent_doc_session: abc-123\nagent_doc_mode: stream\n---\n\n<!-- agent:exchange -->\nOld conversation\n<!-- /agent:exchange -->\n";
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc_path = dir.path().join("docs/session.md");
        std::fs::create_dir_all(doc_path.parent().unwrap()).unwrap();
        std::fs::write(&doc_path, doc).unwrap();
        let archive = build_component_archive(&doc_path, doc, "exchange", "\nOld conversation\n");
        assert!(archive.contains("archived_from: compact"));
        assert!(archive.contains("component: exchange"));
        assert!(archive.contains("document: docs/session.md"));
        assert!(archive.contains("session: abc-123"));
        assert!(archive.contains("Old conversation"));
    }

    #[test]
    fn parse_topic_sections_basic() {
        let content = "### Session Summary\n\nSome preamble.\n\n### Re: first topic\n\nFirst response.\n\n### Re: second topic\n\nSecond response.\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert!(preamble.contains("Session Summary"));
        assert!(preamble.contains("Some preamble."));
        assert_eq!(sections.len(), 2);
        assert!(sections[0].starts_with("### Re: first topic"));
        assert!(sections[0].contains("First response."));
        assert!(sections[1].starts_with("### Re: second topic"));
        assert!(sections[1].contains("Second response."));
    }

    #[test]
    fn parse_topic_sections_keep_threshold() {
        let content = "### Re: topic 1\nResponse 1.\n### Re: topic 2\nResponse 2.\n";
        let (_, sections) = parse_topic_sections(content);
        // 2 sections ≤ keep=3 → no-op when called from run_component_compact_partial
        assert_eq!(sections.len(), 2);
    }

    #[test]
    fn parse_topic_sections_strips_boundary_marker() {
        let content = "### Re: last topic\n\nContent.\n<!-- agent:boundary:abc123 -->\n";
        let (_, sections) = parse_topic_sections(content);
        assert_eq!(sections.len(), 1);
        assert!(!sections[0].contains("agent:boundary"));
    }

    #[test]
    fn parse_topic_sections_no_re_headings() {
        let content = "Just preamble text.\nNo Re: headings here.\n";
        let (preamble, sections) = parse_topic_sections(content);
        assert!(preamble.contains("Just preamble text."));
        assert_eq!(sections.len(), 0);
    }

    #[test]
    fn partial_compact_preserves_trailing_prompt_after_boundary() {
        let doc = concat!(
            "---\nagent_doc_session: test-tail\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\nExisting summary.\n\n",
            "### Re: first topic\n\nResponse one.\n\n",
            "### Re: second topic\n\nResponse two.\n",
            "<!-- agent:boundary:abc123 -->\n",
            "do #autocmp. spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_partial(&file, doc, "exchange", 1, None, false, true).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange = agent_doc_element::element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("### Re: second topic"));
        assert!(!exchange.contains("### Re: first topic"));
        assert!(exchange.contains("do #autocmp. spec-test-build-install-commit-push"));

        let snapshot_after = agent_doc_snapshot_io::load(&file).unwrap().unwrap();
        assert!(
            !snapshot_after.contains("do #autocmp. spec-test-build-install-commit-push"),
            "unresolved trailing prompt must remain live drift after compact, not committed snapshot state:\n{snapshot_after}"
        );
    }

    #[test]
    fn full_exchange_compact_preserves_trailing_prompt_after_boundary() {
        let prompt = "do #fullcmp. spec-test-build-install-commit-push";
        let doc = format!(
            concat!(
                "---\nagent_doc_session: test-full-tail\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Session Summary\n\nExisting summary.\n\n",
                "### Re: archived topic\n\nResponse body.\n",
                "<!-- agent:boundary:abc123 -->\n",
                "{prompt}\n",
                "<!-- /agent:exchange -->\n",
            ),
            prompt = prompt
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, &doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_force_disk(&file, &doc, "exchange", None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange = agent_doc_element::element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("*Compacted. Content archived to `"));
        assert!(exchange.contains(prompt));
        assert!(
            !exchange.contains("Trailing prompt/context"),
            "compact summary must not summarize unresolved live prompt text:\n{exchange}"
        );

        let snapshot_after = agent_doc_snapshot_io::load(&file).unwrap().unwrap();
        assert!(
            !snapshot_after.contains(prompt),
            "unresolved trailing prompt must remain live drift after full compact, not committed snapshot state:\n{snapshot_after}"
        );

        let archive_dir = agent_doc_dir.join("archives");
        let archive_path = std::fs::read_dir(&archive_dir)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .find(|path| path.extension().is_some_and(|ext| ext == "md"))
            .expect("compact should write an archive");
        let archive = std::fs::read_to_string(archive_path).unwrap();
        assert!(
            !archive.contains(prompt),
            "unresolved trailing prompt must not be archived:\n{archive}"
        );
    }

    #[test]
    fn component_compact_preserves_non_target_components() {
        let doc = concat!(
            "---\nagent_doc_session: test-123\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Status: active\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nLong response about topic one.\n\n",
            "### Re: topic two\n\nLong response about topic two.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending\n\n",
            "<!-- agent:pending -->\n",
            "- Task A: do something important\n",
            "- Task B: do something else\n",
            "- Task C: critical item\n",
            "<!-- /agent:pending -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        // Set up .agent-doc dirs
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        // Capture pending content before compact
        let components_before = element::parse(doc).unwrap();
        let pending_before = components_before
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(doc)
            .to_string();
        let status_before = components_before
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(doc)
            .to_string();

        // Run compact on exchange only
        run_component_compact_force_disk(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap();

        // Read the result and verify non-target components are byte-identical
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = element::parse(&result).unwrap();
        let pending_after = components_after
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&result)
            .to_string();
        let status_after = components_after
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(
            pending_before, pending_after,
            "pending component must be byte-identical after compact"
        );
        assert_eq!(
            status_before, status_after,
            "status component must be byte-identical after compact"
        );

        // Verify exchange was actually compacted
        let exchange_after = components_after
            .iter()
            .find(|c| c.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();
        assert!(exchange_after.contains("Compacted summary."));
        assert!(!exchange_after.contains("### Re: topic one"));
    }

    #[test]
    fn component_compact_preserves_summary_leading_code_fence() {
        let doc = concat!(
            "---\nagent_doc_session: test-compact-fence\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ show fenced prompt\n",
            "```\n",
            "prompt body\n",
            "```\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_force_disk(
            &file,
            doc,
            "exchange",
            Some("```\ncompacted summary\n```"),
            false,
        )
        .unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange_after = agent_doc_element::element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(
            exchange_after.matches("```").count(),
            2,
            "compact summary fences must survive:\n{exchange_after}"
        );
        assert!(
            exchange_after.starts_with("```\ncompacted summary\n```\n"),
            "compact summary leading fence must remain first content:\n{exchange_after}"
        );
    }

    #[test]
    fn component_compact_preserves_post_exchange_scratch_comment() {
        let prompt = "The compact exchange scratch comment should not be deleted. #spec-test-build-install-commit-push";
        let doc = format!(
            concat!(
                "---\nagent_doc_session: test-compact-scratch\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "❯ {prompt}\n",
                "### Re: topic one\n\nResponse one.\n\n",
                "### Re: topic two\n\nResponse two.\n",
                "<!-- agent:boundary:head -->\n",
                "<!-- /agent:exchange -->\n\n",
                "###\n\n",
                "<!--\n",
                "{prompt}\n",
                "#spec-test-build-install-commit-push\n",
                "---\n",
                "Keep compact scratch notes visible.\n",
                "-->\n\n",
                "<!-- agent:backlog -->\n",
                "- [ ] [#aaaa] keep me\n",
                "<!-- /agent:backlog -->\n"
            ),
            prompt = prompt
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, &doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_force_disk(
            &file,
            &doc,
            "exchange",
            Some("Compacted summary."),
            false,
        )
        .unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        assert!(
            result.contains(&format!(
                "<!--\n{prompt}\n#spec-test-build-install-commit-push\n---\nKeep compact scratch notes visible.\n-->"
            )),
            "compact exchange must leave post-exchange scratch comments outside the compacted component:\n{result}"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&file).unwrap().unwrap();
        assert!(
            snapshot_after.contains("Keep compact scratch notes visible."),
            "compact snapshot should preserve owned post-exchange scratch comments:\n{snapshot_after}"
        );
    }

    #[test]
    fn component_compact_uses_guarded_direct_write_when_patches_dir_exists() {
        let doc = concat!(
            "---\nagent_doc_session: test-ipc\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        let patches_dir = agent_doc_dir.join("patches");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(&patches_dir).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap();
        let compacted = std::fs::read_to_string(&file).unwrap();
        let patch_count = std::fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count();

        assert!(compacted.contains("Compacted summary."));
        assert!(!compacted.contains("### Re: topic one"));
        assert_eq!(patch_count, 0, "compact must not emit fullContent IPC");
        assert_eq!(
            agent_doc_snapshot_io::load(&file).unwrap().unwrap(),
            compacted
        );
    }

    #[test]
    fn component_compact_direct_write_is_not_blocked_by_previous_cycle_committed() {
        let doc = concat!(
            "---\nagent_doc_session: test-ipc-committed\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        let patches_dir = agent_doc_dir.join("patches");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        std::fs::create_dir_all(&patches_dir).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_cycle_state_io::start_preflight(&file, Some(doc), Some(doc)).unwrap();
        agent_doc_cycle_state_io::mark_response_captured(
            &file,
            "test",
            Some(doc),
            Some(doc),
            "fake-sha",
            None,
        )
        .unwrap();
        agent_doc_cycle_state_io::mark_write_applied(&file, "test", Some(doc), Some(doc)).unwrap();
        agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
            &crate::PIPELINE_FRONTMATTER_EFFECTS,
            &file,
            "test",
            Some(doc),
            Some(doc),
        )
        .unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap();
        let compacted = std::fs::read_to_string(&file).unwrap();
        let patch_count = std::fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count();

        assert!(compacted.contains("Compacted summary."));
        assert!(!compacted.contains("### Re: topic one"));
        assert_eq!(patch_count, 0, "compact must not emit fullContent IPC");
        assert_eq!(
            agent_doc_snapshot_io::load(&file).unwrap().unwrap(),
            compacted
        );
        let ops_log =
            std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            !ops_log.contains("late_fallback_patch_rejected"),
            "operator compact is not a stale response fallback"
        );
    }

    #[test]
    fn component_compact_detached_disk_rejects_late_visible_edit() {
        let doc = concat!(
            "---\nagent_doc_session: test-compact-cas\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n",
        );
        let live = doc.replace(
            "<!-- /agent:exchange -->",
            "live prompt typed during compact\n<!-- /agent:exchange -->",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &live).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            live,
            "detached-disk compact must not overwrite a prompt typed after compaction was computed"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&file).unwrap().unwrap(),
            doc,
            "failed compact must not advance the snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert_stale_compact_source_refusal(&err, &ops_log);
    }

    #[test]
    fn component_compact_without_listener_rejects_idle_unsaved_editor_buffer() {
        let doc = concat!(
            "---\nagent_doc_session: test-compact-live-buffer\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n",
        );
        let live_buffer = doc.replace(
            "<!-- /agent:exchange -->",
            "prompt typed in JetBrains but not saved\n<!-- /agent:exchange -->",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();
        let file_str = file.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &file_str,
            live_buffer.len(),
            &agent_doc_hash::content_hash(&live_buffer),
        )
        .unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            doc,
            "no-listener compact must not rewrite disk while the editor-visible buffer is unsaved"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&file).unwrap().unwrap(),
            doc,
            "failed compact must not advance the snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert_editor_capability_compact_refusal(&err, &ops_log);
    }

    #[test]
    fn component_compact_without_listener_rejects_stale_editor_cache_when_snapshot_is_stale() {
        let stale_snapshot = concat!(
            "---\nagent_doc_session: test-compact-stale-cache\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n",
        );
        let current = stale_snapshot.replace(
            "<!-- /agent:exchange -->",
            "### Re: topic two\n\nResponse two.\n<!-- /agent:exchange -->",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &current).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("live-buffer")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        agent_doc_snapshot_io::save(&file, stale_snapshot, agent_doc_ops_log_io::log_op).unwrap();
        let file_str = file.canonicalize().unwrap().to_string_lossy().to_string();
        agent_doc_debounce::record_live_buffer_digest(
            &file_str,
            stale_snapshot.len(),
            &agent_doc_hash::content_hash(stale_snapshot),
        )
        .unwrap();

        let err = run_component_compact(
            &file,
            &current,
            "exchange",
            Some("Compacted summary."),
            false,
        )
        .unwrap_err();

        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            current,
            "compact must not overwrite the current file when JetBrains still advertises stale cache content"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&file).unwrap().unwrap(),
            stale_snapshot,
            "failed compact must not advance a stale snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert_editor_capability_compact_refusal(&err, &ops_log);
    }

    #[test]
    fn component_compact_empty_message_advances_snapshot_after_exchange_clear() {
        // #clearexchstale: a successful exchange clear must advance the durable
        // merge base, otherwise later preflight/repair paths can replay the
        // stale pre-clear exchange from the snapshot.
        let doc = concat!(
            "---\nagent_doc_session: test-clear-exchange\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: stale topic\n\nThis response should stay archived after clear.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some(""), false).unwrap();

        let cleared = std::fs::read_to_string(&file).unwrap();
        assert!(
            !cleared.contains("stale topic"),
            "cleared exchange must not leave stale content in the visible file:\n{cleared}"
        );
        let snapshot_after = agent_doc_snapshot_io::load(&file).unwrap().unwrap();
        assert_eq!(
            snapshot_after, cleared,
            "successful exchange clear must advance the snapshot merge base to the cleared document"
        );
        assert!(
            !snapshot_after.contains("stale topic"),
            "snapshot merge base must not retain stale pre-clear exchange content:\n{snapshot_after}"
        );
    }

    #[test]
    fn compact_advances_snapshot_and_crdt_so_next_preflight_does_not_refuse() {
        // #compact-overlay-crdt-staleness: a CRDT-mode full compact must leave the
        // overlay CRDT projecting the COMPACTED (small) document, not the
        // pre-compaction (large) one. The earlier defect saved the overlay with
        // the large `&content`, so later cycles re-projected snapshot(large) >
        // visible(small) and `guard_no_stale_snapshot_reset_drift` refused the
        // commit as a "manual cleanup".
        let mut exchange = String::new();
        for i in 0..40 {
            exchange.push_str(&format!(
                "### Re: topic {i}\n\nThis is a fairly long archived response body number {i} that should be removed by compaction and must not survive in the overlay CRDT projection after the compact completes.\n\n"
            ));
        }
        let doc = format!(
            "---\nagent_doc_session: test-crdt-compact\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\n{exchange}<!-- /agent:exchange -->\n"
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        agent_doc_snapshot_io::save(&file, &doc, agent_doc_ops_log_io::log_op).unwrap();
        // Seed both the legacy and overlay CRDT sidecars from the large document,
        // mirroring a live CRDT session before compaction.
        let legacy = agent_doc_merge::crdt::CrdtDoc::from_text(&doc).encode_state();
        agent_doc_merge_io::save_document_crdt(&file, &legacy, &doc).unwrap();

        // Full compact (keep=None) in CRDT mode.
        run_component_compact_force_disk(&file, &doc, "exchange", Some("Session compacted."), true)
            .unwrap();

        let visible = std::fs::read_to_string(&file).unwrap();
        assert!(
            visible.len() < doc.len(),
            "compact must shrink the visible document: {} >= {}",
            visible.len(),
            doc.len()
        );
        assert!(
            !visible.contains("topic 0"),
            "compacted visible file must not retain archived topics:\n{visible}"
        );

        // The snapshot must advance to the compacted document (no archived topics
        // and small relative to the original).
        let snap = agent_doc_snapshot_io::load(&file).unwrap().unwrap();
        assert!(
            !snap.contains("topic 0"),
            "snapshot merge base must not retain archived topics:\n{snap}"
        );
        assert!(
            snap.len() < doc.len(),
            "snapshot must shrink to the compacted document: {} >= {}",
            snap.len(),
            doc.len()
        );

        // The overlay CRDT projection must equal the COMPACTED text, not the
        // original large document.
        let overlay_bytes = agent_doc_snapshot_io::load_overlay_crdt(&file)
            .unwrap()
            .unwrap();
        let projected = agent_doc_markdown_ast::crdt::OverlayCrdtDoc::decode_state(&overlay_bytes)
            .unwrap()
            .to_markdown()
            .unwrap();
        assert!(
            !projected.contains("topic 0"),
            "overlay CRDT must project the compacted document, not the pre-compaction one:\n{projected}"
        );
        assert_eq!(
            projected, visible,
            "overlay CRDT projection must equal the compacted visible document"
        );

        // With the overlay advanced, the stale-snapshot drift guard must NOT bail.
        // (Before the fix, the overlay carried the large doc and a re-projected
        // snapshot would trip the "manual cleanup" refusal.)
        runtime_effects()
            .unwrap()
            .guard_no_stale_snapshot_reset_drift(
                &file,
                Some(projected.as_str()),
                &visible,
                "commit",
            )
            .expect(
                "compacted overlay/snapshot must not trip the stale-snapshot reset-drift guard",
            );
    }

    #[test]
    fn component_compact_detached_disk_rejects_late_post_exchange_scratch_comment() {
        let prompt = "The post-exchange scratch comment was typed while compact exchange was being computed. #spec-test-build-install-commit-push";
        let doc = concat!(
            "---\nagent_doc_session: test-compact-comment-cas\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "-->\n"
        );
        let live = doc.replace("<!--\n-->", &format!("<!--\n{prompt}\n-->"));

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &live).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            live,
            "detached-disk compact must not overwrite scratch comments typed after compaction was computed"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&file).unwrap().unwrap(),
            doc,
            "failed compact must not advance the snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert_stale_compact_source_refusal(&err, &ops_log);
    }

    #[test]
    fn component_compact_rejects_cycle_1779845677327_scratch_directive_race() {
        let scratch_prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Reproduce bugs with tests first that fail and fix the implementation.";
        let scratch_directive = "#spec-test-build-install-commit-push";
        let scratch_dispatch = "dispatch #spec-test-build-install-commit-push";
        let doc = concat!(
            "---\nagent_doc_session: cycle-1779845677327\nagent_doc_format: template\n",
            "prompt_presets:\n",
            "  '#spec-test-build-install-commit-push': update spec + tests. build + install for local testing. commit + push\n",
            "---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "-->\n\n",
            "<!-- agent:queue auto -->\n",
            "dispatch #spec-test-build-install-commit-push\n",
            "- do [#liveipcrace]\n",
            "<!-- /agent:queue -->\n"
        );
        let live_scratch =
            format!("<!--\n{scratch_prompt}\n{scratch_directive}\n---\n{scratch_dispatch}\n-->");
        let live = doc.replace("<!--\n-->", &live_scratch);

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, &live).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        let patches_dir = agent_doc_dir.join("patches");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        std::fs::create_dir_all(&patches_dir).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        let file_after = std::fs::read_to_string(&file).unwrap();
        assert_eq!(
            file_after, live,
            "detached-disk compact must not overwrite cycle-1779845677327 scratch directives"
        );
        assert_eq!(
            file_after.matches(scratch_prompt).count(),
            1,
            "scratch prompt text must not be duplicated or deleted:\n{file_after}"
        );
        assert_eq!(
            file_after.matches(&live_scratch).count(),
            1,
            "prompt preset and dispatch directives in the scratch comment must remain intact:\n{file_after}"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&file).unwrap().unwrap(),
            doc,
            "failed compact must not advance the snapshot to the shorter or live buffer"
        );
        let patch_count = std::fs::read_dir(&patches_dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|value| value.to_str()) == Some("json")
            })
            .count();
        assert_eq!(
            patch_count, 0,
            "compact race handling must not emit file IPC or fullContent payloads"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert_stale_compact_source_refusal(&err, &ops_log);
        assert!(
            !ops_log.contains("snapshot_absorb"),
            "compact race handling must not silently absorb a shorter disk snapshot:\n{ops_log}"
        );
    }

    #[test]
    fn exchange_compact_default_summary_includes_archived_content_digest() {
        let doc = concat!(
            "---\nagent_doc_session: test-summary\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue -->\n",
            "- do #next. run targeted test\n",
            "- do #later. build and install\n",
            "<!-- /agent:queue -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#rpaj] Make compact exchange synthesize backlog-aware context\n",
            "- [/release] [#ship] Wait for release window\n",
            "- [x] [#done] Already handled\n",
            "<!-- /agent:backlog -->\n\n",
            "## Icebox\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#parked] Parked follow-up for later\n",
            "<!-- /agent:icebox -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let components = element::parse(&result).unwrap();
        let exchange = components
            .iter()
            .find(|c| c.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("### Session Summary"));
        assert!(exchange.contains("*Compacted. Content archived to `"));
        assert!(exchange.contains("Compacted content:"));
        assert!(exchange.contains("Archived 2 response topic(s): topic one; topic two"));
        assert!(!exchange.contains("Active backlog:"));
        assert!(!exchange.contains("[#rpaj]"));
        assert!(!exchange.contains("Queue:"));
        assert!(!exchange.contains("Icebox:"));
        assert!(!exchange.contains("### Re: topic one"));
    }

    #[test]
    fn exchange_compact_default_summary_does_not_replay_prior_compact_lists() {
        let doc = concat!(
            "---\nagent_doc_session: test-summary\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "*Compacted. Content archived to `.agent-doc/archives/previous.md`*\n\n",
            "Compacted content:\n",
            "- Archived 1 response topic(s): lender bank wire instruction reveal\n",
            "- Prior summary/context: ### Session Summary *Compacted. Content archived to `.agent-doc/archives/older.md`*\n",
            "8. **Active Funding**: wire confirmed.\n",
            "7. **Committed / Awaiting Wire**: lender committed.\n",
            "6. **Ready to Commit**: lender account is linked.\n",
            "5. **Lender Setup**: admin needs setup.\n",
            "4. **Accreditation Current**: sample portal approved.\n",
            "3. **Accreditation Under Review**: submitted.\n",
            "2. **Accreditation Not Started**: lender still needs to submit.\n",
            "1. **Signed Up**: account exists.\n",
            "- **Active**: at least one fund row is active.\n",
            "- **Committed**: the lender has a pending commitment.\n",
            "- **Lender Ready**: a lender account is linked.\n",
            "8. **Active Funding**: wire confirmed.\n",
            "7. **Committed / Awaiting Wire**: lender committed.\n",
            "6. **Ready to Commit**: lender account is linked.\n",
            "5. **Lender Setup**: admin needs setup.\n",
            "4. **Accreditation Current**: sample portal approved.\n",
            "3. **Accreditation Under Review**: submitted.\n",
            "2. **Accreditation Not Started**: lender still needs to submit.\n",
            "1. **Signed Up**: account exists.\n\n",
            "### Re: investor pipeline resequence — gpt-5\n\n",
            "Suggested lane sequence:\n\n",
            "1. **Signed Up**: account exists.\n",
            "2. **Accreditation Not Started**: lender still needs to submit.\n",
            "3. **Accreditation Under Review**: submitted.\n",
            "4. **Accreditation Current**: sample portal approved.\n",
            "5. **Lender Setup**: admin needs setup.\n",
            "6. **Ready to Commit**: lender account is linked.\n",
            "7. **Committed / Awaiting Wire**: lender committed.\n",
            "8. **Active Funding**: wire confirmed.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange = element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|c| c.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        assert!(exchange.contains("Archived 1 response topic(s): investor pipeline resequence"));
        assert!(exchange.contains(
            "Prior summary/context: prior compacted content: Archived 1 response topic(s): lender bank wire instruction reveal"
        ));
        assert!(
            !exchange.contains("Prior summary/context: ### Session Summary"),
            "prior compact summaries must not be recursively embedded:\n{exchange}"
        );
        assert!(
            !exchange.contains("8. **Active Funding**"),
            "ordered-list response details must stay in the archive, not the compact digest:\n{exchange}"
        );
        assert!(
            !exchange.contains("- **Lender Ready**"),
            "duplicated prior-context bullets must stay out of the compact digest:\n{exchange}"
        );
    }

    #[test]
    fn exchange_compact_reconciles_stale_top_backlog_status() {
        let doc = concat!(
            "---\nagent_doc_session: test-summary\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "Top backlog item: #done.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "<!-- /agent:backlog -->\n"
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some("Compacted."), false)
            .unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        assert!(result.contains("No open backlog items."));
        assert!(!result.contains("Top backlog item: #done."));
        let snap = agent_doc_snapshot_io::load(&file).unwrap().unwrap();
        assert!(snap.contains("No open backlog items."));
    }

    #[test]
    fn crdt_compact_preserves_pending_with_state_refresh() {
        // Test that CRDT state refresh prevents pending items from being lost
        let doc = concat!(
            "---\nagent_doc_session: test-crdt-123\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n\n",
            "### Re: topic three\n\nResponse three.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Pending\n\n",
            "<!-- agent:pending -->\n",
            "- ✅ completed task\n",
            "- 🔄 in-progress work\n",
            "- 🆕 new task to add\n",
            "<!-- /agent:pending -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        // Set up .agent-doc dirs and save initial CRDT state
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        // Create and save initial CRDT state
        let initial_crdt = agent_doc_merge::crdt::CrdtDoc::from_text(doc).encode_state();
        agent_doc_merge_io::save_document_crdt(&file, &initial_crdt, doc).unwrap();

        // Capture pending before compact
        let components_before = element::parse(doc).unwrap();
        let pending_before = components_before
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(doc)
            .to_string();

        // Run compact with CRDT mode enabled (is_crdt=true)
        run_component_compact_force_disk(&file, doc, "exchange", Some("Compacted."), true).unwrap();

        // Read result and verify pending survives
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = element::parse(&result).unwrap();
        let pending_after = components_after
            .iter()
            .find(|c| is_backlog_component(&c.name))
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(
            pending_before, pending_after,
            "pending component must survive CRDT state refresh during compact"
        );
        assert!(pending_after.contains("completed task"));
        assert!(pending_after.contains("in-progress work"));
        assert!(pending_after.contains("new task to add"));
    }

    #[test]
    fn compact_preserves_boundary_marker() {
        // Test that boundary markers (❯) survive compact operations
        let doc = concat!(
            "---\nagent_doc_session: test-boundary\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first topic\n\nResponse one.\n\n",
            "### Re: second topic\n\nResponse two.\n",
            "<!-- agent:boundary:abc123def456 -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "❯ Critical item: verify preservation\n",
            "<!-- /agent:status -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        // Capture status with ❯ before compact
        let components_before = element::parse(doc).unwrap();
        let status_before = components_before
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(doc)
            .to_string();

        run_component_compact_force_disk(&file, doc, "exchange", Some("Archived."), false).unwrap();

        // Verify ❯ marker preserved in non-target component
        let result = std::fs::read_to_string(&file).unwrap();
        let components_after = element::parse(&result).unwrap();
        let status_after = components_after
            .iter()
            .find(|c| c.name == "status")
            .unwrap()
            .content(&result)
            .to_string();

        assert_eq!(status_before, status_after);
        assert!(status_after.contains("❯"));
        assert!(status_after.contains("Critical item"));
    }

    /// `#compactqattr`: compacting the exchange must leave every non-exchange
    /// component opening marker byte-identical, including inline attributes
    /// (`priority`, `preset="..."`, `go`) on an `agent:queue` marker. Operator
    /// observed (sampleorders.md) post-compaction wiping all `agent:queue`
    /// attributes ("too much blast radius").
    #[test]
    fn compact_preserves_queue_marker_inline_attributes() {
        let doc = concat!(
            "---\nagent_doc_session: test-qattr\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first topic\n\nResponse one.\n\n",
            "### Re: second topic\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Queue\n\n",
            "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->\n",
            "- do [#aaa]\n",
            "<!-- /agent:queue -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("crdt")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        // The verbatim opening marker line, captured before compaction.
        let queue_marker =
            "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->";
        assert!(doc.contains(queue_marker));

        run_component_compact_force_disk(&file, doc, "exchange", Some("Archived."), true).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        assert!(
            result.contains(queue_marker),
            "queue opening marker with inline attributes must survive compaction byte-identical; got:\n{result}"
        );

        // The post-compact CRDT refresh must also preserve the marker verbatim:
        // a later stale-supervisor merge bootstraps from this CRDT state.
        let crdt_path = agent_doc_dir.join("crdt").join(format!(
            "{}.yrs",
            agent_doc_fs::document_state_hash(&file).unwrap()
        ));
        if let Ok(bytes) = std::fs::read(&crdt_path) {
            let round_tripped = agent_doc_merge::crdt::CrdtDoc::decode_state(&bytes)
                .unwrap()
                .to_text();
            assert!(
                round_tripped.contains(queue_marker),
                "post-compact CRDT state must round-trip the queue marker verbatim; got:\n{round_tripped}"
            );
        }
    }

    /// `#compactqattr`: the marker guard must fail closed when a rebuild strips
    /// inline attributes from a non-exchange opening marker.
    #[test]
    fn marker_guard_fires_when_queue_attributes_dropped() {
        let before = concat!(
            "<!-- agent:exchange -->\nx\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue priority preset=\"#x\" go -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );
        // Simulated bad rebuild: queue marker lost all inline attributes.
        let after = concat!(
            "<!-- agent:exchange -->\ny\n<!-- /agent:exchange -->\n",
            "<!-- agent:queue -->\n- do [#a]\n<!-- /agent:queue -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::create_dir_all(dir.path().join(".agent-doc").join("logs")).unwrap();

        // Identical markers pass.
        assert!(assert_non_exchange_markers_preserved(&file, before, before, "test").is_ok());
        // Dropped attributes fail closed.
        let err = assert_non_exchange_markers_preserved(&file, before, after, "test").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("#compactqattr"), "got: {msg}");
        assert!(msg.contains("queue"), "got: {msg}");
        assert!(msg.contains("agent-doc admin recycle"), "got: {msg}");
        assert!(!msg.contains("--force"), "got: {msg}");
        assert!(!msg.contains("interrupt-clear"), "got: {msg}");
    }

    #[test]
    fn compact_working_tree_consistency() {
        // Test that compact leaves working tree in consistent state
        // (file unchanged vs disk, snapshot updated, no stale CRDT)
        let doc = concat!(
            "---\nagent_doc_session: test-wt\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic A\n\nResponse A.\n\n",
            "### Re: topic B\n\nResponse B.\n",
            "<!-- /agent:exchange -->\n",
        );

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();

        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        let file_before = std::fs::read_to_string(&file).unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some("Summary."), false).unwrap();

        // After compact: file and snapshot should match
        let file_after = std::fs::read_to_string(&file).unwrap();
        let snap_path = agent_doc_fs::snapshot_path_for(&file).unwrap();
        let snapshot_content = std::fs::read_to_string(&snap_path).unwrap();

        assert_eq!(
            file_after, snapshot_content,
            "file and snapshot must match after compact"
        );

        // Verify the document was actually modified
        assert_ne!(file_before, file_after);
        assert!(file_after.contains("Summary."));
    }

    #[test]
    fn compact_with_commit_writes_vcs_refresh_signal() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        std::process::Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.email", "test@example.com"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["config", "user.name", "Test"])
            .output()
            .unwrap();

        let readme = root.join("README.md");
        fs::write(&readme, "# test\n").unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "README.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "initial", "--no-verify"])
            .output()
            .unwrap();

        let file = root.join("session.md");
        let doc = concat!(
            "---\nagent_doc_session: test-compact\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&file, doc).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["add", "session.md"])
            .output()
            .unwrap();
        std::process::Command::new("git")
            .current_dir(root)
            .args(["commit", "-m", "add doc", "--no-verify"])
            .output()
            .unwrap();

        run(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            true,
            true,
        )
        .unwrap();

        let signal = root.join(".agent-doc/patches/vcs-refresh.signal");
        assert!(
            signal.exists(),
            "expected VCS refresh signal at {}",
            signal.display()
        );

        let log = std::process::Command::new("git")
            .current_dir(root)
            .args(["log", "--oneline", "-n", "1", "--", "session.md"])
            .output()
            .unwrap();
        let log = String::from_utf8_lossy(&log.stdout);
        assert!(
            log.contains("agent-doc(session):"),
            "compact closeout should use agent-doc commit, got:\n{log}"
        );

        let committed = agent_doc_git_io::revision::show_head(&file)
            .unwrap()
            .unwrap();
        assert!(committed.contains("Compacted summary."));
        assert!(!committed.contains("### Re: topic one"));
    }

    #[test]
    fn compact_dirty_treats_diverged_snapshot_as_committable() {
        use agent_doc_snapshot_io::SnapshotCommitStatus;
        // `#jb-compact-commit-editor-ipc-async`: the regression. When the compact
        // converged through the live editor IPC, the on-disk re-read still equals
        // the pre-compact content (`changed_on_disk == false`), but the
        // synchronously-saved compacted snapshot already diverges from HEAD. The
        // commit gate MUST treat that as dirty — the old gate keyed only off
        // `changed_on_disk` and silently skipped the `--commit` closeout, leaving
        // the compacted document uncommitted.
        assert!(
            compact_dirty(
                false,
                &SnapshotCommitStatus::SnapshotDiffersFromHead {
                    snapshot_len: 15_000,
                    head_len: 33_000,
                }
            ),
            "diverged snapshot with an unflushed disk file must still be committable"
        );
        // Disk already shows the change → dirty regardless of snapshot status.
        assert!(compact_dirty(true, &SnapshotCommitStatus::Committed));
        // Snapshot matches HEAD and disk unchanged → nothing to commit.
        assert!(!compact_dirty(false, &SnapshotCommitStatus::Committed));
        // No git HEAD to diverge from → fall back to the on-disk comparison.
        assert!(!compact_dirty(false, &SnapshotCommitStatus::NotInGitRepo));
        assert!(compact_dirty(true, &SnapshotCommitStatus::NotInGitRepo));
    }

    fn init_compact_test_repo(root: &std::path::Path) {
        use std::fs;
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        for args in [
            vec!["init"],
            vec!["config", "user.email", "test@example.com"],
            vec!["config", "user.name", "Test"],
        ] {
            std::process::Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
    }

    fn git_commit_file(root: &std::path::Path, name: &str) {
        for args in [
            vec!["add", name],
            vec!["commit", "-m", "commit", "--no-verify"],
        ] {
            std::process::Command::new("git")
                .current_dir(root)
                .args(&args)
                .output()
                .unwrap();
        }
    }

    const PRECOMPACT_DOC: &str = concat!(
        "---\nagent_doc_session: test-lag\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: topic one\n\nResponse one.\n\n",
        "### Re: topic two\n\nResponse two.\n",
        "<!-- /agent:exchange -->\n",
    );
    const COMPACTED_DOC: &str = concat!(
        "---\nagent_doc_session: test-lag\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "*Compacted. Content archived.*\n",
        "<!-- /agent:exchange -->\n",
    );

    #[test]
    fn compact_commit_lands_head_when_snapshot_replayed_stale() {
        use std::fs;
        // `#jb-compact-commit-left-uncommitted`, real incident: a stale-supervisor
        // CRDT replay reverted the on-disk snapshot back to pre-compact after the
        // compaction converged the compacted content to the editor + disk. HEAD =
        // pre-compact, disk = compacted, snapshot = pre-compact (replayed). The
        // authoritative-content commit must re-assert the compacted snapshot and
        // land it in HEAD instead of staging the stale snapshot.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_compact_test_repo(root);

        let file = root.join("session.md");
        fs::write(&file, PRECOMPACT_DOC).unwrap();
        agent_doc_snapshot_io::save(&file, PRECOMPACT_DOC, agent_doc_ops_log_io::log_op).unwrap();
        git_commit_file(root, "session.md"); // HEAD = pre-compact

        // Editor/plugin flushed the compacted content to disk...
        fs::write(&file, COMPACTED_DOC).unwrap();
        // ...but a stale-supervisor CRDT replay reverted the snapshot to pre-compact.
        agent_doc_snapshot_io::save(&file, PRECOMPACT_DOC, agent_doc_ops_log_io::log_op).unwrap();

        // Authoritative content is known in run() from the compaction itself.
        commit_compacted_authoritative(&file, COMPACTED_DOC).unwrap();

        let committed = agent_doc_git_io::revision::show_head(&file)
            .unwrap()
            .unwrap();
        assert!(
            committed.contains("*Compacted. Content archived.*"),
            "HEAD must hold the compacted content after --commit, got:\n{committed}"
        );
        assert!(
            !committed.contains("### Re: topic one"),
            "pre-compact content must not remain in HEAD:\n{committed}"
        );
    }

    #[test]
    fn compact_commit_fails_closed_when_head_cannot_land() {
        use std::fs;
        // `#jb-compact-commit-left-uncommitted`, fail-closed path: when the
        // working-tree file still equals HEAD (a pure editor-IPC lag with no flush),
        // the selective commit cannot safely stage the compacted content, so the
        // authoritative-content commit must FAIL CLOSED with a recovery command
        // rather than silently leaving uncommitted compaction drift.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_compact_test_repo(root);

        let file = root.join("session.md");
        fs::write(&file, PRECOMPACT_DOC).unwrap();
        agent_doc_snapshot_io::save(&file, PRECOMPACT_DOC, agent_doc_ops_log_io::log_op).unwrap();
        git_commit_file(root, "session.md"); // HEAD = pre-compact
        // Disk still lags (editor holds the compacted buffer, no flush yet).
        assert_eq!(fs::read_to_string(&file).unwrap(), PRECOMPACT_DOC);

        let err = commit_compacted_authoritative(&file, COMPACTED_DOC).unwrap_err();
        assert!(
            err.to_string()
                .contains("did not land the compacted content"),
            "expected fail-closed HEAD-mismatch error, got: {err}"
        );
        // HEAD is unchanged; the operator gets an explicit recovery path.
        let committed = agent_doc_git_io::revision::show_head(&file)
            .unwrap()
            .unwrap();
        assert!(committed.contains("### Re: topic one"));
    }

    #[test]
    fn compact_with_commit_converges_committed_response_head_without_historical_drift_guard() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);

        let file = root.join("session.md");
        let doc = concat!(
            "---\nagent_doc_session: test-compactdrift\nagent_doc_format: template\n---\n\n",
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "rtwbcast landed.\n",
            "<!-- /agent:status -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\n",
            "Earlier work.\n\n",
            "do #rtwbcast. spec-test-build-install-commit-push\n",
            "### Re: do [#rtwbcast] — multi-editor CRDT broadcast — opus-4-8\n\n",
            "Implemented the broadcast rung.\n",
            "<!-- /agent:exchange -->\n\n",
            "## Backlog\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#follow] keep an eye on convergence\n",
            "<!-- /agent:backlog -->\n",
        );
        fs::write(&file, doc).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();
        git(root, &["add", "session.md"]);
        git(root, &["commit", "-q", "-m", "finalized response head"]);

        let _listener = start_component_patch_visible_write_listener(root);
        crate::test_support::wait_for_live_prompt_drift_listener(root);

        run(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            true,
            false,
        )
        .expect("compact --commit must not reject a clean exchange-only historical response");

        let committed = agent_doc_git_io::revision::show_head(&file)
            .unwrap()
            .unwrap();
        assert!(
            committed.contains("Compacted summary."),
            "HEAD should hold the compacted exchange:\n{committed}"
        );
        assert!(
            !committed.contains("### Re: do [#rtwbcast]"),
            "the historical finalized response must be archived out of HEAD:\n{committed}"
        );
        assert!(
            committed.contains("- [ ] [#follow] keep an eye on convergence"),
            "compact must preserve non-exchange components:\n{committed}"
        );
        assert_eq!(
            agent_doc_snapshot_io::load(&file).unwrap().unwrap(),
            committed,
            "post-compact snapshot must match the committed document"
        );

        let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("compact_writeback") && ops_log.contains("transport=editor_ipc"),
            "fixture should prove the Compact Exchange editor-IPC path:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("commit_blocked_committed_historical_patchback")
                && !ops_log.contains("typed_component_drift")
                && !ops_log.contains("refusing to auto-adopt committed historical response"),
            "clean exchange-only compact must not trip the historical response drift guard:\n{ops_log}"
        );
    }

    #[test]
    fn apply_compacted_document_fails_closed_on_malformed_summary() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let file = root.join("doc.md");
        let source = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n❯ prompt\n### Re: prompt — gpt-5\n\nAnswered.\n<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, source).unwrap();
        let malformed_compacted = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:exchange -->\n❯ *Compacted. Content archived to `x.md`*\n<!-- /agent:exchange -->\n",
        );
        let err = apply_compacted_document(
            &file,
            malformed_compacted,
            malformed_compacted,
            source,
            false,
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string()
                .contains("post-compact exchange is malformed"),
            "{err}"
        );
        // The malformed content must NOT have been written.
        assert_eq!(std::fs::read_to_string(&file).unwrap(), source);
    }

    #[test]
    fn compact_without_commit_records_uncommitted_warning() {
        // #jb-compact-repair-left-uncommitted: compact without --commit rewrites
        // the doc but must not silently leave it dirty — it records an explicit
        // uncommitted-state diagnostic with the recovery command.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let file = root.join("session.md");
        let doc = concat!(
            "---\nagent_doc_session: test-compact\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: topic one\n\nResponse one.\n\n",
            "### Re: topic two\n\nResponse two.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();

        run(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            false, // no --commit
            true,
        )
        .unwrap();

        let after = std::fs::read_to_string(&file).unwrap();
        assert!(
            after.contains("Compacted summary."),
            "doc should be compacted"
        );
        let ops_log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("compact_left_uncommitted"),
            "uncommitted compact must be recorded, got:\n{ops_log}"
        );
    }

    fn git(dir: &Path, args: &[&str]) {
        let out = Command::new("git")
            .current_dir(dir)
            .args(args)
            .output()
            .unwrap();
        assert!(out.status.success(), "git {:?} failed", args);
    }

    fn record_compact_lazily_receipt(file: &Path, patch_id: &str, content: &str) -> Option<()> {
        let file_key = file.to_string_lossy();
        agent_doc_debounce::record_live_buffer_synced_content_for_editor_with_capabilities(
            file_key.as_ref(),
            content,
            "compact-test-editor",
            "test",
            "test",
            &[
                agent_doc_debounce::OPERATOR_TEXT_AUTHORITY_CAPABILITY,
                agent_doc_debounce::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY,
            ],
        )
        .ok()?;
        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
            file, patch_id, content, "compact_test",
        )
        .ok()?;
        Some(())
    }

    fn start_component_patch_visible_write_listener(root: &Path) -> std::thread::JoinHandle<()> {
        let root = root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = payload
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                let mut content = payload.get("baseline")?.as_str()?.to_string();
                if let Some(frontmatter) = payload.get("frontmatter").and_then(|v| v.as_str()) {
                    content = replace_frontmatter(&content, frontmatter)?;
                }
                for patch in payload.get("patches")?.as_array()? {
                    let name = patch.get("component")?.as_str()?;
                    let replacement = patch.get("content")?.as_str()?;
                    let components = element::parse(&content).ok()?;
                    let target = components.iter().find(|component| component.name == name)?;
                    content = target.replace_content(&content, replacement);
                }

                if let Some(file_path) = payload.get("file").and_then(|value| value.as_str()) {
                    let _ = std::fs::write(file_path, &content);
                    record_compact_lazily_receipt(Path::new(file_path), patch_id, &content)?;
                }
                Some(
                    serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                        .to_string(),
                )
            });
        })
    }

    /// A live-editor listener that mimics the real JetBrains plugin: it applies a
    /// convergence patch to an in-memory "buffer" and, critically, does NOT write
    /// the working-tree file — it only flushes the buffer to disk when it receives
    /// a `save_document` message. The disk-writing `start_component_patch_visible_write_listener`
    /// hides `#jb-compact-editor-buffer-flush` because it saves on every patch.
    fn start_buffer_only_patch_listener(root: &Path) -> std::thread::JoinHandle<()> {
        use std::collections::HashMap;
        use std::sync::{Arc, Mutex};
        let root = root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            // Per-file in-memory editor buffer, seeded lazily from the patch baseline.
            let buffers: Arc<Mutex<HashMap<String, String>>> = Arc::new(Mutex::new(HashMap::new()));
            let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = payload
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");

                match payload.get("type").and_then(|value| value.as_str()) {
                    Some("save_document") => {
                        // Flush the in-memory buffer to disk, exactly like the real
                        // plugin's `saveDocumentViaDocument`.
                        let file_path = payload.get("file").and_then(|value| value.as_str())?;
                        let content = buffers
                            .lock()
                            .unwrap()
                            .get(file_path)
                            .cloned()
                            .or_else(|| std::fs::read_to_string(file_path).ok())?;
                        std::fs::write(file_path, &content).ok()?;
                        let receipt_file = Path::new(file_path).to_path_buf();
                        let receipt_patch_id = patch_id.to_string();
                        std::thread::spawn(move || {
                            let _ = record_compact_lazily_receipt(
                                &receipt_file,
                                &receipt_patch_id,
                                &content,
                            );
                        });
                        Some(
                            serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                                .to_string(),
                        )
                    }
                    _ => {
                        // Apply the convergence patch to the buffer only (no disk write).
                        let mut content = payload.get("baseline")?.as_str()?.to_string();
                        if let Some(frontmatter) =
                            payload.get("frontmatter").and_then(|v| v.as_str())
                        {
                            content = replace_frontmatter(&content, frontmatter)?;
                        }
                        for patch in payload.get("patches")?.as_array()? {
                            let name = patch.get("component")?.as_str()?;
                            let replacement = patch.get("content")?.as_str()?;
                            let components = element::parse(&content).ok()?;
                            let target =
                                components.iter().find(|component| component.name == name)?;
                            content = target.replace_content(&content, replacement);
                        }
                        if let Some(file_path) =
                            payload.get("file").and_then(|value| value.as_str())
                        {
                            buffers
                                .lock()
                                .unwrap()
                                .insert(file_path.to_string(), content.clone());
                            record_compact_lazily_receipt(
                                Path::new(file_path),
                                patch_id,
                                &content,
                            )?;
                        }
                        Some(
                            serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                                .to_string(),
                        )
                    }
                }
            });
        })
    }

    #[test]
    fn compact_with_commit_flushes_editor_buffer_to_disk() {
        // #jb-compact-editor-buffer-flush: a live editor applies the compact
        // convergence to its in-memory buffer and does NOT save. Without the
        // flush, the selective commit sees a stale pre-compact working tree, so
        // HEAD never lands the summary and the working tree stays dirty — the
        // "JB Compact Exchange left an uncommitted summary" defect.
        use agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();

        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);

        let file = root.join("session.md");
        let doc = concat!(
            "---\nagent_doc_session: test-flush\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older — gpt-5\n\nEarlier work.\n\n",
            "### Re: newer — opus-4-8\n\nMore work.\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&file, doc).unwrap();
        agent_doc_snapshot_io::save(&file, doc, agent_doc_ops_log_io::log_op).unwrap();
        git(root, &["add", "session.md"]);
        git(root, &["commit", "-q", "-m", "seed"]);

        let _listener = start_buffer_only_patch_listener(root);
        crate::test_support::wait_for_live_prompt_drift_listener(root);

        run(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            true,
            false,
        )
        .expect("editor-IPC compact --commit must succeed");

        let head = agent_doc_git_io::revision::show_head(&file)
            .unwrap()
            .unwrap();
        assert!(
            head.contains("Compacted summary."),
            "HEAD should hold the compacted summary:\n{head}"
        );

        // The working-tree file on disk must equal HEAD — no uncommitted summary.
        let disk = fs::read_to_string(&file).unwrap();
        assert_eq!(
            normalize_transient_agent_doc_markers(&disk),
            normalize_transient_agent_doc_markers(&head),
            "working tree must equal HEAD after compact once the editor buffer is flushed:\ndisk:\n{disk}\n---\nhead:\n{head}"
        );

        let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("compact_editor_buffer_flush")
                && (ops_log.contains("transport=save_document")
                    || ops_log.contains("transport=already_disk")),
            "compact --commit must converge the editor buffer to disk:\n{ops_log}"
        );
    }

    fn replace_frontmatter(content: &str, frontmatter: &str) -> Option<String> {
        let rest = content.strip_prefix("---\n")?;
        let end = rest.find("\n---")?;
        Some(format!(
            "---\n{frontmatter}\n---{}",
            &rest[end + "\n---".len()..]
        ))
    }
}
