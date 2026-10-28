//! Document-wide runtime lifecycle projection across every queue
//! (`#queuemirrormarkers`).
//!
//! A queue claim, report receipt, and completion cell are keyed by work
//! identity, never by queue id: a claimed identity is held out of every queue's
//! drain, not only the queue it was dispatched from. The visible lifecycle
//! marker and `[#owner#generation]` annotation are a projection of that same
//! durable state, so every queue that displays the identity must show it. A
//! projection that rewrites only the scheduler-selected queue leaves a mirrored
//! or sibling copy frozen at whatever the last pass that selected its queue
//! painted (an expired generation, a released report, a `🚧` whose turn ended).
//!
//! This module owns only the traversal. Callers own the evidence and build the
//! per-queue targets, so the claim command path, preflight maintenance, and the
//! relay reconciler keep one row rule while all of them reach every queue.

use agent_doc_element::element;
use anyhow::Result;

use crate::document_queue::{self, QueueEntry, QueueRowProjection};
use crate::queue_graph::QueueGraph;

/// One queue block offered to a target builder.
#[derive(Debug, Clone, Copy)]
pub struct QueueProjectionBlock<'a> {
    /// Canonical queue id (`default` for a lone unkeyed queue).
    pub queue_id: &'a str,
    /// True for the queue the scheduler selects (or the only queue). Only this
    /// queue may reopen a struck row on durable live evidence; a sibling's
    /// struck row is that sibling's own terminal or pass-owned evidence
    /// (`agent:loop` keeps struck rows for its restore), so another queue's
    /// claim must never resurrect it.
    pub scheduler_selected: bool,
    /// Parsed body entries of this queue.
    pub entries: &'a [QueueEntry],
}

/// Apply runtime row projections to every queue component of `content`,
/// skipping `skip_queue_id` when the caller already projected that queue with
/// pass-local evidence. Returns the projected document when any queue changed.
///
/// Queues are rewritten last-to-first so each component's byte range, taken
/// from the one parse of `content`, stays valid while later queues are
/// replaced. A document without a queue is returned unchanged.
pub fn project_runtime_projections_across_queues(
    content: &str,
    skip_queue_id: Option<&str>,
    mut targets_for: impl FnMut(QueueProjectionBlock<'_>) -> Vec<QueueRowProjection>,
) -> Result<Option<String>> {
    let components = element::parse(content)?;
    let graph = QueueGraph::parse(content, &components)?;
    let blocks = graph.blocks();
    let selected_occurrence = graph.selected().map(|block| block.occurrence).or_else(|| {
        // Legacy single-queue fallback: the only queue is the selected one even
        // when the scheduler reports it idle or blocked.
        (blocks.len() == 1).then(|| blocks[0].occurrence)
    });
    let mut projected = content.to_string();
    let mut changed = false;
    for block in blocks.iter().rev() {
        if skip_queue_id.is_some_and(|skip| skip == block.id) {
            continue;
        }
        let targets = targets_for(QueueProjectionBlock {
            queue_id: &block.id,
            scheduler_selected: selected_occurrence == Some(block.occurrence),
            entries: &block.entries,
        });
        if targets.is_empty() {
            continue;
        }
        let body = block.component.content(content);
        if let Some((new_body, _)) =
            document_queue::project_prompt_runtime_projections(body, &targets)?
        {
            projected = block.component.replace_content(&projected, &new_body);
            changed = true;
        }
    }
    Ok(changed.then_some(projected))
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_document::queue_projection::{QueueLifecycleMarker, QueueOwnerProjection};

    fn doc(main: &str, issues: &str) -> String {
        format!(
            "## Queue: Main\n<!-- agent:run id=\"main\" subagents -->\n{main}<!-- /agent:run -->\n\n\
## Run: Issues\n<!-- agent:loop id=issues interval=1h priority -->\n{issues}<!-- /agent:loop -->\n"
        )
    }

    fn owner(name: &str, generation: u64) -> Option<QueueOwnerProjection> {
        Some(QueueOwnerProjection {
            owner: name.to_string(),
            generation,
        })
    }

    /// Case 1 of the 2026-10-26 drift: the identity is claimed by generation 2
    /// and the loop still shows the expired generation 1. One traversal paints
    /// the authoritative attempt on both queues.
    #[test]
    fn every_queue_displaying_an_identity_receives_its_authoritative_attempt() {
        let content = doc(
            "- 🚧 [#subagent:layout_liveverify#2] do [#layoutunprovenliveverify]\n",
            "- 🚧 [#subagent:layout_liveverify#1] do [#layoutunprovenliveverify]\n",
        );
        let projected = project_runtime_projections_across_queues(&content, None, |block| {
            block
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    QueueEntry::Prompt(prompt) => Some(QueueRowProjection {
                        text: prompt.text.clone(),
                        marker: QueueLifecycleMarker::Running,
                        owner: owner("subagent:layout_liveverify", 2),
                        blockers: Vec::new(),
                        reopen_completed: false,
                    }),
                    _ => None,
                })
                .collect()
        })
        .unwrap()
        .expect("the stale loop copy is repainted");
        assert_eq!(
            projected,
            doc(
                "- 🚧 [#subagent:layout_liveverify#2] do [#layoutunprovenliveverify]\n",
                "- 🚧 [#subagent:layout_liveverify#2] do [#layoutunprovenliveverify]\n",
            )
        );
        assert_eq!(
            project_runtime_projections_across_queues(&projected, None, |block| {
                block
                    .entries
                    .iter()
                    .filter_map(|entry| match entry {
                        QueueEntry::Prompt(prompt) => Some(QueueRowProjection {
                            text: prompt.text.clone(),
                            marker: QueueLifecycleMarker::Running,
                            owner: owner("subagent:layout_liveverify", 2),
                            blockers: Vec::new(),
                            reopen_completed: false,
                        }),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap(),
            None,
            "the traversal is idempotent"
        );
    }

    #[test]
    fn a_skipped_queue_is_left_byte_identical_and_selection_is_reported() {
        let content = doc("- 🚧 do [#a]\n", "- 🚧 do [#a]\n");
        let mut seen = Vec::new();
        let projected =
            project_runtime_projections_across_queues(&content, Some("main"), |block| {
                seen.push((block.queue_id.to_string(), block.scheduler_selected));
                block
                    .entries
                    .iter()
                    .filter_map(|entry| match entry {
                        QueueEntry::Prompt(prompt) => Some(QueueRowProjection {
                            text: prompt.text.clone(),
                            marker: QueueLifecycleMarker::Pending,
                            owner: None,
                            blockers: Vec::new(),
                            reopen_completed: false,
                        }),
                        _ => None,
                    })
                    .collect()
            })
            .unwrap()
            .unwrap();
        assert_eq!(seen, vec![("issues".to_string(), false)]);
        assert_eq!(projected, doc("- 🚧 do [#a]\n", "- 📥 do [#a]\n"));
    }

    #[test]
    fn a_document_without_queues_is_unchanged() {
        assert_eq!(
            project_runtime_projections_across_queues("# no queue\n", None, |_| Vec::new())
                .unwrap(),
            None
        );
    }

    /// `#fencedmarkerscan`: the live 2026-10-26 `agent-doc-bugs.md` shape. A
    /// pasted report's fence holds a bare `<!-- agent:run priority subagents -->`
    /// line, a heading-anchored copy, and a bare `<!-- /agent:run -->` line.
    /// The element parser, the Markdown-AST overlay (and through it the
    /// document-cell merge scan), and the queue graph all see one `main`
    /// queue holding the rows on both sides of the fence, and the
    /// document-wide projection reaches both rows.
    #[test]
    fn fenced_run_markers_are_content_for_every_queue_scanner() {
        let fenced = concat!(
            "```\n",
            "## Run\n",
            "<!-- agent:run priority subagents -->\n",
            "<!-- /agent:run -->\n",
            "## Run <!-- agent:run priority subagents -->\n",
            "```\n",
        );
        let content = doc(
            &format!("- before the fence\n---\nPasted report:\n{fenced}---\n- after the fence\n"),
            "- onboarding\n",
        );
        let components = element::parse(&content).unwrap();
        assert_eq!(
            components.iter().filter(|c| c.name == "queue").count(),
            2,
            "element parser: main + issues"
        );
        let overlay = agent_doc_markdown_ast::overlay::components(&content);
        let overlay_queues = overlay.iter().filter(|c| c.name == "queue").collect::<Vec<_>>();
        assert_eq!(overlay_queues.len(), 2, "overlay: main + issues");
        let main_texts = overlay_queues[0]
            .items
            .iter()
            .map(|item| item.text.as_str())
            .collect::<Vec<_>>();
        assert!(main_texts.contains(&"before the fence"), "{main_texts:?}");
        assert!(main_texts.contains(&"after the fence"), "{main_texts:?}");
        let scan = agent_doc_markdown_ast::overlay::ComponentMarkerScan::new(&content);
        let mut offset = 0usize;
        let mut openers = 0usize;
        let mut closers = 0usize;
        for line in content.split_inclusive('\n') {
            let text = line.trim_end_matches('\n');
            openers += usize::from(scan.open_at(offset, text).is_some());
            closers += usize::from(scan.close_at(offset, text).is_some());
            offset += line.len();
        }
        assert_eq!((openers, closers), (2, 2), "shared marker scan: fenced markers are content");

        let graph = QueueGraph::parse(&content, &components).unwrap();
        let main = graph
            .blocks()
            .iter()
            .find(|block| block.id == "main")
            .expect("main block");
        let prompts = main
            .entries
            .iter()
            .filter_map(|entry| match entry {
                QueueEntry::Prompt(prompt) => Some(prompt.text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert!(prompts.contains(&"before the fence"), "{prompts:?}");
        assert!(prompts.contains(&"after the fence"), "{prompts:?}");

        let projected = project_runtime_projections_across_queues(&content, None, |block| {
            block
                .entries
                .iter()
                .filter_map(|entry| match entry {
                    QueueEntry::Prompt(prompt) if prompt.text.ends_with("the fence") => {
                        Some(QueueRowProjection {
                            text: prompt.text.clone(),
                            marker: QueueLifecycleMarker::Pending,
                            owner: None,
                            blockers: Vec::new(),
                            reopen_completed: false,
                        })
                    }
                    _ => None,
                })
                .collect()
        })
        .unwrap()
        .expect("both rows gain a pending marker");
        assert!(projected.contains("- 📥 before the fence\n"), "{projected}");
        assert!(projected.contains("- 📥 after the fence\n"), "{projected}");
        assert!(projected.contains(fenced), "{projected}");
    }
}
