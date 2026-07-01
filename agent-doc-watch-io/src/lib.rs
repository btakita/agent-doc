use anyhow::Result;
use std::path::{Path, PathBuf};

pub const PID_FILE: &str = ".agent-doc/watch.pid";

pub fn pid_path(base_dir: &Path) -> PathBuf {
    base_dir.join(PID_FILE)
}

/// Check whether a PID is alive using the same `/proc` probe as the watcher.
pub fn pid_alive(pid: u32) -> bool {
    Path::new(&format!("/proc/{pid}")).exists()
}

pub fn read_pid() -> Option<u32> {
    read_pid_in(&std::env::current_dir().ok()?)
}

pub fn read_pid_in(base_dir: &Path) -> Option<u32> {
    let content = std::fs::read_to_string(pid_path(base_dir)).ok()?;
    content.trim().parse().ok()
}

pub fn write_current_pid() -> Result<()> {
    write_current_pid_in(&std::env::current_dir()?)
}

pub fn write_current_pid_in(base_dir: &Path) -> Result<()> {
    let pid_path = pid_path(base_dir);
    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(pid_path, std::process::id().to_string())?;
    Ok(())
}

pub fn remove_pid() {
    let _ = std::fs::remove_file(PID_FILE);
}

pub fn remove_pid_in(base_dir: &Path) {
    let _ = std::fs::remove_file(pid_path(base_dir));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pid_file_roundtrip() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

        write_current_pid_in(dir.path()).unwrap();
        let pid = read_pid_in(dir.path()).unwrap();
        assert_eq!(pid, std::process::id());

        remove_pid_in(dir.path());
        assert!(read_pid_in(dir.path()).is_none());
    }

    #[test]
    fn pid_alive_self() {
        assert!(pid_alive(std::process::id()));
    }

    #[test]
    fn pid_alive_nonexistent() {
        assert!(!pid_alive(4_294_967_295));
    }
}
