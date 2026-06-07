//! `agent-doc auto-dag <FILE>` — plan the completion work-graph for a session
//! document's backlog + review items (`#auto-dag-first-class`).
//!
//! Emulates the spirit of Claude Code `/goal`, harness-agnostic: parse the
//! `agent:backlog` and `agent:review` items, classify each by how it gets
//! completed (implementable now vs operator live-verify vs IPC-capture vs blocked
//! on a design decision vs already done), and emit the work-graph as a Mermaid
//! diagram and a nested list. Re-runnable, so the graph stays current as items
//! close. Pure read — never mutates the document.

use crate::pending::{self, PendingItem, PendingState};
use anyhow::{Context, Result};
use serde::Serialize;

/// Which completion "lane" an item belongs to — the auto-dag's edge classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// Pure code/test/doc work an agent can complete + reap with no live pane.
    Implementable,
    /// Needs one operator live busy-pane / route / harness verification session.
    LiveVerify,
    /// Needs one operator IPC / typing-corruption capture session.
    IpcCapture,
    /// Blocked on a design decision (e.g. `#subagent-blocks-session`).
    Blocked,
    /// Marked implemented/shipped in its own text — verify + reap.
    LikelyDone,
}

impl Lane {
    pub fn title(self) -> &'static str {
        match self {
            Lane::Implementable => "Path C — implementable now (no live pane, agent-executable)",
            Lane::LiveVerify => {
                "Path A — operator live-verify session (busy-pane / route / harness)"
            }
            Lane::IpcCapture => "Path B — operator IPC / typing-corruption capture session",
            Lane::Blocked => "Blocked on a design decision",
            Lane::LikelyDone => "Likely done — verify + reap",
        }
    }

    fn mermaid_node(self) -> &'static str {
        match self {
            Lane::Implementable => "PC",
            Lane::LiveVerify => "PA",
            Lane::IpcCapture => "PB",
            Lane::Blocked => "BLK",
            Lane::LikelyDone => "DONE",
        }
    }
}

/// Classify an item into a completion lane from its text (case-insensitive
/// keyword heuristics — the same signals an operator reads to decide how to
/// close an item). Order matters: blocked + likely-done win over the
/// session-class buckets.
pub fn classify(text: &str) -> Lane {
    let t = text.to_lowercase();
    if t.contains("#subagent-blocks-session")
        || t.contains("design decision")
        || t.contains("gated on the #subagent")
    {
        return Lane::Blocked;
    }
    if (t.contains("implemented") || t.contains("shipped"))
        && !t.contains("log review")
        && !t.contains("denies completion")
        && !t.contains("not resolved")
        && !t.contains("still incomplete")
    {
        return Lane::LikelyDone;
    }
    if t.contains("patchwatcher")
        || t.contains("file cache")
        || t.contains("typing-corruption")
        || t.contains("ipcfullprompt")
        || t.contains("ipc dup")
        || t.contains("ipc-dup")
        || t.contains("buffer-divergence")
        || (t.contains("ipc") && t.contains("duplicat"))
    {
        return Lane::IpcCapture;
    }
    if t.contains("live-verify")
        || t.contains("live verify")
        || t.contains("log review")
        || t.contains("live repro")
        || t.contains("live-repro")
        || t.contains("operator")
        || t.contains("busy pane")
        || t.contains("busy-pane")
    {
        return Lane::LiveVerify;
    }
    Lane::Implementable
}

/// One classified work-graph node.
#[derive(Debug, Clone, Serialize)]
pub struct DagItem {
    pub id: String,
    pub lane: Lane,
    /// First line of the item text, bounded for a quick scan.
    pub summary: String,
}

/// The full classified work-graph for a document.
#[derive(Debug, Clone, Serialize)]
pub struct AutoDag {
    pub items: Vec<DagItem>,
}

impl AutoDag {
    pub fn lane_items(&self, lane: Lane) -> Vec<&DagItem> {
        self.items.iter().filter(|i| i.lane == lane).collect()
    }
}

const LANES_IN_ORDER: [Lane; 5] = [
    Lane::Implementable,
    Lane::LiveVerify,
    Lane::IpcCapture,
    Lane::Blocked,
    Lane::LikelyDone,
];

fn summarize(item: &PendingItem) -> String {
    let first = item.text.lines().next().unwrap_or("").trim();
    let bounded: String = first.chars().take(100).collect();
    if first.chars().count() > 100 {
        format!("{bounded}…")
    } else {
        bounded
    }
}

/// Build the classified work-graph from a document's content.
pub fn analyze(content: &str) -> Result<AutoDag> {
    let components = crate::component::parse(content).context("auto-dag: parse components")?;
    let mut items = Vec::new();
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "review" | "icebox") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        let (_, parsed, _) = pending::parse_items(body);
        for item in &parsed {
            // Skip reaped/struck items — the graph plans *open* work.
            if matches!(item.state, PendingState::Done) {
                continue;
            }
            items.push(DagItem {
                id: item.id.clone(),
                lane: classify(&item.text),
                summary: summarize(item),
            });
        }
    }
    Ok(AutoDag { items })
}

/// Render the work-graph as a Mermaid `graph TD`.
pub fn render_mermaid(dag: &AutoDag) -> String {
    let mut out = String::from("```mermaid\ngraph TD\n");
    out.push_str("  GOAL[\"/goal: complete all actionable items\"]\n");
    for lane in LANES_IN_ORDER {
        let lane_items = dag.lane_items(lane);
        if lane_items.is_empty() {
            continue;
        }
        let node = lane.mermaid_node();
        out.push_str(&format!(
            "  GOAL --> {node}[\"{} ({} item(s))\"]\n",
            lane.title(),
            lane_items.len()
        ));
    }
    out.push_str("```\n");
    out
}

/// Render the work-graph as a nested list, lanes in completion-priority order.
pub fn render_nested_list(dag: &AutoDag) -> String {
    let mut out = String::new();
    for lane in LANES_IN_ORDER {
        let lane_items = dag.lane_items(lane);
        if lane_items.is_empty() {
            continue;
        }
        out.push_str(&format!("- **{}** ({})\n", lane.title(), lane_items.len()));
        for item in lane_items {
            out.push_str(&format!("  - `#{}` — {}\n", item.id, item.summary));
        }
    }
    out
}

/// `agent-doc auto-dag <FILE>` entry point.
pub fn run(file: &std::path::Path, json: bool) -> Result<()> {
    let content = std::fs::read_to_string(file)
        .with_context(|| format!("auto-dag: read {}", file.display()))?;
    let dag = analyze(&content)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&dag)?);
    } else {
        println!("# Auto-DAG: completion work-graph for {}\n", file.display());
        println!("{}", render_mermaid(&dag));
        println!("## Completion order\n");
        print!("{}", render_nested_list(&dag));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_routes_each_lane() {
        assert_eq!(
            classify("gated on the #subagent-blocks-session decision"),
            Lane::Blocked
        );
        assert_eq!(
            classify("IMPLEMENTED + shipped this session"),
            Lane::LikelyDone
        );
        assert_eq!(
            classify("LOG REVIEW 2026-06-03 denies completion: IMPLEMENTED but unverified"),
            Lane::LiveVerify,
            "a denies-completion log-review is not done even if it says implemented"
        );
        assert_eq!(
            classify("PatchWatcher typing-corruption capture needed"),
            Lane::IpcCapture
        );
        assert_eq!(
            classify("live-verify the busy-pane reopen"),
            Lane::LiveVerify
        );
        assert_eq!(
            classify("accept a leading bare #id token in --pending-add"),
            Lane::Implementable
        );
    }

    #[test]
    fn analyze_classifies_review_items_and_skips_done() {
        let content = concat!(
            "<!-- agent:review -->\n",
            "1. [/] [#aaaa] live-verify the busy-pane reopen\n",
            "2. [/] [#bbbb] accept a leading bare #id token in --pending-add\n",
            "3. [x] [#cccc] already reaped\n",
            "<!-- /agent:review -->\n",
        );
        let dag = analyze(content).unwrap();
        assert_eq!(dag.items.len(), 2, "the reaped [x] item is skipped");
        assert_eq!(dag.lane_items(Lane::LiveVerify).len(), 1);
        assert_eq!(dag.lane_items(Lane::Implementable).len(), 1);
        let md = render_mermaid(&dag);
        assert!(md.contains("graph TD"), "{md}");
        let list = render_nested_list(&dag);
        assert!(list.contains("`#bbbb`"), "{list}");
    }
}
