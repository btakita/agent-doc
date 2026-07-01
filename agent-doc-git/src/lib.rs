//! Pure git command and path policy helpers.

use std::path::{Path, PathBuf};
use std::process::Output;

/// Compute `path` relative to `root`, canonicalizing both sides so symlinks do
/// not cause `strip_prefix` mismatches. Falls back through non-canonical strip
/// and finally to the original path.
pub fn relative_to_root(path: &Path, root: &Path) -> PathBuf {
    let canon_path = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
    let canon_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    if let Ok(rel) = canon_path.strip_prefix(&canon_root) {
        return rel.to_path_buf();
    }
    if let Ok(rel) = path.strip_prefix(root) {
        return rel.to_path_buf();
    }
    path.to_path_buf()
}

pub fn is_index_lock_contention_text(text: &str) -> bool {
    text.contains("index.lock") || text.contains("Unable to create")
}

pub fn render_git_process_output(output: &Output) -> String {
    render_git_streams(&output.stderr, &output.stdout)
}

fn render_git_streams(stderr: &[u8], stdout: &[u8]) -> String {
    let stderr = String::from_utf8_lossy(stderr).trim().to_string();
    let stdout = String::from_utf8_lossy(stdout).trim().to_string();
    match (stderr.is_empty(), stdout.is_empty()) {
        (false, true) => stderr,
        (true, false) => stdout,
        (false, false) => format!("{} | {}", stderr, stdout),
        (true, true) => "no git output".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::{is_index_lock_contention_text, relative_to_root, render_git_streams};
    use std::path::{Path, PathBuf};

    #[test]
    fn relative_to_root_strips_prefix_for_normal_paths() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/home/user/project/src/main.rs");
        let rel = relative_to_root(file, root);
        assert_eq!(rel, PathBuf::from("src/main.rs"));
    }

    #[test]
    fn relative_to_root_returns_original_when_no_common_prefix() {
        let root = Path::new("/home/user/project");
        let file = Path::new("/other/path/file.rs");
        let rel = relative_to_root(file, root);
        assert_eq!(rel, PathBuf::from("/other/path/file.rs"));
    }

    #[cfg(unix)]
    #[test]
    fn relative_to_root_handles_symlinked_path() {
        let real_dir = tempfile::TempDir::new().unwrap();
        let link_dir = tempfile::TempDir::new().unwrap();
        let real_root = real_dir.path();
        let link_path = link_dir.path().join("link");

        let subdir = real_root.join("tasks");
        std::fs::create_dir_all(&subdir).unwrap();
        std::fs::write(subdir.join("doc.md"), "content").unwrap();
        std::os::unix::fs::symlink(real_root, &link_path).unwrap();

        let file_via_symlink = link_path.join("tasks/doc.md");
        assert!(file_via_symlink.exists());

        let rel = relative_to_root(&file_via_symlink, real_root);
        assert_eq!(rel, PathBuf::from("tasks/doc.md"));
    }

    #[test]
    fn index_lock_contention_matches_git_lock_messages() {
        assert!(is_index_lock_contention_text(
            "fatal: Unable to create '/repo/.git/index.lock': File exists."
        ));
        assert!(is_index_lock_contention_text(
            "error: could not write index.lock"
        ));
        assert!(!is_index_lock_contention_text(
            "fatal: not a git repository"
        ));
    }

    #[test]
    fn render_git_streams_prefers_meaningful_output() {
        assert_eq!(render_git_streams(b"fatal\n", b""), "fatal");
        assert_eq!(render_git_streams(b"", b"ok\n"), "ok");
        assert_eq!(render_git_streams(b"fatal\n", b"hint\n"), "fatal | hint");
        assert_eq!(render_git_streams(b" \n", b"\n"), "no git output");
    }
}
