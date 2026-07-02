//! Compact archive file I/O adapters.

use std::path::Path;

/// Read every compact archive referenced by `head`, preserving pointer order and
/// ignoring missing or out-of-scope archive pointers.
pub fn read_head_compact_archives(file: &Path, head: &str) -> Vec<String> {
    agent_doc_document::compact_archive::compact_archive_pointers(head)
        .into_iter()
        .filter_map(|pointer| read_head_compact_archive(file, pointer))
        .collect()
}

/// Read a HEAD-referenced compact archive when the pointer resolves under the
/// owning project's `.agent-doc/archives` directory.
pub fn read_head_compact_archive(file: &Path, pointer: &str) -> Option<String> {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let project_root = agent_doc_project_root_io::project_root_containing(&canonical)?;
    let archive_root = project_root
        .join(".agent-doc/archives")
        .canonicalize()
        .ok()?;
    let pointer_path = Path::new(pointer);
    let archive_path = if pointer_path.is_absolute() {
        pointer_path.to_path_buf()
    } else {
        project_root.join(pointer_path)
    };
    let archive_path = archive_path.canonicalize().ok()?;
    if !archive_path.starts_with(&archive_root) {
        return None;
    }
    std::fs::read_to_string(archive_path).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_head_compact_archives_resolves_relative_pointers_under_archive_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        let archive_dir = root.join(".agent-doc/archives");
        std::fs::create_dir_all(&archive_dir).unwrap();
        let doc = root.join("session.md");
        std::fs::write(&doc, "visible").unwrap();
        std::fs::write(archive_dir.join("a.md"), "archived response").unwrap();

        let head = "*Compacted. Content archived to `.agent-doc/archives/a.md`*\n";

        assert_eq!(
            read_head_compact_archives(&doc, head),
            vec!["archived response".to_string()]
        );
    }

    #[test]
    fn read_head_compact_archive_rejects_pointers_outside_archive_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        let doc = root.join("session.md");
        let outside = root.join("outside.md");
        std::fs::write(&doc, "visible").unwrap();
        std::fs::write(&outside, "outside").unwrap();

        assert_eq!(read_head_compact_archive(&doc, "outside.md"), None);
    }
}
