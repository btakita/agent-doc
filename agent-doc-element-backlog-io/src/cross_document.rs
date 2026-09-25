//! Cross-document tracked-work transfer evidence.
//!
//! The dropped-backlog guard stays fail-closed unless an item is still open in
//! exactly one other agent-doc document under the same project root. This is a
//! bounded, cycle-scoped filesystem observation: callers invoke it only for ids
//! already proven missing from the source document.

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CrossDocumentTransferEvidence {
    pub destinations: BTreeMap<String, PathBuf>,
}

impl CrossDocumentTransferEvidence {
    pub fn ids(&self) -> HashSet<String> {
        self.destinations.keys().cloned().collect()
    }
}

fn project_markdown_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    let mut dirs = vec![root.to_path_buf()];

    while let Some(dir) = dirs.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        let mut entries: Vec<_> = entries.flatten().collect();
        entries.sort_by_key(|entry| entry.file_name());

        for entry in entries.into_iter().rev() {
            let path = entry.path();
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            if file_type.is_dir() {
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if name.starts_with('.') || name == "node_modules" || name == "target" {
                    continue;
                }
                // A nested project owns its own tracked-work namespace.
                if path.join(".agent-doc").is_dir() || path.join(".git").exists() {
                    continue;
                }
                dirs.push(path);
            } else if file_type.is_file()
                && path.extension().and_then(|extension| extension.to_str()) == Some("md")
            {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

/// Find candidate ids that remain open in exactly one other document in the
/// same agent-doc project.
///
/// A prose/code mention is not evidence: only parsed open tracked-work
/// components count. Unreadable or malformed unrelated files are ignored, so
/// they cannot weaken the source document's fail-closed deletion guard.
pub fn transferred_open_ids(
    source_file: &Path,
    source_content: &str,
    candidate_ids: &HashSet<String>,
) -> Result<CrossDocumentTransferEvidence> {
    if candidate_ids.is_empty() {
        return Ok(CrossDocumentTransferEvidence::default());
    }

    let Some(root) = agent_doc_fs::find_project_root(source_file) else {
        return Ok(CrossDocumentTransferEvidence::default());
    };
    let (source_frontmatter, _) = agent_doc_frontmatter::frontmatter::parse(source_content)?;
    let mut matches: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();

    for candidate in project_markdown_files(&root) {
        if agent_doc_fs::same_document_path(source_file, &candidate) {
            continue;
        }
        let Ok(content) = std::fs::read_to_string(&candidate) else {
            continue;
        };
        let open_ids: HashSet<String> =
            agent_doc_element_backlog::backlog::open_tracked_work_ids_in_content(&content)
                .into_iter()
                .map(|id| agent_doc_element_backlog::backlog::normalize_pending_id(&id))
                .filter(|id| candidate_ids.contains(id))
                .collect();
        if open_ids.is_empty() {
            continue;
        }

        let (target_frontmatter, _) = agent_doc_frontmatter::frontmatter::parse(&content)
            .with_context(|| format!("failed to parse transfer target {}", candidate.display()))?;
        agent_doc_frontmatter_io::security_review::enforce_cross_document_review(
            "tracked-work transfer validation",
            source_file,
            &source_frontmatter,
            &candidate,
            Some(&target_frontmatter),
        )?;
        for id in open_ids {
            matches.entry(id).or_default().push(candidate.clone());
        }
    }

    let mut destinations = BTreeMap::new();
    for (id, paths) in matches {
        if paths.len() > 1 {
            anyhow::bail!(
                "tracked-work transfer for #{} is ambiguous: the id is open in multiple documents: {}",
                id,
                paths
                    .iter()
                    .map(|path| path.display().to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            );
        }
        destinations.insert(id, paths[0].clone());
    }

    Ok(CrossDocumentTransferEvidence { destinations })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn session_doc(items: &str) -> String {
        format!(
            "---\nagent_doc_session: sample\nagent_doc_format: template\n---\n\n\
             <!-- agent:exchange -->\n<!-- /agent:exchange -->\n\n\
             <!-- agent:backlog -->\n{items}\n<!-- /agent:backlog -->\n"
        )
    }

    #[test]
    fn finds_an_open_id_moved_to_one_other_project_document() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let source = dir.path().join("tasks/source.md");
        let destination = dir.path().join("tasks/destination.md");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        let source_content = session_doc("");
        std::fs::write(&source, &source_content).unwrap();
        std::fs::write(
            &destination,
            session_doc("- [ ] [#moved1] Continue this work in the destination"),
        )
        .unwrap();

        let evidence = transferred_open_ids(
            &source,
            &source_content,
            &HashSet::from(["moved1".to_string()]),
        )
        .unwrap();

        assert_eq!(evidence.destinations.get("moved1"), Some(&destination));
    }

    #[test]
    fn prose_mentions_do_not_count_as_transfer_evidence() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let source = dir.path().join("source.md");
        let note = dir.path().join("note.md");
        let source_content = session_doc("");
        std::fs::write(&source, &source_content).unwrap();
        std::fs::write(&note, "Mention #moved1 in prose only.\n").unwrap();

        let evidence = transferred_open_ids(
            &source,
            &source_content,
            &HashSet::from(["moved1".to_string()]),
        )
        .unwrap();

        assert!(evidence.destinations.is_empty());
    }

    #[test]
    fn duplicate_open_destinations_fail_closed() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let source = dir.path().join("source.md");
        let source_content = session_doc("");
        std::fs::write(&source, &source_content).unwrap();
        for name in ["first.md", "second.md"] {
            std::fs::write(
                dir.path().join(name),
                session_doc("- [ ] [#moved1] Duplicate destination"),
            )
            .unwrap();
        }

        let error = transferred_open_ids(
            &source,
            &source_content,
            &HashSet::from(["moved1".to_string()]),
        )
        .unwrap_err();

        assert!(error.to_string().contains("open in multiple documents"));
    }
}
