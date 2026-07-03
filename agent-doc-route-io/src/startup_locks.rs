//! Route startup lock acquisition.

use anyhow::{Context, Result};
use fs2::FileExt;
use std::fs::{File, OpenOptions};
use std::path::Path;

pub struct StartupLocks {
    _doc: File,
    _session: File,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartupLockMode {
    Blocking,
    Try,
}

pub enum StartupLockAcquire {
    Acquired(Option<StartupLocks>),
    Busy,
}

pub fn open_start_lock(path: &Path) -> Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .write(true)
        .truncate(false)
        .open(path)
        .with_context(|| format!("failed to open startup lock {}", path.display()))
}

pub fn lock_startup_file(lock: &File, lock_path: &Path, mode: StartupLockMode) -> Result<bool> {
    match mode {
        StartupLockMode::Blocking => {
            lock.lock_exclusive().with_context(|| {
                format!("failed to acquire startup lock {}", lock_path.display())
            })?;
            Ok(true)
        }
        StartupLockMode::Try => match lock.try_lock_exclusive() {
            Ok(()) => Ok(true),
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => Ok(false),
            Err(err) => Err(err)
                .with_context(|| format!("failed to acquire startup lock {}", lock_path.display())),
        },
    }
}

pub fn acquire_startup_locks(
    file: &Path,
    session_name: &str,
    mode: StartupLockMode,
) -> Result<StartupLockAcquire> {
    let Some(doc_lock_path) = agent_doc_fs::startup_document_lock_path_for(file) else {
        return Ok(StartupLockAcquire::Acquired(None));
    };
    let Some(session_lock_path) = agent_doc_fs::startup_session_lock_path_for(file, session_name)
    else {
        return Ok(StartupLockAcquire::Acquired(None));
    };

    let doc_lock = open_start_lock(&doc_lock_path)?;
    if !lock_startup_file(&doc_lock, &doc_lock_path, mode)? {
        return Ok(StartupLockAcquire::Busy);
    }

    let session_lock = open_start_lock(&session_lock_path)?;
    if !lock_startup_file(&session_lock, &session_lock_path, mode)? {
        return Ok(StartupLockAcquire::Busy);
    }

    Ok(StartupLockAcquire::Acquired(Some(StartupLocks {
        _doc: doc_lock,
        _session: session_lock,
    })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn try_startup_lock_reports_busy_without_waiting() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let doc = tmp.path().join("session.md");
        std::fs::write(&doc, "---\nagent_doc_session: startup-lock-test\n---\n").unwrap();

        let starting_dir =
            agent_doc_fs::startup_starting_dir_for(&doc).expect("project root should resolve");
        std::fs::create_dir_all(&starting_dir).unwrap();
        let lock_path = agent_doc_fs::startup_document_lock_path_for(&doc)
            .expect("document startup lock path should resolve");
        let held_doc_lock = open_start_lock(&lock_path).unwrap();
        fs2::FileExt::lock_exclusive(&held_doc_lock).unwrap();

        let start = std::time::Instant::now();
        let acquired =
            acquire_startup_locks(&doc, "startup-lock-test-session", StartupLockMode::Try).unwrap();
        let elapsed = start.elapsed();

        fs2::FileExt::unlock(&held_doc_lock).unwrap();
        assert!(
            matches!(acquired, StartupLockAcquire::Busy),
            "try-mode startup locks should report a busy lock instead of waiting"
        );
        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "try-mode startup lock acquisition should be bounded, elapsed={elapsed:?}"
        );
    }
}
