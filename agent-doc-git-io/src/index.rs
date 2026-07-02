use anyhow::Result;
use std::path::Path;
use std::process::{Command, Output, Stdio};

/// Write `content` to git's object database and return the blob hash.
pub fn hash_object(git_root: &Path, content: &str) -> Result<String> {
    let output = Command::new("git")
        .current_dir(git_root)
        .args(["hash-object", "-w", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .and_then(|mut child| {
            use std::io::Write;
            if let Some(ref mut stdin) = child.stdin {
                stdin.write_all(content.as_bytes())?;
            }
            child.wait_with_output()
        })?;
    if !output.status.success() {
        anyhow::bail!("git hash-object failed");
    }
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

pub fn update_index_cacheinfo(git_root: &Path, cacheinfo: &str) -> Result<Output> {
    Ok(Command::new("git")
        .current_dir(git_root)
        .args(["update-index", "--add", "--cacheinfo", cacheinfo])
        .output()?)
}

pub fn add_path(git_root: &Path, rel_path: &Path) -> Result<Output> {
    Ok(Command::new("git")
        .current_dir(git_root)
        .args(["add", "--"])
        .arg(rel_path)
        .output()?)
}

pub fn add_force(git_root: &Path, rel_path: &Path) -> Result<Output> {
    Ok(Command::new("git")
        .current_dir(git_root)
        .args(["add", "-f", "--"])
        .arg(rel_path)
        .output()?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn init_repo(root: &Path) {
        Command::new("git")
            .current_dir(root)
            .args(["init"])
            .output()
            .unwrap();
    }

    #[test]
    fn hash_object_writes_blob_content() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);

        let hash = hash_object(root, "blob body\n").unwrap();
        let output = Command::new("git")
            .current_dir(root)
            .args(["cat-file", "-p", &hash])
            .output()
            .unwrap();

        assert!(output.status.success());
        assert_eq!(String::from_utf8_lossy(&output.stdout), "blob body\n");
    }

    #[test]
    fn update_index_cacheinfo_stages_blob() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        let hash = hash_object(root, "staged body\n").unwrap();
        let cacheinfo = format!("100644,{hash},doc.md");

        let output = update_index_cacheinfo(root, &cacheinfo).unwrap();
        assert!(output.status.success());

        let staged = Command::new("git")
            .current_dir(root)
            .args(["diff", "--cached", "--name-only"])
            .output()
            .unwrap();
        assert!(staged.status.success());
        assert_eq!(String::from_utf8_lossy(&staged.stdout), "doc.md\n");
    }

    #[test]
    fn add_force_stages_ignored_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::write(root.join(".gitignore"), "doc.md\n").unwrap();
        fs::write(root.join("doc.md"), "body\n").unwrap();

        let output = add_force(root, Path::new("doc.md")).unwrap();
        assert!(output.status.success());

        let listed = Command::new("git")
            .current_dir(root)
            .args(["ls-files", "--error-unmatch", "--", "doc.md"])
            .output()
            .unwrap();
        assert!(listed.status.success());
    }

    #[test]
    fn add_path_stages_file() {
        let dir = tempfile::TempDir::new().unwrap();
        let root = dir.path();
        init_repo(root);
        fs::write(root.join("doc.md"), "body\n").unwrap();

        let output = add_path(root, Path::new("doc.md")).unwrap();
        assert!(output.status.success());

        let listed = Command::new("git")
            .current_dir(root)
            .args(["ls-files", "--error-unmatch", "--", "doc.md"])
            .output()
            .unwrap();
        assert!(listed.status.success());
    }
}
