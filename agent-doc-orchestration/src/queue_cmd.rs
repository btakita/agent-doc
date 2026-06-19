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

use anyhow::{Context, Result, bail};
use std::path::Path;

use crate::component;
use crate::pending;
use crate::queue;
use crate::snapshot;

/// Classification of the active `agent:queue` head for `consume`.
enum HeadKind {
    /// No queue component, or no live prompt to strike.
    None,
    /// A free-text head (a plain question/instruction) — strikable here. Also
    /// covers a bare registered `prompt_presets` token head (`#advance-review`)
    /// that is not a tracked backlog/review id: it has no `--done` reap path, so
    /// it strikes here like free text (#qpresetstrike).
    FreeText,
    /// An id-backed head (`#id`, `[#id]`, `do [#id]`, or a queue trigger) that
    /// names a tracked backlog/review/icebox directive and must be reaped via its
    /// id, never struck blind.
    IdBacked,
}

fn queue_prompt_reference_id(text: &str) -> Option<String> {
    let marker = text.find('#')?;
    let id: String = text[marker + 1..]
        .chars()
        .take_while(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_'))
        .collect();
    if id.is_empty() {
        None
    } else {
        Some(id.to_ascii_lowercase())
    }
}

fn queue_entry_reference_id(entry: &queue::QueueEntry) -> Option<String> {
    match entry {
        queue::QueueEntry::Prompt(prompt) | queue::QueueEntry::Completed(prompt) => {
            queue_prompt_reference_id(&prompt.text)
        }
        _ => None,
    }
}

fn format_queue_ids(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Classify the active queue head using the canonical free-text detector
/// (`write::queue_head_is_free_text_prompt`), which resolves bare `[#id]`,
/// `#id`, and `#preset` heads as id-backed via `topic_resolves_to_exact_id` —
/// the simpler `do [#` prefix check would miss a bare `[#id]` head and strike it,
/// desyncing it from its backlog item.
fn classify_active_head(content: &str) -> Result<HeadKind> {
    let components = component::parse(content)?;
    let Some(qc) = components.iter().find(|c| c.name == "queue") else {
        return Ok(HeadKind::None);
    };
    let entries = queue::parse(&content[qc.open_end..qc.close_start])?;
    if queue::prompts(&entries).is_empty() {
        return Ok(HeadKind::None);
    }
    if crate::write::queue_head_is_free_text_prompt(content)? {
        Ok(HeadKind::FreeText)
    } else {
        Ok(HeadKind::IdBacked)
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
/// Escape hatch (#orphanqhead): strike an orphaned id-backed queue head by id.
/// Delegates to the write-layer striker, which guards against desyncing live
/// open backlog work and keeps the document and snapshot in sync.
pub fn consume_orphan_id(file: &Path, id: &str) -> Result<()> {
    let normalized = pending::normalize_pending_id(id);
    if crate::write::strike_orphan_id_backed_queue_head(file, id)? {
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

pub fn consume(file: &Path, count: usize) -> Result<()> {
    // #sqedit-race Phase 2: hold the queue-edit lease for the whole strike loop so
    // preflight queue maintenance and the supervisor idle-watch defer instead of
    // round-tripping a torn intermediate queue. Released on drop (incl. early
    // return / `bail!`).
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let target = count.max(1);
    let mut struck: Vec<String> = Vec::new();
    let mut last_remaining = 0usize;
    let mut drained = false;

    for _ in 0..target {
        let content = std::fs::read_to_string(file)
            .with_context(|| format!("failed to read {}", file.display()))?;
        match classify_active_head(&content)? {
            HeadKind::None => break, // no queue component or no prompt left to strike
            HeadKind::IdBacked => {
                if struck.is_empty() {
                    bail!(
                        "{}: queue head is an id-backed directive, not a free-text prompt. \
                         Reap it through the normal closeout with `--done <id>` / `--pending-gate <id>` \
                         so the backlog item stays in sync — `queue consume` only strikes free-text heads.",
                        file.display()
                    );
                }
                // Already struck some free-text heads this run; stop cleanly at
                // the first id-backed head rather than desyncing it.
                break;
            }
            HeadKind::FreeText => {}
        }
        match crate::write::consume_queue_prompt_with_outcome(file)? {
            Some(outcome) => {
                struck.push(outcome.consumed_text);
                last_remaining = outcome.remaining;
                if outcome.drained {
                    drained = true;
                    break;
                }
            }
            None => break,
        }
    }

    if struck.is_empty() {
        println!(
            "{}: no free-text queue head to consume (queue inactive, empty, or id-backed head).",
            file.display()
        );
    } else {
        println!(
            "{}: consumed {} free-text queue head(s) (remaining: {}){}",
            file.display(),
            struck.len(),
            last_remaining,
            if drained {
                ", drained — cleared queue_active"
            } else {
                ""
            }
        );
    }
    Ok(())
}

/// `agent-doc queue prune-noise <FILE>` — strike every non-drainable noise queue
/// head at any position (`#goqstall2`), clearing pasted console output / agent
/// response fragments / bare observations that session-check surfaces as
/// `queue_stale_noise_lines=N`. Preserves id-backed directives and drainable
/// free-text heads. Supervisor-safe: routes through the same editor-IPC-converged
/// write path the closeout strikes use.
pub fn prune_noise(file: &Path) -> Result<()> {
    // #sqedit-race Phase 2: hold the queue-edit lease across the prune so the
    // supervisor idle-watch + preflight maintenance defer (single queue writer).
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let struck = crate::write::prune_noise_queue_heads(file)?;
    if struck == 0 {
        println!(
            "{}: no non-drainable noise queue heads to prune (queue inactive, empty, or all heads drainable).",
            file.display()
        );
    } else {
        println!(
            "{}: pruned {} non-drainable noise queue head(s).",
            file.display(),
            struck
        );
    }
    Ok(())
}

pub fn sync(file: &Path) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("failed to read {}", file.display()))?;

    let components = component::parse(&content)
        .with_context(|| format!("failed to parse components in {}", file.display()))?;

    let queue_comp = components.iter().find(|c| c.name == "queue");
    let Some(qc) = queue_comp else {
        bail!(
            "{}: no agent:queue component found. Add `<!-- agent:queue -->..<!-- /agent:queue -->` to the document.",
            file.display()
        );
    };

    let mut mode: Option<queue::BacklogQueueSyncMode> = None;
    let mut ids: Vec<String> = Vec::new();
    let mut enqueue_ids: Vec<String> = Vec::new();
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        enqueue_ids.extend(pending::active_enqueue_item_ids(body));
        let Some(value) = comp.attrs.get("queue") else {
            continue;
        };
        let Some(comp_mode) = queue::BacklogQueueSyncMode::parse(value) else {
            continue;
        };
        if mode.is_none() {
            mode = Some(comp_mode);
        }
        ids.extend(pending::active_item_ids(body));
    }
    if mode.is_none() && !enqueue_ids.is_empty() {
        mode = Some(queue::BacklogQueueSyncMode::Append);
    }
    ids.extend(enqueue_ids);

    let Some(effective_mode) = mode else {
        bail!(
            "{}: no agent:backlog/agent:icebox/agent:pending component carries a `queue` attribute or enqueue marker. \
             Add `<!-- agent:backlog queue -->` (or `queue=sync`, `queue=prepend`) or mark an item with `:inbox_tray:` / `/enqueue`.",
            file.display()
        );
    };

    if ids.is_empty() {
        bail!(
            "{}: no active backlog items found to sync. Add `[ ] [#id] ...` items to agent:backlog first.",
            file.display()
        );
    }

    let body = &content[qc.open_end..qc.close_start];
    let entries = queue::parse(body)
        .with_context(|| format!("failed to parse queue body in {}", file.display()))?;
    let existing_ids: std::collections::HashSet<String> = entries
        .iter()
        .filter_map(queue_entry_reference_id)
        .collect();
    let mut seen_existing = std::collections::HashSet::new();
    let already_present: Vec<String> = ids
        .iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| existing_ids.contains(id))
        .filter(|id| seen_existing.insert(id.clone()))
        .collect();

    let Some(synced) = queue::sync_backlog_into_queue(&entries, &ids, effective_mode) else {
        println!(
            "{}: queue already in sync ({} active backlog id(s), {:?} mode). No changes.",
            file.display(),
            ids.len(),
            effective_mode
        );
        return Ok(());
    };

    let new_body = queue::render(&synced);
    let new_content = qc.replace_content(&content, &new_body);

    std::fs::write(file, &new_content)
        .with_context(|| format!("failed to write {}", file.display()))?;

    let prompt_count = synced
        .iter()
        .filter(|e| matches!(e, queue::QueueEntry::Prompt(_)))
        .count();
    let mut seen_new = std::collections::HashSet::new();
    let newly_materialized: Vec<String> = synced
        .iter()
        .filter_map(queue_entry_reference_id)
        .filter(|id| !existing_ids.contains(id))
        .filter(|id| seen_new.insert(id.clone()))
        .collect();
    println!(
        "{}: synced {} backlog id(s) → {} queue prompt(s) ({:?} mode)",
        file.display(),
        ids.len(),
        prompt_count,
        effective_mode
    );
    if !already_present.is_empty() {
        println!(
            "{}: skipped already represented backlog id(s): {} (reason: already_in_queue)",
            file.display(),
            format_queue_ids(&already_present)
        );
    }
    if !newly_materialized.is_empty() {
        println!(
            "{}: materialized backlog id(s): {}",
            file.display(),
            format_queue_ids(&newly_materialized)
        );
    }

    if let Err(e) = snapshot::save(file, &new_content) {
        eprintln!("[queue sync] warning: failed to update snapshot: {}", e);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
        snapshot::save(&doc, content).unwrap();

        sync(&doc).expect("enqueue marker should append to queue");
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
        snapshot::save(&doc, content).unwrap();

        consume(&doc, 2).expect("consume two answered free-text heads");
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
        snapshot::save(&doc, content).unwrap();

        consume(&doc, 5).expect("consume should stop at the id-backed head");
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
        snapshot::save(&doc, content).unwrap();

        let err = consume(&doc, 1).unwrap_err();
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
        snapshot::save(&doc, content).unwrap();

        let err = consume(&doc, 1).unwrap_err();
        assert!(
            err.to_string().contains("id-backed"),
            "should refuse a leading id-backed head: {err}"
        );
    }
}
