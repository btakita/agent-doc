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
//! - `commit: true` carries separate live and committed targets when unresolved post-boundary input
//!   exists. Editor flush/relay convergence uses the live target; snapshot staging and HEAD
//!   verification use the committed target. A converged live relay is never reset to the
//!   committed-only projection (`#jb-compact-two-target-lineage`). A stale zero-editor relay fallback
//!   is repaired to the live target, and concurrent live-editor drift fails closed.
//! - Template compaction owns only its selected component cell. After CRDT convergence, concurrent
//!   sibling-cell edits (including queue item deletions) are rebased into both the live and committed
//!   targets before snapshot/commit; concurrent edits inside the compacted cell remain fail-closed
//!   (`#compact-independent-cells`).
//! - Before deriving a compact target, normal compaction replays any exact base-keyed durable
//!   editor-op epoch into its semantic input cut. The originally observed bytes remain the
//!   convergence compare-and-swap base, and the epoch is cleared only after the compact write and
//!   snapshot succeed (`#compactcachedeletetombstone`).
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
//! - compact_with_commit_requests_vcs_refresh: `--commit` closeout creates an `agent-doc`
//!   commit and sends the shared `refresh_vcs` intent to registered editor endpoints
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

/// The two intentional projections produced by Compact Exchange.
///
/// An unresolved prompt after the exchange boundary remains in the live editor,
/// but is omitted from the committed snapshot so compaction cannot mark it as
/// answered. Most compactions have identical projections.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CompactDocumentTargets {
    live: String,
    committed: String,
}

struct CompactApplyOptions<'a> {
    target_component: Option<&'a str>,
    refresh_crdt: bool,
    force_disk: bool,
}

impl CompactDocumentTargets {
    fn same(content: String) -> Self {
        Self {
            live: content.clone(),
            committed: content,
        }
    }

    /// Rebase the compacted target cell onto the authoritative sibling cells.
    ///
    /// Compact owns only the archiveable region of `target`. A concurrent
    /// operator edit in `queue`, `backlog`, or another sibling component is
    /// independent and must survive in both projections. For `exchange`, the
    /// post-boundary tail is also independent live state: the committed target
    /// is the compact-owned prefix, while the live target may extend it with the
    /// latest authoritative tail. Drift inside the compact-owned region remains
    /// fail-closed.
    fn rebase_onto_authoritative_siblings(self, authoritative: &str, target: &str) -> Result<Self> {
        let intended_components = element::parse(&self.live)?;
        let committed_components = element::parse(&self.committed)?;
        let authoritative_components = element::parse(authoritative)?;
        let intended_target = intended_components
            .iter()
            .find(|component| component.name == target)
            .ok_or_else(|| {
                anyhow::anyhow!("component '{}' not found in compacted target", target)
            })?;
        let committed_target = committed_components
            .iter()
            .find(|component| component.name == target)
            .ok_or_else(|| {
                anyhow::anyhow!("component '{}' not found in compacted snapshot", target)
            })?;
        let authoritative_target = authoritative_components
            .iter()
            .find(|component| component.name == target)
            .ok_or_else(|| {
                anyhow::anyhow!("component '{}' not found after compact convergence", target)
            })?;

        let intended_target_content = intended_target.content(&self.live);
        let committed_target_content = committed_target.content(&self.committed);
        let authoritative_target_content = authoritative_target.content(authoritative);
        let compact_owned_content_matches = if target == "exchange" {
            intended_target_content.starts_with(committed_target_content)
                && authoritative_target_content.starts_with(committed_target_content)
        } else {
            authoritative_target_content == intended_target_content
        };
        if authoritative_target.attrs != intended_target.attrs || !compact_owned_content_matches {
            anyhow::bail!(
                "compact: '{}' compact-owned content changed during compaction; refusing to rebase a same-cell edit over the compacted target",
                target
            );
        }

        let mut live = authoritative.to_string();
        let mut committed =
            authoritative_target.replace_content(authoritative, committed_target_content);
        if let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(&live)?
        {
            live = reconciled;
        }
        if let Some(reconciled) =
            agent_doc_document::status_projection::reconcile_top_backlog_status_content(&committed)?
        {
            committed = reconciled;
        }

        Ok(Self { live, committed })
    }
}

pub trait CompactRuntimeEffects: Sync {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;
    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String>;
    fn begin_force_disk_authority_scope(
        &self,
        file: &Path,
        source: &str,
    ) -> Result<Box<dyn std::any::Any>>;
    fn commit_with_outcome(&self, file: &Path) -> Result<CompactCommitOutcome>;
    fn atomic_write(&self, file: &Path, content: &str) -> Result<()>;
    fn force_disk_atomic_write(&self, file: &Path, content: &str) -> Result<()>;
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
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
    }

    fn begin_force_disk_authority_scope(
        &self,
        file: &Path,
        source: &str,
    ) -> Result<Box<dyn std::any::Any>> {
        Ok(Box::new(
            agent_doc_document_realtime_io::begin_force_disk_authority_scope(file, source)?,
        ))
    }

    fn commit_with_outcome(&self, file: &Path) -> Result<CompactCommitOutcome> {
        // MUST match the production CLI impl (`CliCompactRuntimeEffects`): route
        // through the authoritative-compaction commit so the committed-historical
        // response-patchback guard stands down for the archived `### Re:` turns.
        // dd9ca291 fixed this double but NOT the CLI impl, so the test passed while
        // the real binary failed closed on agent-doc-bugs2.md; they must stay in
        // lockstep (`#jb-compact-commit-historical-patchback-guard`).
        let outcome = agent_doc_commit_io::commit_with_authoritative_compaction(file)?;
        Ok(CompactCommitOutcome {
            did_commit: outcome.did_commit,
            vcs_refresh_signaled: outcome.vcs_refresh_signaled,
        })
    }

    fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_through_authority(file, content)
    }

    fn force_disk_atomic_write(&self, file: &Path, content: &str) -> Result<()> {
        agent_doc_document_realtime_io::atomic_write_force_disk_through_authority(file, content)
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
        let _ = (file, projected, visible, stage);
        Ok(false)
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
    fn read_current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
        std::fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))
    }

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
    if file.exists() {
        let _ =
            agent_doc_controller_io::project_controller::recycle_stale_supervisor_for_turn_stage(
                file,
                "compact_start",
            );
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

    #[cfg(test)]
    {
        run_in_controller(file, keep, component_name, message, tag, commit, force_disk)
    }
    #[cfg(not(test))]
    agent_doc_controller_io::project_controller::compact_document_via_controller(
        file,
        agent_doc_controller_io::project_controller::ControllerCompactDocumentInvocation {
            keep,
            component_name: component_name.map(str::to_string),
            message: message.map(str::to_string),
            tag: tag.map(str::to_string),
            commit,
            force_disk,
        },
    )
}

fn clear_replayed_editor_ops_after_compact(file: &Path, replayed: bool) {
    if !replayed {
        return;
    }
    match agent_doc_op_capture_io::clear_op_capture(file) {
        Ok(()) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "compact_pending_editor_cut_consumed file={} outcome=cleared_after_successful_write",
                file.display(),
            ),
        ),
        Err(err) => {
            eprintln!(
                "[compact] warning: failed to clear replayed editor-op capture for {} after successful write: {err}",
                file.display(),
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "compact_pending_editor_cut_clear_failed file={} error={}",
                    file.display(),
                    err,
                ),
            );
        }
    }
}

/// Execute compaction inside the CP process. This entrypoint is wired only by
/// the project-controller runtime effect; editor/CLI callers use [`run`].
pub fn run_in_controller(
    file: &Path,
    keep: Option<usize>,
    component_name: Option<&str>,
    message: Option<&str>,
    tag: Option<&str>,
    commit: bool,
    force_disk: bool,
) -> Result<()> {
    agent_doc_document_realtime_io::with_current_document_projection_pass(|| {
        run_in_controller_scoped(file, keep, component_name, message, tag, commit, force_disk)
    })
}

fn run_in_controller_scoped(
    file: &Path,
    keep: Option<usize>,
    component_name: Option<&str>,
    message: Option<&str>,
    tag: Option<&str>,
    commit: bool,
    force_disk: bool,
) -> Result<()> {
    let compact_started = std::time::Instant::now();
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }

    let effects = runtime_effects()?;
    let _force_disk_authority_scope = if force_disk {
        Some(effects.begin_force_disk_authority_scope(file, "compact_force_disk_authorization")?)
    } else {
        None
    };
    let observed_write_base_content = (if force_disk {
        effects.force_disk_document_content(file, "compact_run_initial_force_disk")
    } else {
        effects.current_document_content(file, "compact_run_initial")
    })
    .with_context(|| format!("failed to read {}", file.display()))?;
    // `#compactcachedeletetombstone`: a paused editor can have a durable,
    // base-keyed operator-op epoch even while the controller's visible
    // projection still equals that epoch's base. Compact must derive its
    // semantic cut from those ops before parsing any component. Otherwise the
    // stale queue becomes part of the compacted canonical target, resurrecting
    // rows the operator deleted and provoking a JetBrains File Cache Conflict.
    //
    // Keep the separately observed bytes as the convergence CAS base. The
    // compact target contains both the replayed operator cut and the exchange
    // rewrite, so it can be applied atomically from that exact observed base.
    let pending_editor_cut = if force_disk {
        None
    } else {
        Some(
            agent_doc_document_realtime_io::reconcile_pending_editor_cut(
                file,
                &observed_write_base_content,
                &observed_write_base_content,
                "compact",
            )?,
        )
    };
    let replayed_pending_editor_ops = pending_editor_cut
        .as_ref()
        .is_some_and(|cut| cut.replayed_editor_ops);
    let semantic_base_content = pending_editor_cut
        .map(|cut| cut.content)
        .unwrap_or_else(|| observed_write_base_content.clone());
    // Compact is not a repair command. Gate the exact realtime/disk authority
    // before creating tags, composing captures, mutating CRDT state, or
    // committing. This prevents a no-op compact from laundering a malformed
    // document into HEAD.
    agent_doc_lint_io::validate_integrity_on_content_with_logger(
        file,
        &semantic_base_content,
        agent_doc_ops_log_io::log_op,
    )?;
    agent_doc_lint_io::run_on_content_with_logger(
        file,
        &semantic_base_content,
        None,
        agent_doc_ops_log_io::log_op,
    )?;

    let content = compose_active_capture_for_compaction(file, &semantic_base_content)?;

    let (fm, body) = frontmatter::parse(&content)?;

    let resolved = fm.resolve_mode();
    // Determine a semantic no-op before the checkpoint tag or any archive/document
    // effect. A no-op must not become a generic commit or repair surface.
    let requested_noop = if resolved.is_template() {
        let target = component_name.unwrap_or("exchange");
        let components = element::parse(&content)?;
        let comp = components
            .iter()
            .find(|component| component.name == target)
            .ok_or_else(|| anyhow::anyhow!("component '{}' not found in document", target))?;
        let old_content = comp.content(&content);
        match keep {
            Some(keep) => parse_topic_sections_with_tail(old_content).sections.len() <= keep,
            None if target == "exchange" => split_component_content_at_boundary(old_content)
                .0
                .trim()
                .is_empty(),
            None => old_content.trim().is_empty(),
        }
    } else {
        parse_inline_exchanges(body).len() <= keep.unwrap_or(2)
    };
    if requested_noop {
        eprintln!("[compact] no compaction changes; leaving document and commit state untouched");
        return Ok(());
    }
    let prepare_ms = compact_started.elapsed().as_millis();

    // Create a pre-compact git tag at HEAD only after integrity and semantic
    // mutation validation. Skipped if tag == Some("skip").
    if tag != Some("skip")
        && let Err(e) =
            agent_doc_git_io::checkpoint::create_pre_mutation_tag(file, "pre-compact", tag)
    {
        eprintln!("[compact] Warning: could not create pre-compact tag: {}", e);
    }
    // Compact Exchange intentionally has two targets when an unresolved prompt
    // follows the exchange boundary: the live target preserves that prompt, while
    // the committed target omits it. Keep both through flush + commit closeout so
    // the HEAD-only projection can never reset the converged editor lineage
    // (`#jb-compact-two-target-lineage`).
    let apply_started = std::time::Instant::now();
    let targets = if resolved.is_template() {
        // The live Lazily authority is compacted through the normal intent seam;
        // `apply_compacted_document` then checkpoints only the resulting baseline
        // and cold restart projection in `state.db`.
        let target = component_name.unwrap_or("exchange");
        let is_crdt = resolved.is_crdt();
        match keep {
            Some(n) => run_component_compact_partial(
                file,
                &content,
                &observed_write_base_content,
                PartialCompactOptions {
                    target,
                    keep: n,
                    message,
                    is_crdt,
                    force_disk,
                },
            ),
            None => run_component_compact_with_options(
                file,
                &content,
                &observed_write_base_content,
                target,
                message,
                is_crdt,
                force_disk,
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

        let inline_targets = apply_compacted_document(
            file,
            &compacted,
            &compacted,
            &content,
            &observed_write_base_content,
            CompactApplyOptions {
                target_component: None,
                refresh_crdt: false,
                force_disk,
            },
        )?;
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
        inline_targets
    };
    let apply_ms = apply_started.elapsed().as_millis();
    let post_apply_started = std::time::Instant::now();

    // The durable op epoch is consumed only after the compact target has
    // crossed the authoritative write and snapshot boundary. Any earlier
    // validation/convergence failure leaves it available for a retry.
    clear_replayed_editor_ops_after_compact(file, replayed_pending_editor_ops);

    // A no-op compact is not a response-repair escape hatch. In particular,
    // do not use unrelated snapshot/capture drift to manufacture a commit when
    // the requested compaction retained the document byte-for-byte.
    if targets.live == content && targets.committed == content {
        eprintln!("[compact] no compaction changes; leaving document and commit state untouched");
        return Ok(());
    }

    // `#jb-compact-editor-buffer-flush`: CRDT convergence now requests and proves
    // the owning editor's native save. Keep this older targeted flush as a
    // defense-in-depth path for legacy editor-IPC convergence that updated only
    // the live in-memory buffer. Flush it before the re-read and commit. Otherwise
    // the selective commit compares the stale pre-compact working tree against the
    // compacted snapshot, treats the snapshot as historical exchange drift, and
    // repairs it back to HEAD, leaving HEAD and disk pre-compact (the "JB Compact
    // Exchange left an uncommitted summary" defect). Fail-open: if the editor
    // cannot flush, `commit_compacted_authoritative` / `compact_dirty` still stage
    // the authoritative snapshot and verify HEAD.
    if commit && !force_disk {
        let disk_is_pre_compact = effects
            .force_disk_document_content(file, "compact_pre_commit_disk_flush_probe")
            .map(|disk| disk == observed_write_base_content)
            .unwrap_or(false);
        if disk_is_pre_compact {
            flush_editor_buffer_to_disk_after_compact(file, &targets.live, effects);
        }
    }

    if component_name.is_none() || component_name == Some("exchange") {
        agent_doc_session_accretion_io::record_recent_exchange_compaction(file)?;
    }

    let updated = effects
        .force_disk_document_content(file, "compact_post_write_disk_verify")
        .with_context(|| format!("failed to re-read {} after compact", file.display()))?;
    // `#compact-independent-cells`: do not compare sibling item counts against
    // the pre-compact document here. Successful CRDT convergence may include
    // authoritative concurrent queue/backlog edits, and `targets` already
    // rebased the snapshot/commit projection onto those sibling cells. The
    // deterministic pre-write guard still proves that the compaction rebuild
    // itself changed only its target component.
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
    let post_apply_ms = post_apply_started.elapsed().as_millis();
    let commit_started = std::time::Instant::now();
    if commit {
        if dirty {
            commit_compacted_authoritative(file, &targets.committed, &targets.live)?;
        } else {
            // `#bbdcompactnocommit`: this branch used to be absent, so a
            // `--commit` compact that could not observe committable state
            // returned Ok having silently skipped the commit. Operator-reported
            // on brookebrodack-dev.md: the compacted document and its archive
            // were on disk, only the commit was missing, and it had to be
            // noticed by hand.
            //
            // Reaching here means the compaction DID change something — the
            // genuine no-op already returned above (`targets.live == content &&
            // targets.committed == content`). So `!dirty` is a failure to
            // OBSERVE the change, not an absence of one: with a live editor
            // holding the buffer mid-compact the on-disk re-read still equals
            // the pre-compact content, and the snapshot may not have converged
            // to HEAD divergence yet either (the reported case logged
            // `delivery_converged=false live_editors=1`).
            //
            // Give convergence a bounded chance, then fail loudly. Silently
            // reporting success is the one outcome that must not survive.
            let mut observed = dirty;
            for attempt in 1..=COMPACT_COMMIT_OBSERVE_ATTEMPTS {
                // `#lazily-hot-path` Theme A — the reason this loop re-reads at all is
                // that a live editor may still be delivering the compacted buffer
                // (the reported case logged `delivery_converged=false live_editors=1`).
                // Wait on the controller's convergence witness for the backoff window
                // instead of sleeping it blindly: when delivery settles we re-read
                // immediately. The hub is process-local, so only the controller can
                // answer — when it cannot (no hub for this document, or no controller
                // at all) we fall back to exactly the previous blind sleep, which
                // keeps this fail-open.
                match agent_doc_controller_io::project_controller::await_delivery_convergence_for_file(
                    file,
                    COMPACT_COMMIT_OBSERVE_BACKOFF,
                ) {
                    // Observed: the await already consumed up to the window (it
                    // returns early only when convergence landed), so do not sleep
                    // it a second time.
                    Ok(Some(_)) => {}
                    Ok(None) => std::thread::sleep(COMPACT_COMMIT_OBSERVE_BACKOFF),
                    Err(err) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "compact_commit_observe_convergence_unavailable file={} fallback=blind_backoff detail={}",
                                file.display(),
                                format!("{err:#}")
                                    .replace('\n', " | ")
                                    .chars()
                                    .take(160)
                                    .collect::<String>()
                            ),
                        );
                        std::thread::sleep(COMPACT_COMMIT_OBSERVE_BACKOFF);
                    }
                }
                let retried_disk = effects
                    .force_disk_document_content(file, "compact_commit_observe_retry")
                    .unwrap_or_else(|_| content.to_string());
                let retried_snapshot = agent_doc_snapshot_io::verify_snapshot_committed(file)?;
                observed = compact_dirty(retried_disk != content, &retried_snapshot);
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "compact_commit_observe_retry file={} attempt={}/{} dirty={} (#bbdcompactnocommit)",
                        file.display(),
                        attempt,
                        COMPACT_COMMIT_OBSERVE_ATTEMPTS,
                        observed,
                    ),
                );
                if observed {
                    break;
                }
            }
            if observed {
                commit_compacted_authoritative(file, &targets.committed, &targets.live)?;
            } else {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "compact_commit_unobservable file={} recovery=retry_compact_commit (#bbdcompactnocommit)",
                        file.display()
                    ),
                );
                anyhow::bail!(
                    "{}: compaction was applied but could not be committed — the change is not \
                     observable on disk or in the snapshot after {} attempt(s). A live editor may \
                     still own the buffer (concurrent edits mid-compact). The compacted document and \
                     its archive are intact; only the commit is missing. Re-run `agent-doc compact {} \
                     --commit` once the editor has settled (#bbdcompactnocommit)",
                    file.display(),
                    COMPACT_COMMIT_OBSERVE_ATTEMPTS,
                    file.display(),
                );
            }
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
    let commit_ms = commit_started.elapsed().as_millis();
    let total_ms = compact_started.elapsed().as_millis();
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "compact_latency file={} total_ms={} phases=prepare:{}ms,apply:{}ms,post_apply:{}ms,commit:{}ms commit={} force_disk={}",
            file.display(),
            total_ms,
            prepare_ms,
            apply_ms,
            post_apply_ms,
            commit_ms,
            commit,
            force_disk,
        ),
    );

    Ok(())
}

/// `#bbdcompactnocommit`: how long a `--commit` compact waits for its own change
/// to become observable before failing loudly. A live editor mid-compact can hold
/// the buffer past the first disk re-read, so give convergence a bounded chance
/// rather than either hanging or silently skipping the commit.
#[cfg(test)]
const COMPACT_COMMIT_OBSERVE_ATTEMPTS: u32 = 2;
#[cfg(not(test))]
const COMPACT_COMMIT_OBSERVE_ATTEMPTS: u32 = 5;
#[cfg(test)]
const COMPACT_COMMIT_OBSERVE_BACKOFF: std::time::Duration = std::time::Duration::from_millis(5);
#[cfg(not(test))]
const COMPACT_COMMIT_OBSERVE_BACKOFF: std::time::Duration = std::time::Duration::from_millis(200);

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
fn commit_compacted_authoritative(
    file: &Path,
    authoritative_snapshot: &str,
    live_target: &str,
) -> Result<()> {
    // The commit boundary may not create or reassert secondary durability
    // projections while the editor still owes an ACK for the live target.
    // Matching canonical text is necessary but not sufficient: it can be a
    // retained asynchronous delivery whose listener is build-skewed.
    ensure_compact_live_relay_target(file, live_target)?;
    // Re-assert the authoritative snapshot so a replay/lag between
    // `apply_compacted_document` and here cannot leave a pre-compact snapshot for
    // the selective commit to stage.
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        authoritative_snapshot,
        agent_doc_ops_log_io::log_op,
    )?;
    // `#jb-compact-commit-stale-relay-canonical`: the third desync mechanism.
    // When the compaction wrote through the stale-lease disk-authority path
    // (`crdt_cp_write_disk_authority_stale_lease`, `live_editors == 0` under a
    // phantom editor lease — e.g. an older plugin whose CRDT replica register
    // failed), only disk + snapshot hold the compacted content; the lazily relay
    // canonical stays FROZEN at the pre-compact text. `closeout_compact_with_commit`
    // then resolves the commit's document content through the realtime authority
    // (`try_resolve_current_document_content`), which — seeing the reactive
    // open-docs projection still report the editor open — keeps editor authority
    // and returns that frozen pre-compact canonical, so the commit lands
    // pre-compact content in HEAD (`compact_commit_head_mismatch`, observed live on
    // agent-doc-bugs2.md). Converge a genuinely stale zero-editor canonical to
    // the LIVE compacted target BEFORE the commit reads it. Reliable-sync plane is
    // authority; disk/snapshot are durability sidecars, so the authoritative
    // content must reach the plane, not only disk. Authority-gated + fail-open:
    // `Ok(None)` (headless / missing relay model) leaves the disk+snapshot write
    // authoritative, and `verify_compact_head_landed` still fails closed if HEAD
    // does not land the compacted content.
    closeout_compact_with_commit(file)?;
    verify_compact_head_landed(file, authoritative_snapshot)
}

/// Preserve a converged live editor and repair only the stale relay fallback.
///
/// A prior unconditional adopt used the committed target here. For exchanges with
/// unresolved input that target intentionally differs from the live editor, so the
/// adopt scheduled a whole-buffer rebootstrap and JetBrains replayed delayed events
/// from the pre-compact buffer. If a live replica drifted after compaction, fail
/// closed rather than overwriting concurrent operator input.
fn ensure_compact_live_relay_target(file: &Path, live_target: &str) -> Result<()> {
    match agent_doc_crdt_relay_io::current_text_for_file(file)? {
        agent_doc_crdt_relay_io::CurrentText::Current {
            text,
            delivery_converged,
            ..
        } if text == live_target => {
            if !delivery_converged {
                agent_doc_document_realtime_io::guard_visible_delivery_convergence(
                    file,
                    "compact_live_relay_delivery_barrier",
                )?;
                anyhow::ensure!(
                    matches!(
                        agent_doc_crdt_relay_io::current_text_for_file(file)?,
                        agent_doc_crdt_relay_io::CurrentText::Current {
                            text,
                            delivery_converged: true,
                            ..
                        } if text == live_target
                    ),
                    "compact: editor delivery changed while crossing the ACK barrier for {}; refusing secondary snapshot/commit effects",
                    file.display(),
                );
            }
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "compact_live_relay_target file={} action=already_converged len={} hash={}",
                    file.display(),
                    live_target.len(),
                    agent_doc_hash::content_hash(live_target),
                ),
            );
            Ok(())
        }
        agent_doc_crdt_relay_io::CurrentText::Current {
            live_editors, text, ..
        } if live_editors > 0 => anyhow::bail!(
            "compact: live editor changed after compaction (expected hash {}, found hash {}); refusing to reset concurrent operator input",
            agent_doc_hash::content_hash(live_target),
            agent_doc_hash::content_hash(&text),
        ),
        current => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "compact_live_relay_target file={} action=repair_stale_fallback current={current:?} len={} hash={}",
                    file.display(),
                    live_target.len(),
                    agent_doc_hash::content_hash(live_target),
                ),
            );
            agent_doc_crdt_relay_io::adopt_authoritative_text_for_file(file, live_target)?;
            Ok(())
        }
    }
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
fn flush_editor_buffer_to_disk_after_compact(
    file: &Path,
    expected_content: &str,
    effects: &dyn CompactRuntimeEffects,
) -> bool {
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

    if compact_disk_matches_expected(&canonical, expected_content, effects) {
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

    let Some(registration) =
        agent_doc_controller_io::project_controller::live_editor_registration_for_file(file)
            .ok()
            .flatten()
    else {
        return false;
    };
    if !agent_doc_ipc_io::is_listener_active_for_pid(&project_root, registration.pid) {
        return false;
    }
    let flushed = agent_doc_ipc_io::send_save_document_to_editor(
        &project_root,
        registration.pid,
        &registration.editor_id,
        &path_str,
        &patch_id,
    );

    if let Err(e) = flushed {
        eprintln!(
            "[compact] warning: editor buffer flush after compact failed for {} (working tree may lag HEAD until the editor saves): {e}",
            file.display()
        );
        return false;
    }

    // The typed `save_document` intent responds after saving; the CP receipt is applied
    // asynchronously, so poll the working tree until the flush lands (or time out).
    let deadline = std::time::Instant::now() + std::time::Duration::from_millis(1000);
    loop {
        if compact_disk_matches_expected(&canonical, expected_content, effects) {
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

fn compact_disk_matches_expected(
    file: &Path,
    expected_content: &str,
    effects: &dyn CompactRuntimeEffects,
) -> bool {
    effects
        .force_disk_document_content(file, "compact_editor_buffer_flush_disk_poll")
        .is_ok_and(|disk| {
            agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(&disk)
                == agent_doc_document::transient_markers::normalize_transient_agent_doc_markers(
                    expected_content,
                )
        })
}

fn closeout_compact_with_commit(file: &Path) -> Result<()> {
    // The `commit_with_outcome` port MUST establish the authoritative-compaction
    // scope (`agent_doc_commit_io::commit_with_authoritative_compaction`) so the
    // committed-historical response-patchback guard stands down for the archived
    // `### Re:` turns. compact-io cannot own that scope here — it has no
    // production dependency on commit-io — so the CLI impl and the test double
    // must stay in lockstep on that choice, enforced by
    // `commit_with_authoritative_compaction_used_by_compact_closeout`
    // (`#jb-compact-commit-historical-patchback-guard`).
    let outcome = runtime_effects()?.commit_with_outcome(file)?;
    if outcome.did_commit && outcome.vcs_refresh_signaled == Some(false) {
        eprintln!(
            "[compact] note: committed {} without an attached editor endpoint; VCS refresh is observational and does not invalidate the durable commit",
            file.display()
        );
    }
    eprintln!(
        "{}",
        agent_doc_controller_io::project_controller::COMPACT_COMMIT_SCOPE_NOTE
    );
    Ok(())
}

/// `#stale-capture-after-compaction-blocks-route`: best-effort retirement of
/// capture facts whose response body was just archived. Never fails the
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

/// Bounded editor-convergence retries for a compact write that hits
/// `retry_crdt_merge` (#compactcrdtretry). Small: only the transient in-flight
/// editor-delta race is retried; a genuine concurrent edit fails closed.
const COMPACT_CONVERGE_MAX_ATTEMPTS: usize = 3;

/// How long a `retry_crdt_merge` retry waits for the controller's
/// delivery-convergence witness before re-reading the live canonical text
/// (#compactcrdtretry). A `retry_crdt_merge` refusal means an editor delivery is
/// mid-delta, so retrying instantly re-hits the same in-flight state; two seconds
/// covers a normal editor delta round trip (the same order as the other
/// delivery-await windows) without stalling the compact when the buffer is
/// genuinely contended — the await returns EARLY the moment convergence lands, so
/// the full window is only ever consumed when delivery really has not settled.
/// Tests use a token window: the controller is unreachable there, so the await
/// fails open immediately.
#[cfg(test)]
const COMPACT_CONVERGE_DELIVERY_WAIT: std::time::Duration = std::time::Duration::from_millis(5);
#[cfg(not(test))]
const COMPACT_CONVERGE_DELIVERY_WAIT: std::time::Duration = std::time::Duration::from_secs(2);

/// True when a compact editor-convergence error is the CRDT compare-and-swap
/// baseline refusal (`recovery=retry_crdt_merge`), i.e. the live canonical text
/// drifted from the base the compaction was computed against.
fn is_retryable_crdt_merge_error(err: &anyhow::Error) -> bool {
    format!("{err:#}").contains("recovery=retry_crdt_merge")
}

/// Converge `compacted` through the editor path with bounded `retry_crdt_merge`
/// handling (#compactcrdtretry). Retries only when the live canonical text has
/// re-settled to `source_content` (a transient in-flight editor delta) — it never
/// rewrites the now-stale `compacted` over a genuine concurrent operator edit.
/// A real edit (live text != `source_content`) fails closed with an actionable
/// "re-run when the editor is idle" message instead of the raw CP error.
fn converge_compacted_with_retry(
    effects: &dyn CompactRuntimeEffects,
    file: &Path,
    compacted: &str,
    source_content: &str,
) -> Result<()> {
    let mut attempt = 0usize;
    loop {
        attempt += 1;
        match effects.try_editor_converge(file, compacted, source_content, "compact") {
            Ok(_) => return Ok(()),
            Err(err)
                if attempt < COMPACT_CONVERGE_MAX_ATTEMPTS
                    && is_retryable_crdt_merge_error(&err) =>
            {
                // A `retry_crdt_merge` refusal means the live canonical text is
                // mid-delta from an in-flight editor delivery, so retrying instantly
                // just re-hits the same in-flight state. Wait on the controller's
                // delivery-convergence witness first: the hub is process-local, so
                // only the controller can answer. It returns EARLY the moment
                // convergence lands, otherwise it consumes the window. Doing this
                // BEFORE the `current_document_content` re-read also makes the drift
                // check below MORE accurate — reading a settled buffer distinguishes
                // "transient in-flight delta" from "genuine concurrent operator edit"
                // instead of sampling a half-applied delta and mistaking it for
                // either one. `Ok(None)` (no hub for this document) and `Err` (the
                // controller could not be asked) both mean "nothing to wait on" —
                // never "delivery finished" — so we proceed to the same drift check,
                // which still fails closed on a real edit. This keeps the retry
                // fail-open exactly as before.
                match agent_doc_controller_io::project_controller::await_delivery_convergence_for_file(
                    file,
                    COMPACT_CONVERGE_DELIVERY_WAIT,
                ) {
                    // Observed: the await already consumed up to the window (it
                    // returns early only when convergence landed), so do not wait
                    // again here.
                    Ok(Some(_)) => {}
                    // No hub for this document: there is no in-flight delivery to
                    // wait for, so the immediate re-read is already the settled read.
                    Ok(None) => {}
                    Err(wait_err) => {
                        agent_doc_ops_log_io::log_op(
                            file,
                            &format!(
                                "compact_converge_convergence_unavailable file={} fallback=immediate_reread detail={}",
                                file.display(),
                                format!("{wait_err:#}")
                                    .replace('\n', " | ")
                                    .chars()
                                    .take(160)
                                    .collect::<String>()
                            ),
                        );
                    }
                }
                let current = effects
                    .current_document_content(file, "compact_converge_retry")
                    .unwrap_or_default();
                if current != source_content {
                    anyhow::bail!(
                        "compact: document changed during compaction; re-run compact when the editor is idle (the live buffer no longer matches the compacted base). Underlying: {err:#}"
                    );
                }
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "compact_converge_retry file={} attempt={} max_attempts={} reason=retry_crdt_merge",
                        file.display(),
                        attempt,
                        COMPACT_CONVERGE_MAX_ATTEMPTS
                    ),
                );
            }
            Err(err) if is_retryable_crdt_merge_error(&err) => {
                anyhow::bail!(
                    "compact: could not converge the compacted document after {COMPACT_CONVERGE_MAX_ATTEMPTS} attempts (the live canonical kept drifting from the compacted base); re-run compact when the editor is idle. Underlying: {err:#}"
                );
            }
            Err(err) => return Err(err),
        }
    }
}

/// Fold an active, not-yet-committed capture into the compaction input before
/// selecting archive topics. This closes the race where a response write is
/// retained for editor delivery while a later Compact Exchange is computed
/// from an older disk/controller projection. The write CAS still uses
/// `current_content` as its base; only the archive/summary model is enriched.
fn compose_active_capture_for_compaction(file: &Path, current_content: &str) -> Result<String> {
    let Some(capture) = agent_doc_capture_io::load_active(file)? else {
        return Ok(current_content.to_string());
    };
    if capture.committed_at.is_some()
        || capture.discarded_at.is_some()
        || capture.response_body.trim().is_empty()
        || agent_doc_turn::response_replay::response_materialized_in_content(
            &capture.response_body,
            current_content,
        )
    {
        return Ok(current_content.to_string());
    }

    let plan = agent_doc_template_io::parse_template_patchback(
        file,
        &capture.response_body,
        "compact_active_capture",
        agent_doc_ops_log_io::log_op,
    )?;
    let composed = agent_doc_template_io::apply_patches(
        current_content,
        &plan.patches,
        &plan.unmatched,
        file,
    )?;
    if !agent_doc_turn::response_replay::response_materialized_in_content(
        &capture.response_body,
        &composed,
    ) {
        anyhow::bail!(
            "compact: active captured response {} did not materialize in the compaction model for {}; refusing to archive an older projection",
            capture.capture_id,
            file.display(),
        );
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "compact_active_capture_composed file={} capture_id={} base_hash={} composed_hash={}",
            file.display(),
            capture.capture_id,
            agent_doc_hash::content_hash(current_content),
            agent_doc_hash::content_hash(&composed),
        ),
    );
    Ok(composed)
}

fn apply_compacted_document(
    file: &Path,
    compacted: &str,
    snapshot_content: &str,
    validation_source_content: &str,
    write_base_content: &str,
    options: CompactApplyOptions<'_>,
) -> Result<CompactDocumentTargets> {
    let CompactApplyOptions {
        target_component,
        refresh_crdt,
        force_disk,
    } = options;
    // Fail closed before any write if the rebuilt exchange is structurally
    // malformed (#jb-compact-malformed-response-commit).
    validate_compacted_exchange(file, compacted)?;

    // Fail closed before any write if the rebuild dropped a whole item from a
    // non-exchange singleton list component (#compactdropitem).
    assert_non_exchange_items_preserved(file, validation_source_content, compacted, "apply")?;

    // Fail closed before any write if the rebuild altered a non-exchange
    // opening marker (dropped inline attributes like preset/priority/go)
    // (#compactqattr).
    assert_non_exchange_markers_preserved(file, validation_source_content, compacted, "apply")?;

    let mut targets = CompactDocumentTargets {
        live: compacted.to_string(),
        committed: snapshot_content.to_string(),
    };
    if force_disk {
        runtime_effects()?.force_disk_atomic_write(file, compacted)?;
        if let Some(outcome) = agent_doc_crdt_relay_io::apply_disk_change_for_file(file, compacted)?
        {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "compact_force_disk_reconciled_canonical file={} outcome={outcome:?}",
                    file.display(),
                ),
            );
        }
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
        //
        // #compactcrdtretry: a `retry_crdt_merge` refusal means the live canonical
        // text drifted from `source_content` (the base this compaction was computed
        // against). Retry a bounded number of times, but ONLY when the live text has
        // re-settled to `source_content` (a transient in-flight editor delta) — never
        // rewrite the now-stale `compacted` over a genuine concurrent operator edit.
        // If the text truly changed, fail closed with an actionable message so the
        // operator re-runs compact when the editor is idle, instead of surfacing the
        // raw CP error (the reported JB `Compact Exchange` exit-1). The zero-live
        // editor case is already resolved to disk authority by #stale-lease-cp-authority.
        converge_compacted_with_retry(runtime_effects()?, file, compacted, write_base_content)?;
        agent_doc_document_realtime_io::guard_visible_delivery_convergence(
            file,
            "compact_post_write_delivery_barrier",
        )
        .with_context(|| {
            format!(
                "compact: refusing snapshot/CRDT-sidecar work before editor delivery ACK for {}",
                file.display()
            )
        })?;

        // #compact-independent-cells: editor/CRDT convergence may legitimately
        // carry a concurrent edit from a sibling component cell (for example,
        // queue deletions while exchange compaction is running). Rebase the two
        // compact projections onto that authoritative sibling state before the
        // snapshot/commit boundary. Same-target drift remains fail-closed.
        if let Some(target) = target_component {
            let authoritative = runtime_effects()?
                .current_document_content(file, "compact_post_converge_rebase")?;
            if authoritative != targets.live {
                targets = targets.rebase_onto_authoritative_siblings(&authoritative, target)?;
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "compact_rebased_authoritative_siblings file={} target={} live_hash={} committed_hash={}",
                        file.display(),
                        target,
                        agent_doc_hash::content_hash(&targets.live),
                        agent_doc_hash::content_hash(&targets.committed),
                    ),
                );
            }
        }
    }

    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &targets.committed,
        agent_doc_ops_log_io::log_op,
    )?;

    if refresh_crdt {
        let new_crdt =
            agent_doc_merge::crdt_sync::ReplicaState::from_text(1, &targets.live).encode_state();
        let lineage = format!("compact:{}", agent_doc_hash::content_hash(&targets.live));
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(file, &new_crdt, &lineage)?;
        eprintln!("[compact] cold CRDT recovery projection refreshed from post-compact content");
    }

    Ok(targets)
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
    run_component_compact_with_options(file, content, content, target, message, is_crdt, false)
        .map(|targets| targets.committed)
}

#[cfg(test)]
fn run_component_compact_force_disk(
    file: &Path,
    content: &str,
    target: &str,
    message: Option<&str>,
    is_crdt: bool,
) -> Result<String> {
    let _force_disk_authority_scope =
        agent_doc_document_realtime_io::begin_force_disk_authority_scope(
            file,
            "compact_test_force_disk_authorization",
        )?;
    run_component_compact_with_options(file, content, content, target, message, is_crdt, true)
        .map(|targets| targets.committed)
}

/// Re-attach the exchange boundary marker at the end of the live compacted
/// component content.
///
/// `#compactboundary`: compaction rebuilds the component body from scratch, and
/// `split_component_content_at_boundary` / `parse_topic_sections_*` strip the
/// marker out on the way in. When the live projection reaches the document
/// through a patch-applying write it gets a fresh boundary re-inserted for free
/// (`template.rs`, post-patch boundary repair), but the direct disk/detached
/// projection has no such step — so a compact taken while the editor authority is
/// degraded writes a boundary-less document. Commit still re-mints a boundary on
/// the committed blob, so the operator is left with a permanent marker-only dirty
/// diff that no later cycle heals (each subsequent compact reproduces it).
///
/// Carrying the existing marker id forward keeps the id stable across the
/// compact; a component that genuinely had no boundary gets a fresh one, matching
/// the write path's re-insert behavior. Placement is end-of-component, which is
/// the boundary invariant every other write path maintains.
///
/// Non-exchange components have no boundary and are returned unchanged.
fn append_boundary_marker(content: &str, target: &str, old_content: &str) -> String {
    if target != "exchange" {
        return content.to_string();
    }
    if agent_doc_document::compact_projection::boundary_marker_line(content).is_some() {
        return content.to_string();
    }
    let marker = agent_doc_document::compact_projection::boundary_marker_line(old_content)
        .unwrap_or_else(|| format!("<!-- agent:boundary:{} -->", uuid::Uuid::new_v4()));
    let mut out = content.to_string();
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(&marker);
    out.push('\n');
    out
}

fn append_durable_context_reference(
    file: &Path,
    document: &str,
    content: &mut String,
) -> Result<()> {
    let Some(reference) =
        agent_doc_dynamic_context_io::durable_context_reference_for_document(file, document)?
    else {
        return Ok(());
    };
    if !content.ends_with('\n') {
        content.push('\n');
    }
    content.push('\n');
    content.push_str(&reference);
    content.push('\n');
    Ok(())
}

/// Returns both the live compacted document and the committed snapshot. They differ
/// only when unresolved input follows the exchange boundary.
fn run_component_compact_with_options(
    file: &Path,
    content: &str,
    write_base_content: &str,
    target: &str,
    message: Option<&str>,
    is_crdt: bool,
    force_disk: bool,
) -> Result<CompactDocumentTargets> {
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
        return Ok(CompactDocumentTargets::same(content.to_string()));
    }

    // Archive old content
    let archive_path = save_archive(
        file,
        &build_component_archive(file, content, target, &archive_content),
    )?;

    // Build summary marker
    let mut summary = match message {
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
    if target == "exchange" {
        append_durable_context_reference(file, content, &mut summary)?;
    }

    let mut visible_content = summary.clone();
    if !trailing.trim().is_empty() {
        if !visible_content.ends_with('\n') {
            visible_content.push('\n');
        }
        visible_content.push_str(trailing.trim_end());
        visible_content.push('\n');
    }
    // `#compactboundary`: the live projection must carry a boundary even when it
    // reaches the document through the direct disk write instead of a
    // patch-applying write. Commit re-mints one on the committed blob either way,
    // so a boundary-less live document is a permanent marker-only dirty diff.
    let visible_content = append_boundary_marker(&visible_content, target, old_content);

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
    let targets = apply_compacted_document(
        file,
        &compacted,
        &snapshot_compacted,
        content,
        write_base_content,
        CompactApplyOptions {
            target_component: Some(target),
            refresh_crdt: is_crdt,
            force_disk,
        },
    )?;
    discard_archived_captures(file, &archive_content);

    let line_count = archive_content.lines().count();
    eprintln!(
        "[compact] Archived {} lines from component '{}' to {}",
        line_count,
        target,
        archive_path.display()
    );

    Ok(targets)
}

/// Partial compact a named component in a template/stream-mode document.
///
/// Archives all but the last `keep` `### Re:` topic sections; rebuilds the component
/// with an archive pointer + preamble (or `message`) + kept sections.
struct PartialCompactOptions<'a> {
    target: &'a str,
    keep: usize,
    message: Option<&'a str>,
    is_crdt: bool,
    force_disk: bool,
}

fn run_component_compact_partial(
    file: &Path,
    content: &str,
    write_base_content: &str,
    options: PartialCompactOptions<'_>,
) -> Result<CompactDocumentTargets> {
    let PartialCompactOptions {
        target,
        keep,
        message,
        is_crdt,
        force_disk,
    } = options;
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
        return Ok(CompactDocumentTargets::same(content.to_string()));
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
    if target == "exchange" {
        append_durable_context_reference(file, content, &mut base_new_content)?;
    }

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
    // `#compactboundary`: same live-boundary rule as the full component compact.
    let new_content = append_boundary_marker(&new_content, target, old_content);

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
    let targets = apply_compacted_document(
        file,
        &compacted,
        &snapshot_compacted,
        content,
        write_base_content,
        CompactApplyOptions {
            target_component: Some(target),
            refresh_crdt: is_crdt,
            force_disk,
        },
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

    Ok(targets)
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
    use anyhow::Context;
    use std::process::Command;

    /// Configurable compact effects double for the `#compactcrdtretry` converge tests:
    /// fails `retry_crdt_merge` for the first `fail_times` converge attempts, then
    /// succeeds; `current` is what `current_document_content` reports on a retry.
    struct RetryConvergeEffects {
        fail_times: std::sync::atomic::AtomicUsize,
        current: String,
        converge_calls: std::sync::atomic::AtomicUsize,
    }

    impl CompactRuntimeEffects for RetryConvergeEffects {
        fn current_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
            Ok(self.current.clone())
        }
        fn force_disk_document_content(&self, _file: &Path, _source: &str) -> Result<String> {
            Ok(self.current.clone())
        }
        fn begin_force_disk_authority_scope(
            &self,
            _file: &Path,
            _source: &str,
        ) -> Result<Box<dyn std::any::Any>> {
            unimplemented!("not exercised by converge-retry tests")
        }
        fn commit_with_outcome(&self, _file: &Path) -> Result<CompactCommitOutcome> {
            unimplemented!("not exercised by converge-retry tests")
        }
        fn atomic_write(&self, _file: &Path, _content: &str) -> Result<()> {
            unimplemented!("not exercised by converge-retry tests")
        }
        fn force_disk_atomic_write(&self, _file: &Path, _content: &str) -> Result<()> {
            unimplemented!("not exercised by converge-retry tests")
        }
        fn try_editor_converge(
            &self,
            file: &Path,
            _content: &str,
            _source: &str,
            _origin: &str,
        ) -> Result<bool> {
            use std::sync::atomic::Ordering;
            self.converge_calls.fetch_add(1, Ordering::Relaxed);
            let remaining = self.fail_times.load(Ordering::Relaxed);
            if remaining > 0 {
                self.fail_times.store(remaining - 1, Ordering::Relaxed);
                anyhow::bail!(
                    "CP relay write refused for {}: expected_hash=a current_hash=b recovery=retry_crdt_merge",
                    file.display()
                );
            }
            Ok(true)
        }
        fn guard_no_stale_snapshot_reset_drift(
            &self,
            _file: &Path,
            _projected: Option<&str>,
            _visible: &str,
            _stage: &str,
        ) -> Result<bool> {
            Ok(true)
        }
    }

    #[test]
    fn converge_compacted_retries_transient_resettled_crdt_merge_then_succeeds() {
        // #compactcrdtretry: one retry_crdt_merge refusal, but the live text has
        // re-settled to the compacted base (`current == source`), so the bounded retry
        // converges instead of surfacing the raw CP error.
        use std::sync::atomic::{AtomicUsize, Ordering};
        let base = "prompt\n";
        let effects = RetryConvergeEffects {
            fail_times: AtomicUsize::new(1),
            current: base.to_string(),
            converge_calls: AtomicUsize::new(0),
        };
        let doc = std::path::Path::new("/tmp/does-not-matter.md");
        converge_compacted_with_retry(&effects, doc, "compacted\n", base)
            .expect("transient re-settled retry_crdt_merge must converge on retry");
        assert_eq!(
            effects.converge_calls.load(Ordering::Relaxed),
            2,
            "should retry exactly once after the transient refusal"
        );
    }

    #[test]
    fn converge_compacted_fails_closed_on_genuine_concurrent_edit() {
        // #compactcrdtretry: retry_crdt_merge with the live text CHANGED from the
        // compacted base (a real concurrent operator edit) must NOT rewrite the stale
        // compacted output — it fails closed with an actionable re-run message.
        let effects = RetryConvergeEffects {
            fail_times: std::sync::atomic::AtomicUsize::new(3),
            current: "prompt\noperator typed more\n".to_string(),
            converge_calls: std::sync::atomic::AtomicUsize::new(0),
        };
        let doc = std::path::Path::new("/tmp/does-not-matter.md");
        let err = converge_compacted_with_retry(&effects, doc, "compacted\n", "prompt\n")
            .expect_err(
                "a genuine concurrent edit must fail closed, not rewrite the stale compaction",
            );
        assert!(
            err.to_string()
                .contains("document changed during compaction"),
            "unexpected error: {err:#}"
        );
    }

    #[test]
    fn converge_compacted_delivery_gated_retry_keeps_both_outcomes() {
        // #compactcrdtretry: gating the retry on the controller's delivery-convergence
        // witness must not change either outcome. `/tmp/...md` has no `.agent-doc`
        // project root, so `await_delivery_convergence_for_file` returns Err (the
        // controller cannot be asked) and the retry proceeds fail-open — no live
        // controller and no real wait, which is also why the elapsed bound below holds.
        use std::sync::atomic::{AtomicUsize, Ordering};
        let doc = std::path::Path::new("/tmp/does-not-matter.md");
        let base = "prompt\n";
        let started = std::time::Instant::now();

        // Re-settled live text: the gated retry still converges.
        let resettled = RetryConvergeEffects {
            fail_times: AtomicUsize::new(1),
            current: base.to_string(),
            converge_calls: AtomicUsize::new(0),
        };
        converge_compacted_with_retry(&resettled, doc, "compacted\n", base)
            .expect("delivery-gated retry must still converge a transient retry_crdt_merge");
        assert_eq!(
            resettled.converge_calls.load(Ordering::Relaxed),
            2,
            "gating must not change the retry count"
        );

        // Genuine concurrent edit: the drift check still runs AFTER the wait and
        // still fails closed with the unchanged message.
        let drifted = RetryConvergeEffects {
            fail_times: AtomicUsize::new(3),
            current: "prompt\noperator typed more\n".to_string(),
            converge_calls: AtomicUsize::new(0),
        };
        let err = converge_compacted_with_retry(&drifted, doc, "compacted\n", base)
            .expect_err("a genuine concurrent edit must still fail closed after the wait");
        assert!(
            err.to_string()
                .contains("document changed during compaction"),
            "unexpected error: {err:#}"
        );

        assert!(
            started.elapsed() < std::time::Duration::from_secs(2),
            "the convergence gate must not block on a real wait in tests (elapsed={:?})",
            started.elapsed()
        );
    }

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
    fn compact_targets_rebase_onto_concurrent_sibling_queue_deletions() {
        // #compact-independent-cells: Compact Exchange owns only the exchange
        // cell. Queue deletions that converge while compaction is in flight are
        // authoritative sibling-cell edits and must survive both the live and
        // committed projections.
        let base = COMPACTDROPITEM_DOC.replace(
            "## Review\n",
            concat!(
                "## Queue\n\n",
                "<!-- agent:queue -->\n",
                "- do [#q1]\n",
                "- do [#q2]\n",
                "- do [#q3]\n",
                "<!-- /agent:queue -->\n\n",
                "## Review\n",
            ),
        );
        let compacted = base.replace("Response one.", "*Compacted exchange.*");
        let current = compacted.replace("- do [#q2]\n- do [#q3]\n", "");

        let rebased = CompactDocumentTargets::same(compacted)
            .rebase_onto_authoritative_siblings(&current, "exchange")
            .expect("independent queue deletions should rebase under compacted exchange");

        for projection in [&rebased.live, &rebased.committed] {
            assert!(projection.contains("*Compacted exchange.*"));
            assert!(projection.contains("- do [#q1]"));
            assert!(!projection.contains("- do [#q2]"));
            assert!(!projection.contains("- do [#q3]"));
        }
    }

    #[test]
    fn compact_targets_rebase_preserves_two_target_exchange_lineage() {
        let base = COMPACTDROPITEM_DOC.replace(
            "## Review\n",
            concat!(
                "## Queue\n\n",
                "<!-- agent:queue -->\n",
                "- do [#q1]\n",
                "- do [#q2]\n",
                "<!-- /agent:queue -->\n\n",
                "## Review\n",
            ),
        );
        let live = base.replace(
            "Response one.",
            "*Compacted exchange.*\n\nOperator prompt remains live.",
        );
        let committed = base.replace("Response one.", "*Compacted exchange.*");
        let authoritative = live.replace("- do [#q2]\n", "").replace(
            "Operator prompt remains live.",
            "Operator edited the post-boundary prompt while compact ran.",
        );

        let rebased = CompactDocumentTargets { live, committed }
            .rebase_onto_authoritative_siblings(&authoritative, "exchange")
            .expect("sibling deletion should preserve the two-target exchange lineage");

        assert!(
            rebased
                .live
                .contains("Operator edited the post-boundary prompt while compact ran.")
        );
        assert!(!rebased.live.contains("Operator prompt remains live."));
        assert!(!rebased.committed.contains("Operator prompt remains live."));
        assert!(
            !rebased
                .committed
                .contains("Operator edited the post-boundary prompt")
        );
        assert!(!rebased.live.contains("- do [#q2]"));
        assert!(!rebased.committed.contains("- do [#q2]"));
    }

    #[test]
    fn compact_targets_rebase_fails_closed_on_same_cell_exchange_drift() {
        let compacted = COMPACTDROPITEM_DOC.replace("Response one.", "*Compacted exchange.*");
        let authoritative = compacted.replace(
            "*Compacted exchange.*",
            "*Operator edited the compacted exchange summary.*",
        );

        let err = CompactDocumentTargets::same(compacted)
            .rebase_onto_authoritative_siblings(&authoritative, "exchange")
            .expect_err("same-cell exchange drift must remain fail-closed");
        assert!(err.to_string().contains("same-cell edit"), "{err:#}");
    }

    #[test]
    fn run_component_compact_does_not_drop_backlog_items() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("drop.md");
        std::fs::write(&file, COMPACTDROPITEM_DOC).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            COMPACTDROPITEM_DOC,
            agent_doc_ops_log_io::log_op,
        )
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_component_compact_partial(
            &file,
            doc,
            doc,
            PartialCompactOptions {
                target: "exchange",
                keep: 1,
                message: None,
                is_crdt: false,
                force_disk: true,
            },
        )
        .unwrap();

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

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
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

    /// `#compactboundary`: compact rebuilds the exchange body, so the live
    /// projection must carry the boundary marker forward even on the direct disk
    /// write that has no post-patch boundary repair. Commit re-mints one on the
    /// committed blob regardless, so a boundary-less live document is a permanent
    /// marker-only dirty diff that no later cycle heals.
    #[test]
    fn full_exchange_compact_keeps_boundary_marker_in_live_document() {
        let prompt = "Compact exchange must not orphan the boundary marker.";
        let doc = format!(
            concat!(
                "---\nagent_doc_session: test-compact-boundary\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_component_compact_force_disk(&file, &doc, "exchange", None, false).unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let exchange = agent_doc_element::element::parse(&result)
            .unwrap()
            .into_iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&result)
            .to_string();

        // The existing marker id is carried forward rather than re-minted, so the
        // compact does not churn the boundary identity.
        let marker = "<!-- agent:boundary:abc123 -->";
        let marker_at = exchange.find(marker).unwrap_or_else(|| {
            panic!("live compacted exchange must keep the boundary marker:\n{exchange}")
        });
        let prompt_at = exchange
            .find(prompt)
            .expect("live compacted exchange must keep the unresolved prompt");
        assert!(
            prompt_at < marker_at,
            "boundary must land at the end of the exchange, matching every other write path:\n{exchange}"
        );
        assert_eq!(
            exchange.matches("<!-- agent:boundary:").count(),
            1,
            "compact must leave exactly one boundary marker:\n{exchange}"
        );

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
        assert!(
            !snapshot_after.contains(prompt),
            "unresolved prompt must remain live drift:\n{snapshot_after}"
        );
    }

    /// `#compactboundary`, partial (`--keep N`) variant.
    #[test]
    fn partial_exchange_compact_keeps_boundary_marker_in_live_document() {
        let prompt = "Partial compact must not orphan the boundary marker.";
        let doc = format!(
            concat!(
                "---\nagent_doc_session: test-partial-boundary\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: first topic\n\nResponse one.\n\n",
                "### Re: second topic\n\nResponse two.\n",
                "<!-- agent:boundary:def456 -->\n",
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_component_compact_partial(
            &file,
            &doc,
            &doc,
            PartialCompactOptions {
                target: "exchange",
                keep: 1,
                message: None,
                is_crdt: false,
                force_disk: true,
            },
        )
        .unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        let marker = "<!-- agent:boundary:def456 -->";
        let marker_at = result
            .find(marker)
            .unwrap_or_else(|| panic!("partial compact must keep the boundary marker:\n{result}"));
        let prompt_at = result
            .find(prompt)
            .expect("partial compact must keep the unresolved prompt");
        assert!(
            prompt_at < marker_at,
            "boundary must land at the end of the exchange:\n{result}"
        );
        assert_eq!(
            result.matches("<!-- agent:boundary:").count(),
            1,
            "partial compact must leave exactly one boundary marker:\n{result}"
        );

        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
        assert!(
            !snapshot_after.contains(prompt),
            "unresolved prompt must remain live drift:\n{snapshot_after}"
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
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
            agent_doc_snapshot_io::load_document_baseline(&file)
                .unwrap()
                .unwrap(),
            compacted
        );
    }

    #[test]
    fn component_compact_force_disk_rebuilds_live_canonical_from_compacted_document() {
        let doc = concat!(
            "---\nagent_doc_session: test-force-disk-canonical\nagent_doc_format: template\n---\n\n",
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
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("plugin-owner")).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let _editor = CompactTestEditorBuffer::attach(&file, COMPACT_TEST_EDITOR_ID, doc).unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap();

        let compacted = std::fs::read_to_string(&file).unwrap();
        assert!(compacted.contains("Compacted summary."));
        assert!(!compacted.contains("### Re: topic one"));
        let current = agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap();
        match current {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => {
                assert_eq!(text, compacted);
                assert!(
                    !text.contains("### Re: topic one"),
                    "force-disk compact must remove archived response cells from live canonical text"
                );
            }
            other => panic!("expected live relay current text, got {other:?}"),
        }
        let ops_log =
            std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap_or_default();
        assert!(
            ops_log.contains("compact_force_disk_reconciled_canonical"),
            "force-disk compact must record canonical reconcile:\n{ops_log}"
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
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
            agent_doc_snapshot_io::load_document_baseline(&file)
                .unwrap()
                .unwrap(),
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
    fn compact_exchange_archives_active_capture_missing_from_current_projection() {
        let doc = concat!(
            "---\nagent_doc_session: test-captured-compact\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: older topic\n\nOlder response.\n\n",
            "❯ describe the recovery\n",
            "<!-- /agent:exchange -->\n",
        );
        let response = concat!(
            "<!-- patch:exchange -->\n",
            "### Re: describe the recovery — gpt-5\n\n",
            "Captured response that never reached the current projection.\n",
            "<!-- /patch:exchange -->\n",
        );
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_compact_test_repo(root);
        let file = root.join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = root.join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("logs")).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git_commit_file(root, "test.md");
        agent_doc_cycle_state_io::start_preflight(&file, Some(doc), Some(doc)).unwrap();
        agent_doc_capture_io::capture_response_with_current_content(&file, response, doc).unwrap();

        run(
            &file,
            None,
            Some("exchange"),
            None,
            Some("skip"),
            true,
            true,
        )
        .unwrap();

        let archive = std::fs::read_dir(agent_doc_dir.join("archives"))
            .unwrap()
            .filter_map(Result::ok)
            .find(|entry| entry.path().extension().and_then(|value| value.to_str()) == Some("md"))
            .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
            .expect("compact archive");
        assert!(archive.contains("Captured response that never reached"));
        let head = agent_doc_git_io::revision::show_head(&file)
            .unwrap()
            .expect("compacted document in HEAD");
        assert!(head.contains("*Compacted. Content archived to `"));
        assert!(!head.contains("Captured response that never reached"));
        let cycle = agent_doc_cycle_state_io::load(&file).unwrap().unwrap();
        assert_eq!(cycle.phase, agent_doc_turn::CyclePhase::Committed);
        let ops = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert!(ops.contains("compact_active_capture_composed"));
        assert!(!ops.contains("commit_blocked_missing_captured_response"));
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            live,
            "detached-disk compact must not overwrite a prompt typed after compaction was computed"
        );
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&file)
                .unwrap()
                .unwrap(),
            doc,
            "failed compact must not advance the snapshot"
        );
        let ops_log = std::fs::read_to_string(agent_doc_dir.join("logs/ops.log")).unwrap();
        assert_stale_compact_source_refusal(&err, &ops_log);
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some(""), false).unwrap();

        let cleared = std::fs::read_to_string(&file).unwrap();
        assert!(
            !cleared.contains("stale topic"),
            "cleared exchange must not leave stale content in the visible file:\n{cleared}"
        );
        let snapshot_after = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
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
    fn compact_advances_baseline_and_cold_recovery_projection() {
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let legacy = agent_doc_merge::crdt::CrdtDoc::from_text(&doc).encode_state();
        agent_doc_snapshot_io::checkpoint_crdt_recovery_projection(
            &file,
            &legacy,
            "test:pre-compact",
        )
        .unwrap();

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
        let snap = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
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

        // The cold restart projection must equal the compacted text.
        let recovery = agent_doc_snapshot_io::load_crdt_recovery_projection(&file)
            .unwrap()
            .unwrap();
        let projected = agent_doc_document_realtime::crdt_relay::RelayHub::recover_from_projection(
            1,
            &recovery.projection,
        )
        .unwrap()
        .canonical_text();
        assert!(
            !projected.contains("topic 0"),
            "recovery projection must contain the compacted document, not the pre-compaction one:\n{projected}"
        );
        assert_eq!(
            projected, visible,
            "recovery projection must equal the compacted visible document"
        );

        // With the durable baseline advanced, the stale-snapshot drift guard must not bail.
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let err = run_component_compact(&file, doc, "exchange", Some("Compacted summary."), false)
            .unwrap_err();

        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            live,
            "detached-disk compact must not overwrite scratch comments typed after compaction was computed"
        );
        assert_eq!(
            agent_doc_snapshot_io::load_document_baseline(&file)
                .unwrap()
                .unwrap(),
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
            agent_doc_snapshot_io::load_document_baseline(&file)
                .unwrap()
                .unwrap(),
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
    fn exchange_compact_preserves_durable_context_handles_without_payload() {
        let doc = concat!(
            "---\nagent_doc_session: context-session\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: first topic\n\nFirst response.\n\n",
            "### Re: second topic\n\nSecond response.\n",
            "<!-- /agent:exchange -->\n",
        );
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("test.md");
        std::fs::write(&file, doc).unwrap();
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let mut conn = agent_doc_sqlite::state_store::open_state_db(dir.path()).unwrap();
        agent_doc_sqlite::context_injection_ledger::record_context_manifest(
            &mut conn,
            &agent_doc_sqlite::context_injection_ledger::ContextManifestWrite {
                document_id: agent_doc_hash::document_id_for_path(&file),
                session_id: "context-session".to_string(),
                cycle_id: "cycle-context".to_string(),
                cycle_state: "preflight_started".to_string(),
                harness: "codex".to_string(),
                prompt_fingerprint: "fingerprint-context".to_string(),
                pack_ids: vec!["pack-context".to_string()],
                token_count: 12,
                injections: vec![
                    agent_doc_sqlite::context_injection_ledger::ContextInjectionWrite {
                        pack_id: "pack-context".to_string(),
                        chunk_id: "chunk-context".to_string(),
                        content_hash: "hash-context".to_string(),
                        source_uri: "src/context.rs".to_string(),
                        range_start: Some(4),
                        range_end: Some(8),
                        injection_mode:
                            agent_doc_sqlite::context_injection_ledger::ContextInjectionMode::Expanded,
                    },
                ],
            },
        )
        .unwrap();
        drop(conn);

        run_component_compact_force_disk(&file, doc, "exchange", None, false).unwrap();

        let compacted = std::fs::read_to_string(&file).unwrap();
        let components = element::parse(&compacted).unwrap();
        let exchange = components
            .iter()
            .find(|component| component.name == "exchange")
            .unwrap()
            .content(&compacted);
        assert!(exchange.contains("<dynamic_context_ref"));
        assert!(exchange.contains("tsift://pack-context/chunk-context"));
        assert!(exchange.contains("modes=\"expanded:1\""));
        assert!(!exchange.contains("<context_chunk"));
        assert!(!exchange.contains("First response."));
        assert!(!exchange.contains("Second response."));
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some("Compacted."), false)
            .unwrap();

        let result = std::fs::read_to_string(&file).unwrap();
        assert!(result.contains("No open backlog items."));
        assert!(!result.contains("Top backlog item: #done."));
        let snap = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
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

        // Set up durable baseline and archive storage.
        let agent_doc_dir = dir.path().join(".agent-doc");
        std::fs::create_dir_all(agent_doc_dir.join("snapshots")).unwrap();
        std::fs::create_dir_all(agent_doc_dir.join("archives")).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let file_before = std::fs::read_to_string(&file).unwrap();

        run_component_compact_force_disk(&file, doc, "exchange", Some("Summary."), false).unwrap();

        // After compact: file and snapshot should match
        let file_after = std::fs::read_to_string(&file).unwrap();
        let snapshot_content = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();

        assert_eq!(
            file_after, snapshot_content,
            "file and snapshot must match after compact"
        );

        // Verify the document was actually modified
        assert_ne!(file_before, file_after);
        assert!(file_after.contains("Summary."));
    }

    #[test]
    fn compact_with_commit_does_not_write_vcs_refresh_sidecar() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
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
        assert!(!signal.exists(), "VCS refresh must use the typed endpoint");

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

    /// `#bbdcompactnocommit`: a `--commit` compact that cannot observe its own
    /// change must never report success.
    ///
    /// Operator-reported on brookebrodack-dev.md while queue lines were being
    /// deleted mid-compact: ops.log showed `delivery_converged=false
    /// live_editors=1`, the compacted document and archive landed on disk, and
    /// the commit simply never happened. The commit leg was
    /// `if commit { if dirty { ... } }` with no else, so an unobservable change
    /// fell through and returned Ok.
    ///
    /// The genuine no-op returns earlier (`targets.live == content &&
    /// targets.committed == content`), so `!dirty` at the commit gate means the
    /// change could not be OBSERVED, not that there was none — which is exactly
    /// the case that must fail loudly.
    #[test]
    fn compact_commit_leg_has_no_silent_skip() {
        let source = include_str!("lib.rs");
        let commit_leg = source
            .split("    if commit {")
            .nth(1)
            .expect("the commit gate must exist");

        assert!(
            commit_leg.contains("compact_commit_unobservable"),
            "an unobservable --commit compact must be recorded, not silently skipped"
        );
        assert!(
            commit_leg.contains("compact_commit_observe_retry"),
            "an unobservable --commit compact must retry for convergence first"
        );
        // The load-bearing part: the failure path must actually return an error.
        let unobservable_idx = commit_leg
            .find("compact_commit_unobservable")
            .expect("checked above");
        let bail_idx = commit_leg[unobservable_idx..]
            .find("anyhow::bail!")
            .expect("the unobservable path must fail loudly");
        assert!(
            bail_idx < 800,
            "the bail must follow the unobservable log, not be some later unrelated error"
        );
    }

    /// The retry budget must be bounded — a compact that hangs waiting for an
    /// editor that never settles is no better than one that silently skips.
    #[test]
    fn compact_commit_observe_retry_is_bounded() {
        assert!(COMPACT_COMMIT_OBSERVE_ATTEMPTS >= 1);
        assert!(
            COMPACT_COMMIT_OBSERVE_ATTEMPTS <= 10,
            "the observe retry must stay bounded"
        );
        assert!(
            COMPACT_COMMIT_OBSERVE_BACKOFF <= std::time::Duration::from_millis(500),
            "per-attempt backoff must stay small enough to bound the whole gate"
        );
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
    fn compact_passive_poll_does_not_recreate_an_evicted_zero_editor_relay() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let file = root.join("session.md");
        fs::write(&file, PRECOMPACT_DOC).unwrap();

        // Preserve durable editor ownership while removing the relay member. This
        // models the frozen phantom-lease fallback that motivated the original
        // authoritative adopt.
        let editor =
            CompactTestEditorBuffer::attach(&file, "compact-stale-zero-editor", PRECOMPACT_DOC)
                .unwrap();
        assert!(
            agent_doc_crdt_relay_io::deregister_replica_for_file(&file, &editor.replica_identity,)
                .unwrap()
        );

        let live = COMPACTED_DOC.replace(
            "<!-- /agent:exchange -->\n",
            "unresolved operator prompt\n<!-- /agent:exchange -->\n",
        );
        ensure_compact_live_relay_target(&file, &live).unwrap();

        let current = agent_doc_crdt_relay_io::current_text_for_file(&file).unwrap();
        assert_eq!(
            current,
            agent_doc_crdt_relay_io::CurrentText::EditorAttachedMissingReplica,
            "compact's fail-open missing-model path and the stale delivery poll must not recreate a disk-seeded phantom hub"
        );
    }

    #[test]
    fn compact_matching_text_still_requires_delivery_ack_before_commit_effects() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        let file = root.join("session.md");
        fs::write(&file, PRECOMPACT_DOC).unwrap();
        let _editor = CompactTestEditorBuffer::attach_with_delivery_pump(
            &file,
            "compact-unacked-target",
            PRECOMPACT_DOC,
            false,
        )
        .unwrap();

        let write = agent_doc_crdt_relay_io::apply_cp_write_for_file(
            &file,
            PRECOMPACT_DOC,
            COMPACTED_DOC,
            "compact_unacked_target_test",
        )
        .unwrap()
        .expect("attached editor should receive a CRDT target");
        assert!(!write.delivery_converged);

        let error = ensure_compact_live_relay_target(&file, COMPACTED_DOC)
            .expect_err("matching canonical bytes without the editor ACK must fail closed");
        assert!(
            format!("{error:#}").contains("remained unacknowledged"),
            "compact must report ACK backpressure, not claim matching text converged: {error:#}"
        );
        let ops = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default();
        assert!(
            !ops.contains("action=already_converged"),
            "matching text must not publish a converged compact fact before delivery ACK: {ops}"
        );
    }

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            PRECOMPACT_DOC,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git_commit_file(root, "session.md"); // HEAD = pre-compact

        // Editor/plugin flushed the compacted content to disk...
        fs::write(&file, COMPACTED_DOC).unwrap();
        // ...but a stale-supervisor CRDT replay reverted the snapshot to pre-compact.
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            PRECOMPACT_DOC,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        // Authoritative content is known in run() from the compaction itself.
        commit_compacted_authoritative(&file, COMPACTED_DOC, COMPACTED_DOC).unwrap();

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
    fn verify_compact_head_landed_bails_on_genuine_mismatch() {
        use std::fs;
        // The fail-closed safety net is preserved: when HEAD genuinely does not
        // hold the authoritative compacted content (e.g. the commit never landed),
        // `verify_compact_head_landed` bails with the recovery-command error so the
        // caller cannot silently report a successful compaction.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_compact_test_repo(root);

        let file = root.join("session.md");
        fs::write(&file, PRECOMPACT_DOC).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            PRECOMPACT_DOC,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git_commit_file(root, "session.md"); // HEAD = pre-compact

        let err = verify_compact_head_landed(&file, COMPACTED_DOC).unwrap_err();
        assert!(
            err.to_string()
                .contains("did not land the compacted content"),
            "expected fail-closed HEAD-mismatch error, got: {err}"
        );
    }

    #[test]
    fn compact_commit_lands_head_when_disk_lags_at_precompact() {
        use std::fs;
        // `#jb-compact-commit-left-uncommitted`, fail-OPEN path (design intent at
        // `apply_compacted_document`: "if the editor cannot flush,
        // `commit_compacted_authoritative` still stage[s] the authoritative
        // snapshot and verify[ies] HEAD"): even when the working-tree file still
        // equals HEAD (a pure editor-IPC lag with no flush), the authoritative
        // compacted content is explicitly known and re-asserted to the snapshot, so
        // the commit stages that snapshot and lands the compacted content in HEAD
        // rather than letting the historical-drift repair revert it and leave
        // uncommitted compaction drift. Landing is strictly better than the former
        // fail-closed recovery dance; `verify_compact_head_landed` still fails
        // closed only if HEAD genuinely does not match afterward.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        init_compact_test_repo(root);

        let file = root.join("session.md");
        fs::write(&file, PRECOMPACT_DOC).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            PRECOMPACT_DOC,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git_commit_file(root, "session.md"); // HEAD = pre-compact
        // Disk still lags (editor holds the compacted buffer, no flush yet).
        assert_eq!(fs::read_to_string(&file).unwrap(), PRECOMPACT_DOC);

        commit_compacted_authoritative(&file, COMPACTED_DOC, COMPACTED_DOC)
            .expect("authoritative compaction must land the compacted content in HEAD");
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
    fn compact_with_commit_converges_committed_response_head_without_historical_drift_guard() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/plugin-owner")).unwrap();

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let _initial_editor =
            CompactTestEditorBuffer::attach(&file, COMPACT_TEST_EDITOR_ID, doc).unwrap();
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
            agent_doc_snapshot_io::load_document_baseline(&file)
                .unwrap()
                .unwrap(),
            committed,
            "post-compact snapshot must match the committed document"
        );

        let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("compact_writeback")
                && ops_log.contains("transport=crdt_relay")
                && ops_log.contains("secondary_transport=none"),
            "fixture should prove the Compact Exchange CRDT-only path:\n{ops_log}"
        );
        assert!(
            !ops_log.contains("commit_blocked_committed_historical_patchback")
                && !ops_log.contains("typed_component_drift")
                && !ops_log.contains("refusing to auto-adopt committed historical response"),
            "clean exchange-only compact must not trip the historical response drift guard:\n{ops_log}"
        );
    }

    #[test]
    fn compact_replays_pending_queue_delete_before_rewrite_and_consumes_after_success() {
        use agent_doc_merge::crdt::EditorOp;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let file = root.join("session.md");
        let deleted_queue_row = "- [ ] [#struck] resurrected work\n";
        let kept_queue_row = "- [ ] [#keep] retained work\n";
        let doc = format!(
            concat!(
                "---\nagent_doc_session: test-compact-editor-cut\nagent_doc_format: template\n---\n\n",
                "## Exchange\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: older — gpt-5\n\nEarlier response.\n\n",
                "### Re: newer — gpt-5\n\nNewer response.\n",
                "<!-- /agent:exchange -->\n\n",
                "## Queue\n\n",
                "<!-- agent:queue go -->\n",
                "{}{}",
                "<!-- /agent:queue -->\n",
            ),
            deleted_queue_row, kept_queue_row,
        );
        std::fs::write(&file, &doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            &doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let delete_offset = doc.find(deleted_queue_row).unwrap();
        agent_doc_op_capture_io::record_editor_op(
            &file,
            &agent_doc_hash::content_hash(&doc),
            EditorOp::Delete {
                offset: delete_offset,
                len: deleted_queue_row.len(),
            },
        )
        .unwrap();

        run_in_controller(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            false,
            false,
        )
        .unwrap();

        let visible = std::fs::read_to_string(&file).unwrap();
        assert!(visible.contains("Compacted summary."), "{visible}");
        assert!(!visible.contains(deleted_queue_row), "{visible}");
        assert!(visible.contains(kept_queue_row), "{visible}");
        let snapshot = agent_doc_snapshot_io::load_document_baseline(&file)
            .unwrap()
            .unwrap();
        assert!(!snapshot.contains(deleted_queue_row), "{snapshot}");
        assert!(snapshot.contains(kept_queue_row), "{snapshot}");
        assert!(
            agent_doc_op_capture_io::load_op_capture(&file)
                .unwrap()
                .is_none(),
            "a successfully applied compact must consume the replayed editor-op epoch"
        );
        let ops_log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            ops_log.contains("compact_pending_editor_cut_replayed")
                && ops_log.contains("compact_pending_editor_cut_consumed"),
            "{ops_log}"
        );
    }

    #[test]
    fn compact_preserves_pending_editor_ops_when_replayed_cut_is_invalid() {
        use agent_doc_merge::crdt::EditorOp;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        std::fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();

        let file = root.join("session.md");
        let exchange_close = "<!-- /agent:exchange -->\n";
        let doc = concat!(
            "---\nagent_doc_session: test-compact-editor-cut-failure\nagent_doc_format: template\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: response — gpt-5\n\nResponse.\n",
            "<!-- /agent:exchange -->\n",
        );
        std::fs::write(&file, doc).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        agent_doc_op_capture_io::record_editor_op(
            &file,
            &agent_doc_hash::content_hash(doc),
            EditorOp::Delete {
                offset: doc.find(exchange_close).unwrap(),
                len: exchange_close.len(),
            },
        )
        .unwrap();

        let err = run_in_controller(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            false,
            false,
        )
        .unwrap_err();
        assert!(
            err.to_string().contains("component")
                || err.to_string().contains("marker")
                || err.to_string().contains("integrity"),
            "{err:#}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), doc);
        assert!(
            agent_doc_op_capture_io::load_op_capture(&file)
                .unwrap()
                .is_some(),
            "a failed compact must retain the editor-op epoch for recovery"
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
            source,
            CompactApplyOptions {
                target_component: Some("exchange"),
                refresh_crdt: false,
                force_disk: false,
            },
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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

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

    const COMPACT_TEST_EDITOR_ID: &str = "compact-test-listener";

    struct CompactTestEditorBuffer {
        content: String,
        replica_identity: String,
        replica: agent_doc_merge::crdt_sync::ReplicaState,
    }

    impl CompactTestEditorBuffer {
        fn attach(file: &Path, editor_id: &str, seed: &str) -> Result<Self> {
            Self::attach_with_delivery_pump(file, editor_id, seed, true)
        }

        fn attach_with_delivery_pump(
            file: &Path,
            editor_id: &str,
            seed: &str,
            start_delivery_pump: bool,
        ) -> Result<Self> {
            let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
            let document_hash = agent_doc_hash::document_id_for_path(&canonical);
            let liveness_ops = vec![
                agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                    document_hash: document_hash.clone(),
                    pid: std::process::id().into(),
                    tag: format!("compact-test:{editor_id}:{}", canonical.display()),
                },
                agent_doc_reliable_sync_io::liveness::LivenessOp::Register(
                    agent_doc_reliable_sync_io::liveness::EditorRegistration {
                        document_hash: document_hash.clone(),
                        pid: std::process::id().into(),
                        path: canonical.to_string_lossy().into_owned(),
                        editor_id: editor_id.to_string(),
                        editor_kind: "test".to_string(),
                        editor_version: "test".to_string(),
                        capabilities: vec![
                            agent_doc_document_realtime::editor_contract::OPERATOR_TEXT_AUTHORITY_CAPABILITY.to_string(),
                            agent_doc_document_realtime::editor_contract::LAZILY_TRANSPORT_RECEIPTS_CAPABILITY.to_string(),
                        ],
                        timestamp_ms: 1,
                    },
                ),
            ];
            agent_doc_reliable_sync_io::global_liveness_plane()
                .lock()
                .restore_liveness(&liveness_ops);
            let project_root = agent_doc_fs::find_project_root(&canonical)
                .context("compact test could not resolve project root")?;
            let outbox = lazily::SqliteOutbox::open(
                &agent_doc_sqlite::state_store::state_db_path(&project_root),
                document_hash.clone(),
            )?;
            let mut endpoint =
                agent_doc_reliable_sync_io::push::LivenessPushEndpoint::new(document_hash, outbox);
            endpoint.enqueue(&liveness_ops)?;
            let transport =
                agent_doc_controller_io::project_controller::RpcLivenessPushTransport::new(
                    &project_root,
                );
            let progress = endpoint.flush(&transport)?;
            anyhow::ensure!(
                !progress.stalled && progress.retained == 0,
                "compact test reliable-sync registration did not reach controller"
            );
            let replica_identity = format!("{}:{}", editor_id, canonical.display());
            let (client_id, bootstrap) =
                agent_doc_crdt_relay_io::register_replica_for_file(file, &replica_identity)?
                    .context("compact test editor could not register CRDT replica")?;
            let replica =
                agent_doc_merge::crdt_sync::ReplicaState::from_encoded(client_id, &bootstrap)?;
            let mut editor = Self {
                content: replica.text(),
                replica_identity,
                replica,
            };
            if editor.content != seed {
                editor.publish(file, seed)?;
            }
            if start_delivery_pump {
                let _ = agent_doc_test_support::start_crdt_delivery_pump(
                    file,
                    &editor.replica_identity,
                );
            }
            Ok(editor)
        }

        fn publish(&mut self, file: &Path, content: &str) -> Result<()> {
            let delete_len = self.content.len() as u32;
            self.replica.apply_local_edit(0, delete_len, content);
            let update = self.replica.encode_state();
            agent_doc_crdt_relay_io::relay_replica_update_for_file(
                file,
                &self.replica_identity,
                &update,
            )?
            .context("compact test editor CRDT relay update refused")?;
            self.content = content.to_string();
            Ok(())
        }
    }

    fn record_compact_lazily_receipt(file: &Path, patch_id: &str, content: &str) -> Option<()> {
        agent_doc_controller_io::project_controller::record_visible_write_commit_candidate_for_file(
            file, patch_id, content, "compact_test",
        )
        .ok()?;
        Some(())
    }

    fn start_component_patch_visible_write_listener(root: &Path) -> std::thread::JoinHandle<()> {
        use parking_lot::Mutex;
        use std::collections::HashMap;
        use std::sync::Arc;
        let root = root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            let buffers: Arc<Mutex<HashMap<String, CompactTestEditorBuffer>>> =
                Arc::new(Mutex::new(HashMap::new()));
            let _ = agent_doc_ipc_io::start_listener(&root, move |msg| {
                let payload: serde_json::Value = serde_json::from_str(msg).ok()?;
                let patch_id = payload
                    .get("patch_id")
                    .and_then(|value| value.as_str())
                    .unwrap_or("unknown");
                if payload.get("type").and_then(|value| value.as_str()) == Some("save_document") {
                    let file_path = payload.get("file").and_then(|value| value.as_str())?;
                    let content =
                        match agent_doc_crdt_relay_io::current_text_for_file(Path::new(file_path))
                            .ok()?
                        {
                            agent_doc_crdt_relay_io::CurrentText::Current {
                                text,
                                delivery_converged: true,
                                ..
                            } => text,
                            _ => std::fs::read_to_string(file_path).ok()?,
                        };
                    std::fs::write(file_path, &content).ok()?;
                    record_compact_lazily_receipt(Path::new(file_path), patch_id, &content)?;
                    return Some(
                        serde_json::json!({"type": "receipt", "status": "applied", "id": patch_id})
                            .to_string(),
                    );
                }
                let baseline = payload.get("baseline")?.as_str()?;
                let mut content = baseline.to_string();
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
                    let mut buffers = buffers.lock();
                    if !buffers.contains_key(file_path) {
                        buffers.insert(
                            file_path.to_string(),
                            CompactTestEditorBuffer::attach(
                                Path::new(file_path),
                                COMPACT_TEST_EDITOR_ID,
                                baseline,
                            )
                            .ok()?,
                        );
                    }
                    buffers
                        .get_mut(file_path)?
                        .publish(Path::new(file_path), &content)
                        .ok()?;
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
        use parking_lot::Mutex;
        use std::collections::HashMap;
        use std::sync::Arc;
        let root = root.to_path_buf();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::thread::spawn(move || {
            // Per-file in-memory editor buffer, seeded lazily from the patch baseline.
            let buffers: Arc<Mutex<HashMap<String, CompactTestEditorBuffer>>> =
                Arc::new(Mutex::new(HashMap::new()));
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
                        let content = match agent_doc_crdt_relay_io::current_text_for_file(
                            Path::new(file_path),
                        )
                        .ok()?
                        {
                            agent_doc_crdt_relay_io::CurrentText::Current {
                                text,
                                delivery_converged: true,
                                ..
                            } => text,
                            _ => buffers
                                .lock()
                                .get(file_path)
                                .map(|buffer| buffer.content.clone())
                                .or_else(|| std::fs::read_to_string(file_path).ok())?,
                        };
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
                            let mut buffers = buffers.lock();
                            if !buffers.contains_key(file_path) {
                                buffers.insert(
                                    file_path.to_string(),
                                    CompactTestEditorBuffer::attach(
                                        Path::new(file_path),
                                        COMPACT_TEST_EDITOR_ID,
                                        payload.get("baseline")?.as_str()?,
                                    )
                                    .ok()?,
                                );
                            }
                            buffers
                                .get_mut(file_path)?
                                .publish(Path::new(file_path), &content)
                                .ok()?;
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
        fs::create_dir_all(root.join(".agent-doc/plugin-owner")).unwrap();

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
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            doc,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        let _initial_editor =
            CompactTestEditorBuffer::attach(&file, COMPACT_TEST_EDITOR_ID, doc).unwrap();
        git(root, &["add", "session.md"]);
        git(root, &["commit", "-q", "-m", "seed"]);

        let _listener = start_buffer_only_patch_listener(root);
        crate::test_support::wait_for_live_prompt_drift_listener(root);

        if let Err(err) = run(
            &file,
            None,
            Some("exchange"),
            Some("Compacted summary."),
            Some("skip"),
            true,
            false,
        ) {
            let ops_log =
                fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default();
            eprintln!("compact_with_commit_flushes_editor_buffer_to_disk ops log:\n{ops_log}");
            panic!("editor-IPC compact --commit must succeed: {err:?}");
        }

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
            (ops_log.contains("compact_editor_buffer_flush")
                && (ops_log.contains("transport=save_document")
                    || ops_log.contains("transport=already_disk")))
                || (ops_log.contains("native_editor_save_settled")
                    && ops_log.contains("transport=crdt_editor_native_save"))
                || ops_log.contains("compact_writeback file=")
                    && ops_log.contains("transport=crdt_relay"),
            "compact --commit must converge the editor buffer to disk:\n{ops_log}"
        );
    }

    #[test]
    fn compact_commit_preserves_only_unresolved_prompt_in_live_editor() {
        // #jb-compact-two-target-lineage: Compact Exchange has two intentional
        // projections when operator input follows the exchange boundary:
        // HEAD/snapshot omit that unresolved prompt, while the live editor keeps
        // it on top of the compacted history. The commit must not reset the live
        // relay to the HEAD-only projection and force a whole-buffer rebootstrap;
        // that reset lets delayed JetBrains document events replay the old editor
        // lineage and resurrect archived exchange content.
        use agent_doc_document::transient_markers::normalize_transient_agent_doc_markers;
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        fs::create_dir_all(root.join(".agent-doc/snapshots")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/patches")).unwrap();
        fs::create_dir_all(root.join(".agent-doc/plugin-owner")).unwrap();

        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);

        let file = root.join("session.md");
        let committed = concat!(
            "---\nagent_doc_session: test-two-target\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: archived-one — gpt-5\n\nOld response one.\n\n",
            "### Re: archived-two — gpt-5\n\nOld response two.\n",
            "<!-- agent:boundary:deadbeef:before-compact -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let prompt = "Fix the next binary issue without restoring git HEAD.";
        let live = committed.replace(
            "<!-- agent:boundary:deadbeef:before-compact -->\n",
            &format!("<!-- agent:boundary:deadbeef:before-compact -->\n{prompt}\n"),
        );

        fs::write(&file, committed).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            committed,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        git(root, &["add", "session.md"]);
        git(root, &["commit", "-q", "-m", "seed"]);

        // Model a JetBrains buffer with a newly typed, unresolved prompt. The
        // prompt is live editor drift and therefore absent from HEAD/snapshot.
        fs::write(&file, &live).unwrap();
        let _editor = CompactTestEditorBuffer::attach(&file, COMPACT_TEST_EDITOR_ID, &live)
            .expect("attach live editor replica");
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
        .expect("two-target Compact Exchange commit must succeed");

        let head = agent_doc_git_io::revision::show_head(&file)
            .unwrap()
            .expect("compacted HEAD");
        assert!(head.contains("Compacted summary."), "{head}");
        assert!(
            !head.contains(prompt),
            "unresolved prompt must not enter HEAD:\n{head}"
        );
        assert!(
            !head.contains("Old response one."),
            "archived history remained in HEAD:\n{head}"
        );

        let live_after = match agent_doc_crdt_relay_io::current_text_for_file(&file)
            .expect("resolve live relay after compact")
        {
            agent_doc_crdt_relay_io::CurrentText::Current { text, .. } => text,
            other => panic!("live relay must remain attached after compact: {other:?}"),
        };
        assert!(
            live_after.contains(prompt),
            "the unresolved prompt must remain in the live compacted editor:\n{live_after}"
        );
        assert!(
            !live_after.contains("Old response one.") && !live_after.contains("Old response two."),
            "archived history must not survive in the live editor:\n{live_after}"
        );

        let recovery = agent_doc_snapshot_io::load_crdt_recovery_projection(&file)
            .unwrap()
            .expect("post-compact recovery projection");
        // Recovery checkpoints persist the relay's compact ReplicaState
        // (`ADCR1`), the same projection the cold-start runtime consumes.
        let recovery_markdown =
            agent_doc_document_realtime::crdt_relay::RelayHub::recover_from_projection(
                1,
                &recovery.projection,
            )
            .unwrap()
            .canonical_text();
        assert_eq!(
            normalize_transient_agent_doc_markers(&recovery_markdown),
            normalize_transient_agent_doc_markers(&live_after),
            "cold recovery projection must match the live compacted editor"
        );

        let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
        assert!(
            !ops_log
                .lines()
                .any(|line| line.contains("crdt_adopt_authoritative_text")
                    && line.contains("changed=true")),
            "compact commit must not reset a converged live editor to the HEAD-only projection:\n{ops_log}"
        );
    }

    #[test]
    fn compact_integrity_gate_fails_before_tag_archive_or_document_mutation() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("session.md");
        let malformed = concat!(
            "---\nagent_doc_session: test\nagent_doc_lint_dialect: off\n---\n\n",
            "<!-- agent:exchange -->\n",
            "<!-- agent:notes -->\ncorrupted\n",
            "<!-- /agent:exchange -->\n",
            "<!-- /agent:notes -->\n",
        );
        fs::write(&doc, malformed).unwrap();

        let err = run(&doc, Some(1), Some("exchange"), None, None, true, false)
            .expect_err("compact must reject malformed component authority");
        assert!(err.to_string().contains("[integrity-gate] INTERRUPTED"));
        assert_eq!(fs::read_to_string(&doc).unwrap(), malformed);
        assert!(!dir.path().join(".agent-doc/archives").exists());
    }

    #[test]
    fn no_op_compact_does_not_create_a_tag_or_commit_dirty_document() {
        use std::fs;
        use std::process::Command;

        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let doc = root.join("session.md");
        let initial = concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: only topic — gpt-5\n\ncomplete\n",
            "<!-- /agent:exchange -->\n",
        );
        fs::write(&doc, initial).unwrap();
        git(root, &["init", "-q"]);
        git(root, &["config", "user.email", "test@example.com"]);
        git(root, &["config", "user.name", "Test"]);
        git(root, &["add", "session.md"]);
        git(root, &["commit", "-q", "-m", "seed"]);
        let head_before = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        let dirty = initial.replace("complete", "complete but locally annotated");
        fs::write(&doc, &dirty).unwrap();

        run(&doc, Some(1), Some("exchange"), None, None, true, false)
            .expect("semantic no-op compact should return cleanly");

        let head_after = Command::new("git")
            .current_dir(root)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout;
        assert_eq!(head_after, head_before);
        assert_eq!(fs::read_to_string(&doc).unwrap(), dirty);
        let tags = Command::new("git")
            .current_dir(root)
            .args(["tag", "--list", "agent-doc/*"])
            .output()
            .unwrap()
            .stdout;
        assert!(
            tags.is_empty(),
            "no-op compact must not create a checkpoint tag"
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
