//! Editor-surface vs tmux pane placement divergence (GH #62).
//!
//! The editor surface names the documents the operator can see; the structural
//! layout owner is supposed to hold exactly those documents' panes in the
//! `agent-doc` window and park the rest in `stash`. When auto-sync stops (or
//! syncs against a surface that no longer matches the editor), a visible
//! document's pane stays stashed or disappears entirely, and nothing reported
//! it: diagnosing that took reading three disagreeing views by hand. This fold
//! turns those facts into a verdict `reliable-sync-status` can print.

use serde::{Deserialize, Serialize};

/// The tmux window a document's pane currently lives in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "name")]
pub enum PaneWindow {
    /// The structural `agent-doc` window.
    AgentDoc,
    /// The `stash` window (panes parked out of the visible layout).
    Stash,
    /// Any other window, by name.
    Other(String),
}

impl PaneWindow {
    /// Classify a tmux window name.
    pub fn from_window_name(name: &str) -> Self {
        match name {
            "agent-doc" => Self::AgentDoc,
            name if name == "stash" || name.starts_with("stash") => Self::Stash,
            other => Self::Other(other.to_string()),
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Self::AgentDoc => "agent-doc",
            Self::Stash => "stash",
            Self::Other(name) => name,
        }
    }
}

/// One tmux pane bound to a document by its `agent-doc` owner process.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PanePlacement {
    /// Absolute document path.
    pub document: String,
    pub pane_id: String,
    /// `session:window_index`.
    pub window_target: String,
    pub window: PaneWindow,
}

/// A divergence between the editor surface and tmux pane placement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum PlacementFinding {
    /// A document the editor shows has its pane parked in `stash`.
    VisibleDocumentStashed { document: String, pane_id: String },
    /// A document the editor shows has no tmux pane at all.
    VisibleDocumentWithoutPane { document: String },
    /// A pane occupies the `agent-doc` window for a document the editor does
    /// not show.
    HiddenDocumentInAgentDocWindow { document: String, pane_id: String },
}

impl PlacementFinding {
    pub fn describe(&self) -> String {
        match self {
            Self::VisibleDocumentStashed { document, pane_id } => {
                format!("visible document's pane {pane_id} is parked in stash: {document}")
            }
            Self::VisibleDocumentWithoutPane { document } => {
                format!("visible document has no tmux pane: {document}")
            }
            Self::HiddenDocumentInAgentDocWindow { document, pane_id } => {
                format!(
                    "pane {pane_id} occupies the agent-doc window for a document the editor does not show: {document}"
                )
            }
        }
    }
}

/// Compare the editor's visible documents against where their panes live.
///
/// `visible` and every `placement.document` must already be absolute paths in
/// the same normalization. Findings are ordered by `visible` first, then by
/// placement order, so the output is deterministic.
pub fn surface_pane_divergence(
    visible: &[String],
    placements: &[PanePlacement],
) -> Vec<PlacementFinding> {
    let mut findings = Vec::new();
    for document in visible {
        let panes: Vec<&PanePlacement> = placements
            .iter()
            .filter(|placement| &placement.document == document)
            .collect();
        if panes.is_empty() {
            findings.push(PlacementFinding::VisibleDocumentWithoutPane {
                document: document.clone(),
            });
            continue;
        }
        // A document with a pane in the agent-doc window is placed correctly
        // even if a duplicate pane also sits in stash.
        if panes
            .iter()
            .any(|placement| placement.window == PaneWindow::AgentDoc)
        {
            continue;
        }
        if let Some(stashed) = panes
            .iter()
            .find(|placement| placement.window == PaneWindow::Stash)
        {
            findings.push(PlacementFinding::VisibleDocumentStashed {
                document: document.clone(),
                pane_id: stashed.pane_id.clone(),
            });
        }
    }
    for placement in placements {
        if placement.window == PaneWindow::AgentDoc && !visible.contains(&placement.document) {
            findings.push(PlacementFinding::HiddenDocumentInAgentDocWindow {
                document: placement.document.clone(),
                pane_id: placement.pane_id.clone(),
            });
        }
    }
    findings
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pane(document: &str, pane_id: &str, window: PaneWindow) -> PanePlacement {
        PanePlacement {
            document: document.to_string(),
            pane_id: pane_id.to_string(),
            window_target: "0:0".to_string(),
            window,
        }
    }

    #[test]
    fn classifies_window_names() {
        assert_eq!(
            PaneWindow::from_window_name("agent-doc"),
            PaneWindow::AgentDoc
        );
        assert_eq!(PaneWindow::from_window_name("stash"), PaneWindow::Stash);
        assert_eq!(PaneWindow::from_window_name("stash-2"), PaneWindow::Stash);
        assert_eq!(
            PaneWindow::from_window_name("zsh"),
            PaneWindow::Other("zsh".to_string())
        );
    }

    #[test]
    fn converged_layout_has_no_findings() {
        let visible = vec!["/p/a.md".to_string(), "/p/b.md".to_string()];
        let placements = vec![
            pane("/p/a.md", "%1", PaneWindow::AgentDoc),
            pane("/p/b.md", "%2", PaneWindow::AgentDoc),
            pane("/p/c.md", "%3", PaneWindow::Stash),
        ];
        assert!(surface_pane_divergence(&visible, &placements).is_empty());
    }

    /// The GH #62 shape: two visible documents, one pane in agent-doc, the
    /// other visible document's pane stashed, and a third visible document
    /// with no pane at all.
    #[test]
    fn reports_stashed_and_missing_visible_panes() {
        let visible = vec![
            "/p/agent-doc.md".to_string(),
            "/p/laptop.md".to_string(),
            "/p/pmt2.md".to_string(),
        ];
        let placements = vec![
            pane("/p/agent-doc.md", "%28", PaneWindow::AgentDoc),
            pane("/p/pmt2.md", "%24", PaneWindow::Stash),
            pane("/p/1097.md", "%25", PaneWindow::Stash),
        ];
        assert_eq!(
            surface_pane_divergence(&visible, &placements),
            vec![
                PlacementFinding::VisibleDocumentWithoutPane {
                    document: "/p/laptop.md".to_string()
                },
                PlacementFinding::VisibleDocumentStashed {
                    document: "/p/pmt2.md".to_string(),
                    pane_id: "%24".to_string()
                },
            ]
        );
    }

    #[test]
    fn reports_hidden_document_holding_agent_doc_window() {
        let visible = vec!["/p/a.md".to_string()];
        let placements = vec![
            pane("/p/a.md", "%1", PaneWindow::AgentDoc),
            pane("/p/old.md", "%9", PaneWindow::AgentDoc),
        ];
        assert_eq!(
            surface_pane_divergence(&visible, &placements),
            vec![PlacementFinding::HiddenDocumentInAgentDocWindow {
                document: "/p/old.md".to_string(),
                pane_id: "%9".to_string()
            }]
        );
    }

    #[test]
    fn duplicate_stashed_pane_is_fine_when_one_pane_is_placed() {
        let visible = vec!["/p/a.md".to_string()];
        let placements = vec![
            pane("/p/a.md", "%1", PaneWindow::Stash),
            pane("/p/a.md", "%2", PaneWindow::AgentDoc),
        ];
        assert!(surface_pane_divergence(&visible, &placements).is_empty());
    }

    #[test]
    fn pane_in_other_window_is_not_reported_as_stashed() {
        let visible = vec!["/p/a.md".to_string()];
        let placements = vec![pane("/p/a.md", "%1", PaneWindow::Other("w".to_string()))];
        assert!(surface_pane_divergence(&visible, &placements).is_empty());
    }
}
