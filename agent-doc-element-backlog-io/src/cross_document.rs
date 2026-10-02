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

    for candidate in agent_doc_fs::project_markdown_files(&root) {
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

/// Ids tracked by an item in `content`: every bracketed `[#id]` outside
/// `exchange`, so open, gated, and done items in any tracked-work component all
/// count, while a tag merely written in a response does not.
fn document_tracked_ids(content: &str) -> HashSet<String> {
    let Ok(components) = agent_doc_element::element::parse(content) else {
        return HashSet::new();
    };
    components
        .iter()
        .filter(|component| component.name != "exchange")
        .flat_map(|component| {
            agent_doc_element_backlog::backlog::extract_pending_ids_from_text(
                component.content(content),
            )
        })
        .collect()
}

/// Which `candidates` are tracked by some session document under `root`, and
/// where (GH 92).
///
/// A response or commit that cites a decision tracked in a sibling session
/// document is referencing it, not inventing it. The coined-id guards used to
/// know only the active document's ledger, so the remedies they offered were
/// both wrong: `--backlog-add` files a second live record of one decision, and
/// "reuse an existing id" had nothing to reuse in the document they looked at.
/// The sibling's own done archives count too, through the same predicate the
/// active document uses (`#coinedguardledgerasymmetry`).
pub fn sibling_tracked_ids(root: &Path, candidates: &[String]) -> BTreeMap<String, PathBuf> {
    let mut resolved = BTreeMap::new();
    if candidates.is_empty() {
        return resolved;
    }
    for document in agent_doc_fs::project_session_documents(root) {
        let Ok(content) = std::fs::read_to_string(&document) else {
            continue;
        };
        let mut tracked = document_tracked_ids(&content);
        if let Ok(archived) = crate::done_archive::archived_tracked_ids(&document, &content) {
            tracked.extend(archived);
        }
        for id in candidates {
            if !resolved.contains_key(id) && tracked.contains(id) {
                resolved.insert(id.clone(), document.clone());
            }
        }
        if resolved.len() == candidates.len() {
            break;
        }
    }
    resolved
}

/// Narrow ids the active ledger did not vouch for to the ones that resolve
/// NOWHERE in the project: not an instruction or source anchor
/// (`#hookhashanchortags`), not tracked in a sibling session document (GH 92).
///
/// The single post-ledger predicate both coined-id guards read, so the
/// `PreToolUse` hook and `session-check` cannot drift on what "resolves" means.
/// Each widening pass is lazy — it runs only when ids are still left — because
/// the common call has nothing to report and must not touch the disk.
pub fn unresolved_in_project(root: &Path, coined: Vec<String>) -> Vec<String> {
    if coined.is_empty() {
        return coined;
    }
    let anchors = agent_doc_fs::instruction_surface_anchors(root);
    let coined: Vec<String> = coined
        .into_iter()
        .filter(|tag| !anchors.contains(tag))
        .collect();
    if coined.is_empty() {
        return coined;
    }
    let siblings = sibling_tracked_ids(root, &coined);
    coined
        .into_iter()
        .filter(|tag| !siblings.contains_key(tag))
        .collect()
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

    /// GH 92: an id tracked (even gated) in a sibling session document is a
    /// citation, and resolves to the document that owns it; an id tracked
    /// nowhere stays unresolved, and a tag only written in a sibling's
    /// exchange vouches for nothing.
    #[test]
    fn sibling_tracked_ids_resolve_cross_document_citations() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join("tasks/pmt2/mr")).unwrap();
        let owner = dir.path().join("tasks/pmt2/offline-mode.md");
        std::fs::write(
            &owner,
            session_doc("- [/] [#pushurl] push. What is the url?"),
        )
        .unwrap();
        std::fs::write(
            dir.path().join("tasks/pmt2/mr/1099.md"),
            "---\nagent_doc_session: s2\n---\n\n<!-- agent:exchange -->\nI coined [#selfvouch] here\n<!-- /agent:exchange -->\n",
        )
        .unwrap();

        let candidates = vec![
            "pushurl".to_string(),
            "selfvouch".to_string(),
            "nowhere".to_string(),
        ];
        let resolved = sibling_tracked_ids(dir.path(), &candidates);
        assert_eq!(resolved.get("pushurl"), Some(&owner));
        assert!(!resolved.contains_key("selfvouch"), "{resolved:?}");
        assert!(!resolved.contains_key("nowhere"), "{resolved:?}");

        assert_eq!(
            unresolved_in_project(dir.path(), candidates),
            vec!["selfvouch".to_string(), "nowhere".to_string()]
        );
    }
}
