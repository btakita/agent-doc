//! # Module: queue_cmd
//!
//! CLI subcommands for managing the `agent:queue` component.
//!
//! - `agent-doc queue sync <FILE>` — one-shot sync from backlog items with
//!   `queue` attribute or per-item enqueue markers into `agent:queue`.
//! - `agent-doc queue consume <FILE> [--count N]` — explicitly strike the
//!   leading N free-text queue head(s) the agent has already answered.
//! - `agent-doc queue consume <FILE> --id <id>` — escape hatch (#orphanqhead)
//!   that strikes an orphaned id-backed head whose backing backlog item was
//!   already reaped (or is gone), so it stops re-firing the auto-loop.
//! - `agent-doc queue consume <FILE> --ack-id <id>` — explicit acknowledgement
//!   (#freshqueueauth) for an id-backed correction head while leaving the open
//!   backlog item unresolved.
//! - `agent-doc queue consume <FILE> --ack-text <text>` — exact acknowledgement
//!   for a free-text head completed through direct harness steering while the
//!   queue remained paused.

use anyhow::{Result, bail};
use std::path::Path;

use crate::one_shot_sync::OneShotQueueSyncResult;
use agent_doc_element_backlog::backlog;
use agent_doc_queue::queue_heads::active_queue_head_text;
use agent_doc_queue::queue_heads::{ActiveQueueHeadKind, classify_active_queue_head};

#[derive(Clone, Copy, Debug, Default)]
pub struct ConsumeOptions {
    pub force_disk: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueCommandConsumeOutcome {
    pub consumed_text: String,
    pub consumed_count: usize,
    pub remaining: usize,
    pub drained: bool,
}

pub trait QueueCommandEffects {
    fn current_document_content(&self, file: &Path, source: &str) -> Result<String>;

    fn converge_document_or_disk(
        &self,
        file: &Path,
        target_content: &str,
        source_content: &str,
        reason: &str,
    ) -> Result<()>;

    fn consume_free_text_queue_prompts_force_disk(
        &self,
        file: &Path,
        count: usize,
    ) -> Result<Option<QueueCommandConsumeOutcome>>;
    fn consume_free_text_queue_prompts_with_outcome(
        &self,
        file: &Path,
        count: usize,
    ) -> Result<Option<QueueCommandConsumeOutcome>>;
    fn strike_orphan_id_backed_queue_head(&self, file: &Path, id: &str) -> Result<bool>;
    fn acknowledge_open_id_backed_queue_head(&self, file: &Path, id: &str) -> Result<bool>;
    fn acknowledge_paused_free_text_queue_head(
        &self,
        file: &Path,
        expected_head: &str,
    ) -> Result<Option<QueueCommandConsumeOutcome>>;
    fn prune_noise_queue_heads(&self, file: &Path) -> Result<usize>;
    fn converge_captured_answered_free_text_projection(&self, _file: &Path) -> Result<usize> {
        Ok(0)
    }
}

/// Explicitly strike the leading `count` free-text queue head(s) — the agent
/// asserting it has already answered them, the same contract `--done <id>` gives
/// an id-backed head (`#multi-head-consume-one-per-finalize`).
///
/// The free-text strike heuristic only consumes ONE head per finalize (the head
/// current at that cycle's preflight), so when several free-text heads are
/// answered across a single cycle the trailing ones stay queued and re-serve on
/// the next auto-loop, producing duplicate-response churn. This gives a
/// deterministic, non-heuristic way to drain those answered stragglers without a
/// fuzzy head↔response matcher that could delete a genuinely unanswered prompt.
/// Session-check still requires committed exchange proof for the struck
/// free-text head, normally through the queue-prompt echo.
///
/// Scoped to free-text heads: if a head in range is id-backed it bails with
/// guidance to use `--done`, so it can never silently desync a head from its
/// still-open backlog item. Writes document + snapshot like `sync`; the caller
/// closes out through the normal commit path.
pub fn consume_with_options(
    effects: &impl QueueCommandEffects,
    file: &Path,
    count: usize,
    options: ConsumeOptions,
) -> Result<()> {
    // #sqedit-race Phase 2: hold the queue-edit lease for the whole strike loop so
    // preflight queue maintenance and the supervisor idle-watch defer instead of
    // round-tripping a torn intermediate queue. Released on drop (incl. early
    // return / `bail!`).
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let target = count.max(1);
    let content = effects.current_document_content(file, "queue_consume_classify")?;
    match classify_active_queue_head(&content)? {
        ActiveQueueHeadKind::None => {}
        ActiveQueueHeadKind::Inactive => {
            let struck = effects.converge_captured_answered_free_text_projection(file)?;
            if struck > 0 {
                println!(
                    "{}: recovered torn inactive queue projection and consumed {} captured-response-proven head(s)",
                    file.display(),
                    struck
                );
                return Ok(());
            }
            bail!(
                "{}: queue rows exist but queue_active is false, and no durable captured response proves an answered head. Preserve the rows and repair the activation projection before consuming work.",
                file.display()
            );
        }
        ActiveQueueHeadKind::IdBacked => {
            let queue_active = agent_doc_frontmatter::frontmatter::parse(&content)?
                .0
                .queue_active;
            let classified_head = active_queue_head_text(&content)?;
            bail!(
                "{}: queue head is an id-backed directive, not a free-text prompt. \
                 classification evidence: queue_active={queue_active:?} active_head={classified_head:?}. \
                 If it represents completed or gated work, reap it through the normal closeout with \
                 `--done <id>` / `--pending-gate <id>` so the backlog item stays in sync. \
                 If it is only an acknowledgement/correction for still-open work, use \
                 `agent-doc queue consume <FILE> --ack-id <id>` to strike the head without closing \
                 the backlog item. Otherwise leave it queued.",
                file.display()
            );
        }
        ActiveQueueHeadKind::FreeText => {}
    }
    let outcome = if matches!(
        classify_active_queue_head(&content)?,
        ActiveQueueHeadKind::FreeText
    ) {
        if options.force_disk {
            effects.consume_free_text_queue_prompts_force_disk(file, target)?
        } else {
            effects.consume_free_text_queue_prompts_with_outcome(file, target)?
        }
    } else {
        None
    };

    match outcome {
        None => println!(
            "{}: no free-text queue head to consume (queue inactive, empty, or id-backed head).",
            file.display()
        ),
        Some(outcome) => {
            println!(
                "{}: consumed {} free-text queue head(s) (remaining: {}){}",
                file.display(),
                outcome.consumed_count,
                outcome.remaining,
                if outcome.drained {
                    ", drained — cleared queue_active"
                } else {
                    ""
                }
            );
        }
    }
    Ok(())
}

/// Escape hatch (#orphanqhead): strike an orphaned id-backed queue head by id.
/// Delegates to the write-layer striker, which guards against desyncing live
/// open backlog work and keeps the document and snapshot in sync.
pub fn consume_orphan_id(effects: &impl QueueCommandEffects, file: &Path, id: &str) -> Result<()> {
    let normalized = backlog::normalize_pending_id(id);
    if effects.strike_orphan_id_backed_queue_head(file, id)? {
        println!(
            "{}: struck orphaned id-backed queue head [#{}] (#orphanqhead).",
            file.display(),
            normalized
        );
    } else {
        println!(
            "{}: no change — orphaned id-backed head [#{}] was already struck or drained.",
            file.display(),
            normalized
        );
    }
    Ok(())
}

pub fn acknowledge_open_id(
    effects: &impl QueueCommandEffects,
    file: &Path,
    id: &str,
) -> Result<()> {
    let normalized = backlog::normalize_pending_id(id);
    if effects.acknowledge_open_id_backed_queue_head(file, id)? {
        println!(
            "{}: acknowledged id-backed correction head [#{}] while preserving the open backlog item (#freshqueueauth).",
            file.display(),
            normalized
        );
    } else {
        println!(
            "{}: no change — id-backed correction head [#{}] was already acknowledged or drained.",
            file.display(),
            normalized
        );
    }
    Ok(())
}

pub fn acknowledge_paused_free_text(
    effects: &impl QueueCommandEffects,
    file: &Path,
    expected_head: &str,
) -> Result<()> {
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    match effects.acknowledge_paused_free_text_queue_head(file, expected_head)? {
        Some(outcome) => println!(
            "{}: acknowledged exact paused free-text queue head (remaining: {}); \
             queue stays paused (#paused-free-text-ack).",
            file.display(),
            outcome.remaining
        ),
        None => println!(
            "{}: no paused free-text queue head to acknowledge.",
            file.display()
        ),
    }
    Ok(())
}

pub fn consume(effects: &impl QueueCommandEffects, file: &Path, count: usize) -> Result<()> {
    consume_with_options(effects, file, count, ConsumeOptions::default())
}

/// `agent-doc queue prune-noise <FILE>` — strike every non-drainable noise queue
/// head at any position (`#goqstall2`), clearing pasted console output / agent
/// response fragments that session-check surfaces as
/// `queue_stale_noise_lines=N`. Also strikes **orphan id-backed heads**
/// (`#orphanqhead`): a `do [#id]` / `[#id]` head whose id names no open
/// `agent:backlog` item, which is non-drainable yet blocks the leading-run
/// `queue consume` from reaching answered free-text heads behind it. Preserves
/// id-backed directives whose id is still open backlog work (including deferred
/// `[operator-verify]` / `[focused-cycle]` items) and drainable free-text/prose
/// heads. The prune set is predicate-proven; fresh operator prompts that remain
/// drainable are never removed by this command.
/// Supervisor-safe: routes through the same editor-IPC-converged write path the
/// closeout strikes use.
pub fn prune_noise(effects: &impl QueueCommandEffects, file: &Path) -> Result<()> {
    // #sqedit-race Phase 2: hold the queue-edit lease across the prune so the
    // supervisor idle-watch + preflight maintenance defer (single queue writer).
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let struck = effects.prune_noise_queue_heads(file)?;
    if struck == 0 {
        println!(
            "{}: no predicate-proven queue heads to prune (queue inactive, empty, or all heads are drainable/live).",
            file.display()
        );
    } else {
        println!(
            "{}: pruned {} predicate-proven queue head(s) (noise/orphan only).",
            file.display(),
            struck
        );
    }
    Ok(())
}

pub fn sync_with_effects(effects: &impl QueueCommandEffects, file: &Path) -> Result<()> {
    let content = effects.current_document_content(file, "queue_sync")?;
    match crate::one_shot_sync::sync_one_shot_backlog_queue_with_snapshot(
        file,
        &content,
        |path, current, target| {
            effects.converge_document_or_disk(path, target, current, "queue_sync")
        },
        |path, content| {
            agent_doc_snapshot_io::checkpoint_document_baseline(
                path,
                content,
                agent_doc_ops_log_io::log_op,
            )
        },
    )? {
        OneShotQueueSyncResult::AlreadyInSync {
            requested_count,
            mode,
        } => {
            println!(
                "{}: queue already in sync ({} active backlog id(s), {:?} mode). No changes.",
                file.display(),
                requested_count,
                mode
            );
        }
        OneShotQueueSyncResult::Synced(applied) => {
            println!(
                "{}: synced {} backlog id(s) → {} queue prompt(s) ({:?} mode)",
                file.display(),
                applied.requested_count,
                applied.prompt_count,
                applied.mode
            );
            if !applied.already_present.is_empty() {
                println!(
                    "{}: skipped already represented backlog id(s): {} (reason: already_in_queue)",
                    file.display(),
                    crate::one_shot_sync::format_queue_ids(&applied.already_present)
                );
            }
            if !applied.newly_materialized.is_empty() {
                println!(
                    "{}: materialized backlog id(s): {}",
                    file.display(),
                    crate::one_shot_sync::format_queue_ids(&applied.newly_materialized)
                );
            }
            if let Some(warning) = applied.snapshot_warning {
                eprintln!("[queue sync] warning: failed to update snapshot: {warning}");
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeEffects;

    impl FakeEffects {
        fn strike_free_text_heads(
            file: &Path,
            count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            let mut updated = std::fs::read_to_string(file)?;
            let mut consumed = Vec::new();
            for _ in 0..count.max(1) {
                let Some(head) = active_queue_head_text(&updated)? else {
                    break;
                };
                if !matches!(
                    classify_active_queue_head(&updated)?,
                    ActiveQueueHeadKind::FreeText
                ) {
                    break;
                }
                let needle = format!("- {head}");
                let replacement = format!("- ~{head}~");
                updated = updated.replacen(&needle, &replacement, 1);
                consumed.push(head);
            }
            let Some(first) = consumed.first().cloned() else {
                return Ok(None);
            };
            // One write proves the batch is planned from one authoritative cut.
            std::fs::write(file, &updated)?;
            Ok(Some(QueueCommandConsumeOutcome {
                consumed_text: first,
                consumed_count: consumed.len(),
                remaining: usize::from(active_queue_head_text(&updated)?.is_some()),
                drained: false,
            }))
        }
    }

    impl QueueCommandEffects for FakeEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            std::fs::write(file, target_content)?;
            Ok(())
        }

        fn consume_free_text_queue_prompts_force_disk(
            &self,
            file: &Path,
            count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            Self::strike_free_text_heads(file, count)
        }

        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            file: &Path,
            count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            Self::strike_free_text_heads(file, count)
        }

        fn strike_orphan_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            Ok(true)
        }

        fn acknowledge_open_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            Ok(true)
        }

        fn acknowledge_paused_free_text_queue_head(
            &self,
            file: &Path,
            expected_head: &str,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            let mut content = std::fs::read_to_string(file)?;
            let needle = format!("- {}", expected_head.trim());
            if !content.lines().any(|line| line == needle) {
                bail!("exact queue head mismatch");
            }
            let replacement = format!("- ~~{}~~", expected_head.trim());
            content = content.replacen(&needle, &replacement, 1);
            std::fs::write(file, content)?;
            Ok(Some(QueueCommandConsumeOutcome {
                consumed_text: expected_head.trim().to_string(),
                consumed_count: 1,
                remaining: 0,
                drained: true,
            }))
        }

        fn prune_noise_queue_heads(&self, _file: &Path) -> Result<usize> {
            Ok(0)
        }
    }

    #[test]
    fn sync_accepts_enqueue_marker_without_queue_attr() {
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: false\n---\n\n",
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#alpha] :inbox_tray: add me\n",
            "- [ ] [#beta] leave me alone\n",
            "- [/] [#gated] /enqueue blocked\n",
            "<!-- /agent:backlog -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects;
        sync_with_effects(&effects, &doc).expect("enqueue marker should append to queue");
        let result = std::fs::read_to_string(&doc).unwrap();

        assert!(
            result.contains("- do [#alpha]"),
            "marked item should be queued:\n{result}"
        );
        assert!(
            !result.contains("- do [#beta]"),
            "unmarked item must not be queued:\n{result}"
        );
        assert!(
            !result.contains("- do [#gated]"),
            "gated marker must not be queued:\n{result}"
        );
    }

    #[test]
    fn consume_strikes_multiple_answered_free_text_heads() {
        // #multi-head-consume-one-per-finalize: a single turn answered two
        // free-text heads; `queue consume --count 2` drains both stragglers
        // deterministically while leaving the trailing id-backed head intact.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n",
            "### Re: head one\n\nDone.\n",
            "### Re: head two\n\nDone.\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- head one free text\n",
            "- head two free text\n",
            "- do [#keepme]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects;
        consume_with_options(&effects, &doc, 2, ConsumeOptions { force_disk: true })
            .expect("consume two answered free-text heads");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("~head one free text~"),
            "head one must be struck:\n{result}"
        );
        assert!(
            result.contains("~head two free text~"),
            "head two must be struck:\n{result}"
        );
        assert!(
            result.contains("- do [#keepme]"),
            "trailing id-backed head must be preserved:\n{result}"
        );
    }

    #[test]
    fn consume_stops_at_id_backed_head_after_striking_free_text() {
        // count overruns the free-text run: strike the one free-text head, then
        // stop cleanly at the id-backed head instead of desyncing it.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:exchange -->\n### Re: only free\n\nDone.\n<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue go -->\n",
            "- only free head\n",
            "- do [#keepme]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects;
        consume_with_options(&effects, &doc, 5, ConsumeOptions { force_disk: true })
            .expect("consume should stop at the id-backed head");
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("~only free head~"),
            "free head struck:\n{result}"
        );
        assert!(
            result.contains("- do [#keepme]"),
            "id-backed head preserved:\n{result}"
        );
    }

    #[test]
    fn consume_treats_bare_bracket_id_head_as_id_backed() {
        // Regression: a bare `[#id]` head (no `do` prefix) must be classified
        // id-backed via topic_resolves_to_exact_id, not struck as free text.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- [#admin-recover]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects;
        let err = consume(&effects, &doc, 1).unwrap_err();
        assert!(
            err.to_string().contains("id-backed"),
            "a bare [#id] head must be refused, not struck: {err}"
        );
        let result = std::fs::read_to_string(&doc).unwrap();
        assert!(
            result.contains("- [#admin-recover]") && !result.contains("~[#admin-recover]~"),
            "the id-backed head must be left intact:\n{result}"
        );
    }

    #[test]
    fn consume_bails_on_leading_id_backed_head() {
        // An id-backed head must be reaped via --done, never struck blind here.
        let dir = tempfile::tempdir().unwrap();
        let doc = dir.path().join("s.md");
        let content = concat!(
            "---\nqueue_active: true\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#someid]\n",
            "<!-- /agent:queue -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &doc,
            content,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();

        let effects = FakeEffects;
        let err = consume(&effects, &doc, 1).unwrap_err();
        assert!(
            err.to_string().contains("id-backed"),
            "should refuse a leading id-backed head: {err}"
        );
    }
}
