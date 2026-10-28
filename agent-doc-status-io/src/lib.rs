//! Status component writeback adapters.
//!
//! Pure status projection lives in `agent-doc-document`. This crate owns the
//! command flow for `write --status`: read the document, apply the focused
//! projection, choose forced disk write vs editor-aware convergence, and emit
//! the status-write audit marker through injected orchestration effects.

use anyhow::{Context, Result};
use std::path::Path;

/// Effects required by status writeback.
pub trait StatusWriteEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_or_disk_write(
        &self,
        file: &Path,
        previous: &str,
        updated: &str,
        phase: &str,
    ) -> Result<()>;

    fn record_document_write_provenance(&self, file: &Path, updated: &str);

    fn attribute_agent_write_to_baseline(&self, file: &Path, before: &str, after: &str);

    /// `#statussyncrevert`: record the landed `status:sync` payload as the
    /// latest synced value, so a later editor change to the block (an IDE
    /// undo, a hand edit) is recognized as a revert the next checkpoint must
    /// re-sync over.
    fn record_status_sync_synced(&self, file: &Path, landed: &str);

    fn log_op(&self, file: &Path, message: &str);
}

pub struct RuntimeStatusWriteEffects;

pub static RUNTIME_STATUS_WRITE_EFFECTS: RuntimeStatusWriteEffects = RuntimeStatusWriteEffects;

impl StatusWriteEffects for RuntimeStatusWriteEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, source)
    }

    fn force_disk_document_content(&self, file: &Path, source: &str) -> Result<String> {
        agent_doc_document_realtime_io::resolve_disk_current_document_content(file, source)
    }

    fn converge_or_disk_write(
        &self,
        file: &Path,
        previous: &str,
        updated: &str,
        phase: &str,
    ) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            file,
            previous,
            updated,
            phase,
        )
    }

    fn record_document_write_provenance(&self, file: &Path, updated: &str) {
        agent_doc_document_realtime_io::record_document_write_provenance(file, updated);
    }

    fn attribute_agent_write_to_baseline(&self, file: &Path, before: &str, after: &str) {
        // Attribution follows a successfully landed standalone write. Failure is
        // fail-safe bookkeeping: leave the baseline untouched and preserve the
        // successful focused status update.
        if let Err(err) = agent_doc_snapshot_io::attribute_agent_write_to_document_baseline(
            file,
            before,
            after,
            agent_doc_ops_log_io::log_op,
        ) {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "document_baseline_agent_write_attribution file={} outcome=error error={}",
                    file.display(),
                    format!("{err:#}").replace('\n', " | ")
                ),
            );
        }
    }

    fn record_status_sync_synced(&self, file: &Path, landed: &str) {
        // Bookkeeping after a landed write: a failure only means the next
        // checkpoint cannot detect a revert, never that the status write failed.
        match agent_doc_snapshot_io::record_status_sync_synced_payload(file, landed) {
            Ok(Some(hash)) => agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "status_sync_synced_payload_recorded file={} payload_hash={hash} (#statussyncrevert)",
                    file.display()
                ),
            ),
            Ok(None) => {}
            Err(err) => agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "status_sync_synced_payload_recorded file={} outcome=error error={} (#statussyncrevert)",
                    file.display(),
                    format!("{err:#}").replace('\n', " | ")
                ),
            ),
        }
    }

    fn log_op(&self, file: &Path, message: &str) {
        agent_doc_ops_log_io::log_op(file, message);
    }
}

/// Reapply the authoritative status projection through the bounded lossless-tree
/// primitive. Extracting the projected component body keeps the shadow faithful
/// to both full replacement and `sync`-managed updates without duplicating status
/// policy in the IO adapter.
fn status_tree_candidate(full_content: &str, legacy_new_doc: &str) -> Option<String> {
    let status = agent_doc_element::element::parse(legacy_new_doc)
        .ok()?
        .into_iter()
        .find(|component| component.name == "status")?;
    agent_doc_markdown_lossless::replace_component_inner(
        full_content,
        "status",
        status.content(legacy_new_doc),
    )
}

/// Phase 2 strangler shadow (`#lzlosstree`): log whether the bounded lossless-tree
/// status replace is byte-identical to `legacy_new_doc`. Never changes behavior.
fn log_status_tree_parity<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    full_content: &str,
    legacy_new_doc: &str,
) {
    let candidate = status_tree_candidate(full_content, legacy_new_doc);
    let result = match &candidate {
        Some(tree_doc) if tree_doc == legacy_new_doc => "match",
        Some(_) => "mismatch",
        None => "declined",
    };
    effects.log_op(
        file,
        &format!(
            "lossless_write_parity op=status_set result={result} legacy_len={} tree_len={}",
            legacy_new_doc.len(),
            candidate.as_ref().map(|c| c.len()).unwrap_or(0),
        ),
    );
}

/// Update the status component with the provided text.
///
/// The legacy path replaces the whole body. When the status opening marker has
/// `sync="..."`, the pure document projection applies its additive (default) or
/// protected replace synchronization contract instead.
pub fn set<E: StatusWriteEffects + ?Sized>(effects: &E, file: &Path, text: &str) -> Result<()> {
    set_with_options(effects, file, text, false)
}

pub fn set_with_runtime_options(file: &Path, text: &str, force_disk: bool) -> Result<()> {
    set_with_options(&RUNTIME_STATUS_WRITE_EFFECTS, file, text, force_disk)
}

/// Update only `agent:status` as an independent, mid-turn-safe command.
///
/// Unlike response writeback, this does not capture, seal, or commit the active
/// cycle. The landed component delta is attributed to the agent in the document
/// baseline so a later preflight does not report it as operator input.
pub fn set_standalone_with_runtime_options(
    file: &Path,
    text: &str,
    force_disk: bool,
) -> Result<()> {
    set_with_options_and_attribution(&RUNTIME_STATUS_WRITE_EFFECTS, file, text, force_disk, true)
}

pub fn set_with_options<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    text: &str,
    force_disk: bool,
) -> Result<()> {
    set_with_options_and_attribution(effects, file, text, force_disk, false)
}

fn set_with_options_and_attribution<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    text: &str,
    force_disk: bool,
    attribute_baseline: bool,
) -> Result<()> {
    let full_content = if force_disk {
        effects.force_disk_document_content(file, "status_set")
    } else {
        effects.current_document_content(file, "status_set")
    }
    .context("failed to read document")?;
    let new_doc =
        agent_doc_document::status_projection::update_status_content(&full_content, text)?;
    // Phase 2 strangler shadow (#lzlosstree): compute the same projected status
    // body through a bounded lossless-tree mutation and log whether it is
    // byte-identical to the legacy projection. Measurement only — `new_doc` stays
    // the authority until parity is proven green on live writes.
    log_status_tree_parity(effects, file, &full_content, &new_doc);
    land_status_projection(
        effects,
        file,
        &full_content,
        &new_doc,
        force_disk,
        attribute_baseline,
        "status_set",
    )
}

/// Land a computed status projection through the editor-aware transport (or
/// the explicit `--force-disk` override) and, for standalone commands,
/// attribute only the landed status delta to the agent document baseline.
fn land_status_projection<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    full_content: &str,
    new_doc: &str,
    force_disk: bool,
    attribute_baseline: bool,
    op: &str,
) -> Result<()> {
    if force_disk {
        std::fs::write(file, new_doc)
            .with_context(|| format!("{op}: failed to write {}", file.display()))?;
        effects.record_document_write_provenance(file, new_doc);
        effects.log_op(
            file,
            &format!(
                "{op}_writeback file={} transport=disk_force reason=force_disk len={} hash={}",
                file.display(),
                new_doc.len(),
                agent_doc_hash::content_hash(new_doc)
            ),
        );
        if attribute_baseline {
            effects.attribute_agent_write_to_baseline(file, full_content, new_doc);
        }
        effects.record_status_sync_synced(file, new_doc);
        return Ok(());
    }
    effects.converge_or_disk_write(file, full_content, new_doc, op)?;
    if attribute_baseline {
        effects.attribute_agent_write_to_baseline(file, full_content, new_doc);
    }
    effects.record_status_sync_synced(file, new_doc);
    Ok(())
}

/// Result of `agent-doc status repair`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusRepairOutcome {
    /// The `status:sync` structure was already well-formed; nothing was written.
    AlreadyWellFormed,
    /// A nested/duplicated/unclosed/orphaned structure was folded to one region.
    Repaired,
}

/// Binary-owned repair for a malformed `status:sync` structure in
/// `agent:status` (the shape preflight refuses as "duplicate or nested
/// status:sync blocks"). Keeps the latest innermost sync payload, moves leaked
/// free-form prose back outside the region without duplicating it, and leaves
/// every byte outside `agent:status` untouched. Lands like `status set`:
/// editor-aware by default, standalone baseline attribution, no cycle capture.
pub fn repair_with_options<E: StatusWriteEffects + ?Sized>(
    effects: &E,
    file: &Path,
    force_disk: bool,
    attribute_baseline: bool,
) -> Result<StatusRepairOutcome> {
    let full_content = if force_disk {
        effects.force_disk_document_content(file, "status_repair")
    } else {
        effects.current_document_content(file, "status_repair")
    }
    .context("failed to read document")?;
    let Some(new_doc) =
        agent_doc_document::status_projection::normalize_status_sync_content(&full_content)?
    else {
        effects.log_op(
            file,
            &format!(
                "status_repair file={} outcome=already_well_formed",
                file.display()
            ),
        );
        return Ok(StatusRepairOutcome::AlreadyWellFormed);
    };
    effects.log_op(
        file,
        &format!(
            "status_repair file={} outcome=normalized before_len={} after_len={}",
            file.display(),
            full_content.len(),
            new_doc.len()
        ),
    );
    land_status_projection(
        effects,
        file,
        &full_content,
        &new_doc,
        force_disk,
        attribute_baseline,
        "status_repair",
    )?;
    Ok(StatusRepairOutcome::Repaired)
}

/// `agent-doc status repair <FILE>` with the runtime effects.
pub fn repair_standalone_with_runtime_options(
    file: &Path,
    force_disk: bool,
) -> Result<StatusRepairOutcome> {
    repair_with_options(&RUNTIME_STATUS_WRITE_EFFECTS, file, force_disk, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[test]
    fn tree_status_replace_is_byte_identical_to_legacy_projection() {
        // The strangler shadow is only meaningful if the tree mutation reproduces
        // the legacy `replace_status_content` result for the standard status shape.
        // This proves that the projected component body is a sufficient bounded
        // tree edit for the common newline-framed component.
        let doc = concat!(
            "---\ntitle: t\n---\n\n",
            "<!-- agent:exchange -->\nhi\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:status patch=replace -->\nold status\n<!-- /agent:status -->\n",
        );
        for text in ["new status", "line one\nline two", "café ☕ 世界", ""] {
            let legacy =
                agent_doc_document::status_projection::replace_status_content(doc, text).unwrap();
            let tree = status_tree_candidate(doc, &legacy).expect("status component exists");
            assert_eq!(
                tree, legacy,
                "tree/legacy status replace diverged for {text:?}"
            );
        }
    }

    struct TestEffects {
        current_content: RefCell<Option<String>>,
        disk_content: RefCell<Option<String>>,
        previous: RefCell<Option<String>>,
        updated: RefCell<Option<String>>,
        current_reads: Cell<u32>,
        disk_reads: Cell<u32>,
        converges: Cell<u32>,
        provenance: Cell<u32>,
        attributions: Cell<u32>,
        attributed_before: RefCell<Option<String>>,
        attributed_after: RefCell<Option<String>>,
        logs: RefCell<Vec<String>>,
        synced_recorded: RefCell<Option<String>>,
    }

    impl TestEffects {
        fn new() -> Self {
            Self {
                current_content: RefCell::new(None),
                disk_content: RefCell::new(None),
                previous: RefCell::new(None),
                updated: RefCell::new(None),
                current_reads: Cell::new(0),
                disk_reads: Cell::new(0),
                converges: Cell::new(0),
                provenance: Cell::new(0),
                attributions: Cell::new(0),
                attributed_before: RefCell::new(None),
                attributed_after: RefCell::new(None),
                logs: RefCell::new(Vec::new()),
                synced_recorded: RefCell::new(None),
            }
        }

        fn with_current_content(self, content: impl Into<String>) -> Self {
            *self.current_content.borrow_mut() = Some(content.into());
            self
        }

        fn with_disk_content(self, content: impl Into<String>) -> Self {
            *self.disk_content.borrow_mut() = Some(content.into());
            self
        }
    }

    impl StatusWriteEffects for TestEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            self.current_reads.set(self.current_reads.get() + 1);
            if let Some(content) = self.current_content.borrow().clone() {
                return Ok(content);
            }
            Ok(std::fs::read_to_string(file)?)
        }

        fn force_disk_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            self.disk_reads.set(self.disk_reads.get() + 1);
            if let Some(content) = self.disk_content.borrow().clone() {
                return Ok(content);
            }
            Ok(std::fs::read_to_string(file)?)
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            previous: &str,
            updated: &str,
            _phase: &str,
        ) -> Result<()> {
            self.converges.set(self.converges.get() + 1);
            *self.previous.borrow_mut() = Some(previous.to_string());
            *self.updated.borrow_mut() = Some(updated.to_string());
            std::fs::write(file, updated)?;
            Ok(())
        }

        fn record_document_write_provenance(&self, _file: &Path, _updated: &str) {
            self.provenance.set(self.provenance.get() + 1);
        }

        fn attribute_agent_write_to_baseline(&self, _file: &Path, before: &str, after: &str) {
            self.attributions.set(self.attributions.get() + 1);
            *self.attributed_before.borrow_mut() = Some(before.to_string());
            *self.attributed_after.borrow_mut() = Some(after.to_string());
        }

        fn record_status_sync_synced(&self, _file: &Path, landed: &str) {
            *self.synced_recorded.borrow_mut() = Some(landed.to_string());
        }

        fn log_op(&self, _file: &Path, message: &str) {
            self.logs.borrow_mut().push(message.to_string());
        }
    }

    fn write_status_doc(dir: &tempfile::TempDir) -> std::path::PathBuf {
        let doc = dir.path().join("plan.md");
        std::fs::write(
            &doc,
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace -->\n",
                "old status\n",
                "<!-- /agent:status -->\n",
            ),
        )
        .unwrap();
        doc
    }

    #[test]
    fn set_uses_converge_write_by_default() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = write_status_doc(&dir);
        let effects = TestEffects::new();

        set(&effects, &doc, "new status").unwrap();

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(on_disk.contains("new status"));
        assert!(!on_disk.contains("old status"));
        assert_eq!(effects.converges.get(), 1);
        assert_eq!(effects.provenance.get(), 0);
        assert_eq!(effects.attributions.get(), 0);
        // The only log on the converge path is the lossless-tree parity shadow,
        // which matches the legacy projection for the standard status shape.
        let logs = effects.logs.borrow();
        assert_eq!(logs.len(), 1);
        assert!(
            logs[0].contains("lossless_write_parity op=status_set result=match"),
            "{:?}",
            logs[0]
        );
        drop(logs);
        assert_eq!(effects.current_reads.get(), 1);
        assert_eq!(effects.disk_reads.get(), 0);
    }

    #[test]
    fn standalone_set_attributes_only_the_landed_status_delta() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = write_status_doc(&dir);
        let before = std::fs::read_to_string(&doc).unwrap();
        let effects = TestEffects::new();

        set_with_options_and_attribution(&effects, &doc, "current status", false, true).unwrap();

        let after = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(effects.attributions.get(), 1);
        assert_eq!(
            effects.attributed_before.borrow().as_deref(),
            Some(before.as_str())
        );
        assert_eq!(
            effects.attributed_after.borrow().as_deref(),
            Some(after.as_str())
        );
        assert!(after.contains("current status"));
        assert_eq!(effects.converges.get(), 1);
    }

    #[test]
    fn set_projects_from_current_document_not_stale_disk() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = write_status_doc(&dir);
        let current = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace -->\n",
            "editor status\n",
            "<!-- /agent:status -->\n",
            "\noperator note\n",
        );
        let effects = TestEffects::new().with_current_content(current);

        set(&effects, &doc, "new status").unwrap();

        let previous = effects.previous.borrow().clone().unwrap();
        assert!(previous.contains("editor status"));
        assert!(previous.contains("operator note"));
        let updated = effects.updated.borrow().clone().unwrap();
        assert!(updated.contains("new status"));
        assert!(updated.contains("operator note"));
        assert!(!updated.contains("old status"));
        assert_eq!(effects.current_reads.get(), 1);
        assert_eq!(effects.disk_reads.get(), 0);
    }

    #[test]
    fn force_disk_writes_records_provenance_and_logs_hash() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = write_status_doc(&dir);
        let effects = TestEffects::new().with_disk_content(
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace -->\n",
                "disk status\n",
                "<!-- /agent:status -->\n",
            )
            .to_string(),
        );

        set_with_options(&effects, &doc, "new status", true).unwrap();

        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert!(on_disk.contains("new status"));
        assert!(!on_disk.contains("disk status"));
        assert_eq!(effects.current_reads.get(), 0);
        assert_eq!(effects.disk_reads.get(), 1);
        assert_eq!(effects.converges.get(), 0);
        assert_eq!(effects.provenance.get(), 1);
        // Two logs now: the lossless-tree parity shadow (logged first, before the
        // force-disk branch) and the force-disk writeback audit marker.
        let logs = effects.logs.borrow();
        assert_eq!(logs.len(), 2);
        assert!(
            logs.iter()
                .any(|l| l.contains("lossless_write_parity op=status_set result=match")),
            "{logs:?}"
        );
        let writeback = logs
            .iter()
            .find(|l| l.contains("status_set_writeback"))
            .expect("writeback log present");
        assert!(writeback.contains("transport=disk_force"));
        assert!(writeback.contains("reason=force_disk"));
    }

    #[test]
    fn set_honors_additive_status_sync_and_preserves_marker_attributes() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("sync.md");
        let current = concat!(
            "## Status\n\n",
            "<!-- agent:status patch=replace sync=\"Keep release current\" custom=kept -->\n",
            "operator note\n",
            "<!-- /agent:status -->\n",
        );
        std::fs::write(&doc, current).unwrap();
        let effects = TestEffects::new();

        set(&effects, &doc, "Published: v1.2.3").unwrap();

        let updated = effects.updated.borrow().clone().unwrap();
        assert!(updated.contains("operator note"));
        assert!(updated.contains("<!-- status:sync -->\nPublished: v1.2.3\n<!-- /status:sync -->"));
        assert!(updated.contains(
            "<!-- agent:status patch=replace sync=\"Keep release current\" custom=kept -->"
        ));

        assert_eq!(
            effects.synced_recorded.borrow().as_deref(),
            Some(updated.as_str()),
            "a landed sync write records its payload as the latest synced value"
        );

        set(&effects, &doc, "Published: v1.2.3").unwrap();

        let reapplied = effects.updated.borrow().clone().unwrap();
        assert_eq!(reapplied, updated);
        assert_eq!(reapplied.matches("<!-- status:sync -->").count(), 1);
        let logs = effects.logs.borrow();
        assert_eq!(
            logs.iter()
                .filter(|line| {
                    line.contains("lossless_write_parity op=status_set result=match")
                })
                .count(),
            2,
            "{logs:?}"
        );
        assert!(
            !logs
                .iter()
                .any(|line| line.contains("lossless_write_parity op=status_set result=mismatch")),
            "{logs:?}"
        );
    }

    /// `#statussyncrevert` regression, replaying 2026-10-28 12:27Z on
    /// tasks/agent-doc/agent-doc-bugs.md: `status set` synced v0.35.559 at
    /// 07:41Z; five hours later the operator's Ctrl+Z walked IntelliJ's undo
    /// stack past their own typo into that agent edit and put the v0.35.558
    /// payload back, reported as operator text. The revert must be detected
    /// against the recorded synced value, and the re-sync must rewrite only the
    /// managed block, keeping the operator's other status prose and typing.
    #[test]
    fn an_editor_undo_of_a_synced_payload_is_detected_and_resynced_without_touching_operator_text()
    {
        use agent_doc_document::status_projection::{
            render_status_sync_checkpoint_evidence_with_synced, status_sync_revert,
        };
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("bugs.md");
        let before = concat!(
            "## Status <!-- agent:status patch=replace sync=\"##sync-release\" -->\n\n",
            "📤 Queue handoff: 1 reported item awaiting coordinator integration.\n\n",
            "<!-- status:sync -->\n",
            "Latest release: agent-doc v0.35.558 (release commit `00caa013ac`). v0.35.559 is being prepared.\n",
            "<!-- /status:sync -->\n",
            "<!-- /agent:status -->\n\n",
            "## Loop <!-- agent:loop interval=\"1h\" -->\n- watch issues\n<!-- /agent:loop -->\n",
        );
        std::fs::write(&doc, before).unwrap();
        let effects = TestEffects::new();
        let synced_text =
            "Released and published agent-doc v0.35.559 from release commit `5870713c02`.";

        // 07:41:30Z: the agent's status set lands and is recorded.
        set(&effects, &doc, synced_text).unwrap();
        let synced_doc = effects.updated.borrow().clone().unwrap();
        let recorded = effects.synced_recorded.borrow().clone().unwrap();
        let recorded_payload =
            agent_doc_document::status_projection::status_sync_managed_payload(&recorded).unwrap();
        assert_eq!(recorded_payload.trim(), synced_text);
        assert_eq!(
            status_sync_revert(&synced_doc, Some(&recorded_payload)),
            None
        );

        // 12:27:36Z: the IDE undo restores the 07:41:15Z text exactly, and the
        // operator keeps typing elsewhere (here: a note in the loop body).
        let reverted = before.replace("- watch issues", "- watch issues\n- operator note");
        std::fs::write(&doc, &reverted).unwrap();
        let revert = status_sync_revert(&reverted, Some(&recorded_payload))
            .expect("the undone payload differs from the latest synced value");
        assert_eq!(revert.synced_payload, synced_text);
        let checkpoint =
            render_status_sync_checkpoint_evidence_with_synced(&reverted, Some(&recorded_payload))
                .unwrap();
        assert!(checkpoint.contains("does not stick"), "{checkpoint}");
        assert!(
            checkpoint.contains(&format!("> {synced_text}")),
            "{checkpoint}"
        );
        assert!(
            checkpoint.contains("> Latest release: agent-doc v0.35.558"),
            "{checkpoint}"
        );

        // The checkpoint's re-sync writes the freshly derived value.
        let fresh = "Released and published agent-doc v0.35.559; main is at `67413e99fd`.";
        set(&effects, &doc, fresh).unwrap();
        let resynced = effects.updated.borrow().clone().unwrap();
        assert!(resynced.contains(fresh), "{resynced}");
        assert!(!resynced.contains("v0.35.558"), "{resynced}");
        assert!(
            resynced.contains("- operator note"),
            "operator text outside the block survives"
        );
        assert!(
            resynced.contains("📤 Queue handoff: 1 reported item"),
            "{resynced}"
        );
        let recorded_after = effects.synced_recorded.borrow().clone().unwrap();
        let recorded_after =
            agent_doc_document::status_projection::status_sync_managed_payload(&recorded_after)
                .unwrap();
        assert_eq!(recorded_after.trim(), fresh);
        assert_eq!(status_sync_revert(&resynced, Some(&recorded_after)), None);
    }

    const NESTED_SYNC_DOC: &str = concat!(
        "## Status\n\n",
        "<!-- agent:status patch=replace sync=\"Keep release current\" -->\n",
        "\n📤 Queue handoff: 5 reported items awaiting coordinator integration.\n",
        "Review pass done.\n\n",
        "<!-- status:sync -->\n",
        "Review pass done.\n\n",
        "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
        "<!-- /status:sync -->\n",
        "<!-- /agent:status -->\n",
        "\n<!-- agent:exchange -->\nkeep\n<!-- /agent:exchange -->\n",
    );

    #[test]
    fn repair_folds_nested_sync_through_converge_and_attributes_baseline() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("nested.md");
        std::fs::write(&doc, NESTED_SYNC_DOC).unwrap();
        let effects = TestEffects::new();

        let outcome = repair_with_options(&effects, &doc, false, true).unwrap();
        assert_eq!(outcome, StatusRepairOutcome::Repaired);
        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            on_disk,
            concat!(
                "## Status\n\n",
                "<!-- agent:status patch=replace sync=\"Keep release current\" -->\n",
                "\n📤 Queue handoff: 5 reported items awaiting coordinator integration.\n",
                "Review pass done.\n\n",
                "<!-- status:sync -->\nPublished: v2\n<!-- /status:sync -->\n",
                "<!-- /agent:status -->\n",
                "\n<!-- agent:exchange -->\nkeep\n<!-- /agent:exchange -->\n",
            )
        );
        agent_doc_document::status_projection::status_sync_directive(&on_disk)
            .expect("repaired document satisfies the preflight contract");
        assert_eq!(effects.converges.get(), 1);
        assert_eq!(effects.provenance.get(), 0);
        assert_eq!(effects.attributions.get(), 1);
        assert_eq!(
            effects.attributed_before.borrow().as_deref(),
            Some(NESTED_SYNC_DOC)
        );

        let again = TestEffects::new();
        let outcome = repair_with_options(&again, &doc, false, true).unwrap();
        assert_eq!(outcome, StatusRepairOutcome::AlreadyWellFormed);
        assert_eq!(
            again.converges.get(),
            0,
            "a well-formed document is never rewritten"
        );
        assert_eq!(again.attributions.get(), 0);
    }

    #[test]
    fn set_self_heals_nested_sync_instead_of_refusing() {
        let dir = tempfile::TempDir::new().unwrap();
        let doc = dir.path().join("nested.md");
        std::fs::write(&doc, NESTED_SYNC_DOC).unwrap();
        let effects = TestEffects::new();

        set(&effects, &doc, "Published: v3").unwrap();
        let on_disk = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            on_disk.matches("<!-- status:sync -->").count(),
            1,
            "{on_disk}"
        );
        assert_eq!(on_disk.matches("Review pass done.").count(), 1, "{on_disk}");
        assert!(on_disk.contains("<!-- status:sync -->\nPublished: v3\n<!-- /status:sync -->"));
    }
}
