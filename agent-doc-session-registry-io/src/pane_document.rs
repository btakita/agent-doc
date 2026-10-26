//! Resolve the document a tmux pane claims.
//!
//! The no-argument harness entrypoint — `/agent-doc` in Claude Code, Grok Build
//! and OpenCode, plain `agent-doc` in Codex — runs the normal cycle on the
//! document the current pane claimed with `agent-doc claim`. This is the one
//! binary-owned answer to "which document is that?", shared by the
//! `agent-doc document` command and the `UserPromptSubmit` hooks,
//! so the sealed cycle contract and a shell lookup can never disagree.
//!
//! It fails closed. Outside tmux, with no project root, with no claim, with a
//! claim on several documents, or with a claimed file that no longer exists,
//! it returns a typed refusal and never picks a document.

use std::fmt;
use std::path::{Path, PathBuf};

use agent_doc_session_registry::{PaneDocumentClaim, pane_document_claim};

/// A pane-document lookup that did not produce exactly one existing document.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneDocumentRefusal {
    /// No pane id: the process is not running inside a tmux pane.
    NotInTmux,
    /// `cwd` has no `.agent-doc` project root above it.
    NoProjectRoot { cwd: PathBuf },
    /// The registry exists but could not be read.
    RegistryUnreadable {
        pane: String,
        root: PathBuf,
        cause: String,
    },
    /// The registry binds no document to this pane.
    Unclaimed { pane: String, root: PathBuf },
    /// The registry binds several documents to this pane.
    Ambiguous {
        pane: String,
        root: PathBuf,
        files: Vec<String>,
    },
    /// The pane's claimed document is not a file on disk.
    ClaimedFileMissing { pane: String, file: PathBuf },
}

impl PaneDocumentRefusal {
    /// Stable snake_case kind, for logs and `--json` output.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::NotInTmux => "not_in_tmux",
            Self::NoProjectRoot { .. } => "no_project_root",
            Self::RegistryUnreadable { .. } => "registry_unreadable",
            Self::Unclaimed { .. } => "unclaimed",
            Self::Ambiguous { .. } => "ambiguous",
            Self::ClaimedFileMissing { .. } => "claimed_file_missing",
        }
    }
}

const EXPLICIT_FILE_REMEDY: &str = "Invoke the entrypoint with the document instead (`/agent-doc <FILE>`, or `agent-doc <FILE>` in Codex)";

impl fmt::Display for PaneDocumentRefusal {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotInTmux => write!(
                formatter,
                "no FILE argument and no tmux pane: `$TMUX_PANE` is unset, so there is no \
                 pane claim to resolve. {EXPLICIT_FILE_REMEDY}."
            ),
            Self::NoProjectRoot { cwd } => write!(
                formatter,
                "no FILE argument and no agent-doc project root above `{}`, so there is no \
                 session registry holding a pane claim. {EXPLICIT_FILE_REMEDY}.",
                cwd.display()
            ),
            Self::RegistryUnreadable { pane, root, cause } => write!(
                formatter,
                "no FILE argument and the session registry under `{}` could not be read to \
                 resolve pane {pane}'s claim: {cause}. {EXPLICIT_FILE_REMEDY}.",
                root.display()
            ),
            Self::Unclaimed { pane, root } => write!(
                formatter,
                "no FILE argument and pane {pane} claims no document in `{}`. Run \
                 `agent-doc claim <FILE>` in this pane first, or {}.",
                root.display(),
                lowercase_first(EXPLICIT_FILE_REMEDY)
            ),
            Self::Ambiguous { pane, root, files } => write!(
                formatter,
                "no FILE argument and pane {pane} claims {} documents in `{}` ({}); refusing to \
                 guess. {EXPLICIT_FILE_REMEDY}.",
                files.len(),
                root.display(),
                files.join(", ")
            ),
            Self::ClaimedFileMissing { pane, file } => write!(
                formatter,
                "no FILE argument and pane {pane}'s claimed document `{}` is not a file. Re-run \
                 `agent-doc claim <FILE>` in this pane, or {}.",
                file.display(),
                lowercase_first(EXPLICIT_FILE_REMEDY)
            ),
        }
    }
}

impl std::error::Error for PaneDocumentRefusal {}

fn lowercase_first(text: &str) -> String {
    let mut chars = text.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// The document `pane` claims, resolved from the project root above `cwd`.
///
/// `pane` is explicit (callers pass `$TMUX_PANE`) so the decision is testable
/// without a tmux server and never consults tmux's ambient "current" pane: a
/// hook spawned outside the pane must not borrow some other pane's claim.
pub fn resolve_pane_document(
    cwd: &Path,
    pane: Option<&str>,
) -> Result<PathBuf, PaneDocumentRefusal> {
    let pane = pane
        .map(str::trim)
        .filter(|pane| !pane.is_empty())
        .ok_or(PaneDocumentRefusal::NotInTmux)?;
    let root =
        agent_doc_fs::find_project_root(cwd).ok_or_else(|| PaneDocumentRefusal::NoProjectRoot {
            cwd: cwd.to_path_buf(),
        })?;
    // A project with no registry on disk has no claims; opening it would
    // create an empty `state.db` as a side effect of a read.
    if !crate::registry_path_in(&root).exists() {
        return Err(PaneDocumentRefusal::Unclaimed {
            pane: pane.to_string(),
            root,
        });
    }
    let registry =
        crate::load_in(&root).map_err(|err| PaneDocumentRefusal::RegistryUnreadable {
            pane: pane.to_string(),
            root: root.clone(),
            cause: format!("{err:#}"),
        })?;
    match pane_document_claim(&root, &registry, pane) {
        PaneDocumentClaim::Unclaimed => Err(PaneDocumentRefusal::Unclaimed {
            pane: pane.to_string(),
            root,
        }),
        PaneDocumentClaim::Ambiguous { files } => Err(PaneDocumentRefusal::Ambiguous {
            pane: pane.to_string(),
            root,
            files,
        }),
        PaneDocumentClaim::Claimed { file } => {
            let file = PathBuf::from(file);
            let file = if file.is_absolute() {
                file
            } else {
                root.join(file)
            };
            if file.is_file() {
                Ok(file)
            } else {
                Err(PaneDocumentRefusal::ClaimedFileMissing {
                    pane: pane.to_string(),
                    file,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_session_registry::{RegistryEntryFields, insert_registry_entry};
    use tempfile::TempDir;
    use tmux_router::Registry;

    fn project() -> TempDir {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        dir
    }

    fn claim(root: &Path, registry: &mut Registry, pane: &str, file: &str, session: &str) {
        insert_registry_entry(
            root,
            registry,
            RegistryEntryFields {
                session_id: session,
                pane_id: pane,
                file,
                pid: 1,
                cwd: &root.display().to_string(),
                started: "2026-10-26T00:00:00Z",
                window: "@1",
                supervisor_instance_id: "",
            },
        );
    }

    #[test]
    fn a_claimed_pane_resolves_to_its_document() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("tasks/plan.md"), "# plan\n").unwrap();
        std::fs::write(root.join("tasks/other.md"), "# other\n").unwrap();
        let mut registry = Registry::new();
        claim(root, &mut registry, "%7", "tasks/plan.md", "s-plan");
        claim(root, &mut registry, "%8", "tasks/other.md", "s-other");
        crate::save_in(root, &registry).unwrap();

        let resolved = resolve_pane_document(&root.join("tasks"), Some("%7")).unwrap();
        assert_eq!(resolved, root.join("tasks/plan.md"));
    }

    #[test]
    fn outside_tmux_fails_closed() {
        let dir = project();
        assert_eq!(
            resolve_pane_document(dir.path(), None),
            Err(PaneDocumentRefusal::NotInTmux)
        );
        assert_eq!(
            resolve_pane_document(dir.path(), Some("  ")),
            Err(PaneDocumentRefusal::NotInTmux)
        );
    }

    #[test]
    fn an_unclaimed_pane_fails_closed_and_names_claim() {
        let dir = project();
        let root = dir.path();
        // No registry on disk yet: unclaimed, and the read creates nothing.
        let refusal = resolve_pane_document(root, Some("%3")).unwrap_err();
        assert_eq!(refusal.kind(), "unclaimed");
        assert!(!crate::registry_path_in(root).exists());

        std::fs::write(root.join("tasks/plan.md"), "# plan\n").unwrap();
        let mut registry = Registry::new();
        claim(root, &mut registry, "%7", "tasks/plan.md", "s-plan");
        crate::save_in(root, &registry).unwrap();
        let refusal = resolve_pane_document(root, Some("%3")).unwrap_err();
        assert_eq!(refusal.kind(), "unclaimed");
        let message = refusal.to_string();
        assert!(message.contains("pane %3"), "{message}");
        assert!(message.contains("agent-doc claim <FILE>"), "{message}");
    }

    #[test]
    fn an_ambiguous_pane_fails_closed_and_lists_every_document() {
        let dir = project();
        let root = dir.path();
        std::fs::write(root.join("tasks/a.md"), "# a\n").unwrap();
        std::fs::write(root.join("tasks/b.md"), "# b\n").unwrap();
        let mut registry = Registry::new();
        claim(root, &mut registry, "%5", "tasks/a.md", "s-a");
        claim(root, &mut registry, "%5", "tasks/b.md", "s-b");
        crate::save_in(root, &registry).unwrap();

        let refusal = resolve_pane_document(root, Some("%5")).unwrap_err();
        assert_eq!(
            refusal,
            PaneDocumentRefusal::Ambiguous {
                pane: "%5".to_string(),
                root: root.to_path_buf(),
                files: vec!["tasks/a.md".to_string(), "tasks/b.md".to_string()],
            }
        );
        let message = refusal.to_string();
        assert!(message.contains("refusing to guess"), "{message}");
        assert!(message.contains("tasks/a.md, tasks/b.md"), "{message}");
    }

    #[test]
    fn a_claim_on_a_deleted_document_fails_closed() {
        let dir = project();
        let root = dir.path();
        let mut registry = Registry::new();
        claim(root, &mut registry, "%7", "tasks/gone.md", "s-gone");
        crate::save_in(root, &registry).unwrap();

        assert_eq!(
            resolve_pane_document(root, Some("%7")),
            Err(PaneDocumentRefusal::ClaimedFileMissing {
                pane: "%7".to_string(),
                file: root.join("tasks/gone.md"),
            })
        );
    }

    #[test]
    fn no_project_root_fails_closed() {
        let dir = TempDir::new().unwrap();
        let refusal = resolve_pane_document(dir.path(), Some("%1")).unwrap_err();
        assert_eq!(refusal.kind(), "no_project_root");
    }
}
