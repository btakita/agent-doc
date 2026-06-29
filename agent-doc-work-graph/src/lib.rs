//! Pure work-graph analysis and rendering for tracked work items.
//!
//! This crate is source work-items in, typed graph/string renderings out. It
//! also exposes a document-text adapter for the current markdown session
//! format, but it does not read files, write documents, inspect git, dispatch
//! agents, or commit.

use agent_doc_element_backlog::backlog::{self, PendingState};
use anyhow::{Context, Result};
use serde::Serialize;

/// Which completion "lane" an item belongs to: the Auto-DAG's edge classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lane {
    /// Pure code/test/doc work an agent can complete + reap with no live pane.
    Implementable,
    /// Needs one operator live busy-pane / route / harness verification session.
    LiveVerify,
    /// Needs one operator IPC / typing-corruption capture session.
    IpcCapture,
    /// Blocked on a design decision.
    Blocked,
    /// Marked implemented/shipped in its own text; verify + reap.
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

/// Classify an item into a completion lane from its text.
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
    /// First line of the item text, bounded for quick scanning.
    pub summary: String,
}

/// One work item from any source adapter: a markdown document, another document,
/// or an external project-management integration.
#[derive(Debug, Clone)]
pub struct SourceWorkItem {
    pub id: String,
    pub text: String,
    pub done: bool,
}

/// The full classified work-graph for source work items.
#[derive(Debug, Clone, Serialize)]
pub struct AutoDag {
    pub items: Vec<DagItem>,
}

impl AutoDag {
    pub fn lane_items(&self, lane: Lane) -> Vec<&DagItem> {
        self.items.iter().filter(|item| item.lane == lane).collect()
    }
}

const LANES_IN_ORDER: [Lane; 5] = [
    Lane::Implementable,
    Lane::LiveVerify,
    Lane::IpcCapture,
    Lane::Blocked,
    Lane::LikelyDone,
];

fn summarize_text(text: &str) -> String {
    let first = text.lines().next().unwrap_or("").trim();
    let bounded: String = first.chars().take(100).collect();
    if first.chars().count() > 100 {
        format!("{bounded}…")
    } else {
        bounded
    }
}

/// Build the classified work-graph from source-adapter work items.
pub fn analyze_items(items: impl IntoIterator<Item = SourceWorkItem>) -> AutoDag {
    let items = items
        .into_iter()
        .filter(|item| !item.done)
        .map(|item| DagItem {
            id: item.id,
            lane: classify(&item.text),
            summary: summarize_text(&item.text),
        })
        .collect();
    AutoDag { items }
}

/// Build the classified work-graph from the current markdown document format.
pub fn analyze_document(content: &str) -> Result<AutoDag> {
    let components =
        agent_doc_element::element::parse(content).context("auto-dag: parse components")?;
    let mut source_items = Vec::new();
    for comp in &components {
        if !matches!(comp.name.as_str(), "backlog" | "review" | "icebox") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        let (_, parsed, _) = backlog::parse_items(body);
        for item in &parsed {
            source_items.push(SourceWorkItem {
                id: item.id.clone(),
                text: item.text.clone(),
                done: matches!(item.state, PendingState::Done),
            });
        }
    }
    Ok(analyze_items(source_items))
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
        let dag = analyze_document(content).unwrap();
        assert_eq!(dag.items.len(), 2, "the reaped [x] item is skipped");
        assert_eq!(dag.lane_items(Lane::LiveVerify).len(), 1);
        assert_eq!(dag.lane_items(Lane::Implementable).len(), 1);
    }

    #[test]
    fn analyze_items_accepts_non_document_sources() {
        let dag = analyze_items([
            SourceWorkItem {
                id: "pm-123".into(),
                text: "live-verify the imported PM task".into(),
                done: false,
            },
            SourceWorkItem {
                id: "pm-456".into(),
                text: "already reaped".into(),
                done: true,
            },
        ]);
        assert_eq!(dag.items.len(), 1);
        assert_eq!(dag.items[0].id, "pm-123");
        assert_eq!(dag.items[0].lane, Lane::LiveVerify);
    }

    #[test]
    fn render_mermaid_lists_only_non_empty_lanes() {
        let dag = AutoDag {
            items: vec![DagItem {
                id: "aaaa".into(),
                lane: Lane::Implementable,
                summary: "do it".into(),
            }],
        };
        let mermaid = render_mermaid(&dag);
        assert!(mermaid.contains("graph TD"));
        assert!(mermaid.contains("PC"));
        assert!(!mermaid.contains("PA["));
    }

    #[test]
    fn render_nested_list_includes_ids_and_summaries() {
        let dag = AutoDag {
            items: vec![DagItem {
                id: "aaaa".into(),
                lane: Lane::Implementable,
                summary: "do it".into(),
            }],
        };
        let list = render_nested_list(&dag);
        assert!(list.contains("Path C"));
        assert!(list.contains("#aaaa"));
        assert!(list.contains("do it"));
    }
}
