//! `#unsavedcanonicalcarry`: the canonical text a controller holds ahead of disk.
//!
//! The in-memory retained projection dies with the process. A replacement that
//! registers with no editor frontier used to seed [`RelayHub::from_text`] from
//! disk and then re-project queue attempts, which drops that cut. On 2026-11-05
//! the predecessor's canonical was 383 bytes ahead of disk, no native save
//! receipt had landed, and the successor's 12-byte state vector was a fresh
//! single-peer rebuild of the disk text. Those 383 bytes are not in any
//! surviving artifact, so this record exists so the next replacement can carry
//! the cut instead of reconstructing it.
//!
//! The record is the text and the hash of the disk baseline it diverged from,
//! not a CRDT history. It answers only while disk is still that baseline. A
//! moved disk is left alone, and the record stays for inspection. A failure to
//! read or write one never fails the document edit it describes.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

use agent_doc_document_realtime::crdt_relay::RetainedCanonicalProjection;

/// Project-relative directory of unsaved-canonical records.
const UNSAVED_CANONICAL_DIR: &str = ".agent-doc/state/unsaved-canonical";

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
struct Record {
    content_hash: String,
    /// Hash of the committed disk baseline this cut diverged from.
    disk_hash: String,
    #[serde(default)]
    component_structure_fenced: bool,
    content: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Remembered {
    Absent,
    /// `content_hash` and `disk_hash` last written, plus the fence flag.
    Persisted(String),
    /// The file holds a different cut than the durable canonical. Leave it.
    Foreign,
}

/// What a cold seed with no editor frontier should start from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ColdSeed {
    Disk,
    Carried {
        content: String,
        component_structure_fenced: bool,
    },
}

fn cache() -> &'static parking_lot::Mutex<std::collections::HashMap<String, Remembered>> {
    static CACHE: std::sync::OnceLock<
        parking_lot::Mutex<std::collections::HashMap<String, Remembered>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(|| parking_lot::Mutex::new(std::collections::HashMap::new()))
}

fn remembered_key(content_hash: &str, disk_hash: &str, fenced: bool) -> String {
    format!("{content_hash}\n{disk_hash}\n{fenced}")
}

fn record_path(file: &Path) -> Option<PathBuf> {
    let project_root = agent_doc_project_root_io::project_root_containing(file)?;
    let hash = agent_doc_fs::document_state_hash(file).ok()?;
    Some(
        project_root
            .join(UNSAVED_CANONICAL_DIR)
            .join(format!("{hash}.json")),
    )
}

fn document_key(file: &Path) -> Option<String> {
    agent_doc_fs::document_state_hash(file).ok()
}

fn read_record(path: &Path) -> Result<Option<Record>> {
    let bytes = match std::fs::read(path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read unsaved canonical {}", path.display()));
        }
    };
    let record: Record = serde_json::from_slice(&bytes)
        .with_context(|| format!("malformed unsaved canonical {}", path.display()))?;
    anyhow::ensure!(
        agent_doc_hash::content_hash(&record.content) == record.content_hash,
        "unsaved canonical hash mismatch {}",
        path.display()
    );
    Ok(Some(record))
}

fn write_record(path: &Path, record: &Record) -> Result<()> {
    let payload = serde_json::to_vec(record).context("failed to encode unsaved canonical")?;
    agent_doc_fs::write_atomic(path, &payload)
        .with_context(|| format!("failed to record unsaved canonical {}", path.display()))
}

fn log_failure(file: &Path, action: &str, err: &anyhow::Error) {
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_unsaved_canonical_record_failed file={} action={action} error={err:#} (#unsavedcanonicalcarry)",
            file.display(),
        ),
    );
}

/// Remember or drop the record after a hub mutation.
///
/// A canonical that still matches its committed baseline is durable: a record of
/// that same text is removed. A canonical that has moved past the baseline is
/// written. Empty text is not a record (an empty cut is not authority); an
/// older record is removed so it cannot resurrect over the clear.
pub(crate) fn sync(file: &Path, projection: &RetainedCanonicalProjection) {
    if projection.retained_replica_reseed_pending {
        return;
    }
    let Some(committed) = projection.last_committed_text.as_deref() else {
        return;
    };
    if projection.current_text == committed {
        note_durable(file, &projection.current_text);
        return;
    }
    if projection.current_text.is_empty() {
        forget(file);
        return;
    }
    let disk_hash = agent_doc_hash::content_hash(committed);
    let content_hash = agent_doc_hash::content_hash(&projection.current_text);
    persist(
        file,
        &projection.current_text,
        &content_hash,
        &disk_hash,
        projection.component_structure_fenced,
    );
}

fn persist(file: &Path, content: &str, content_hash: &str, disk_hash: &str, fenced: bool) {
    let Some(key) = document_key(file) else {
        return;
    };
    let token = remembered_key(content_hash, disk_hash, fenced);
    let already_persisted = matches!(
        cache().lock().get(&key),
        Some(Remembered::Persisted(existing)) if existing == &token
    );
    if already_persisted {
        return;
    }
    let Some(path) = record_path(file) else {
        return;
    };
    let record = Record {
        content_hash: content_hash.to_string(),
        disk_hash: disk_hash.to_string(),
        component_structure_fenced: fenced,
        content: content.to_string(),
    };
    match write_record(&path, &record) {
        Ok(()) => {
            cache().lock().insert(key, Remembered::Persisted(token));
        }
        Err(err) => log_failure(file, "persist", &err),
    }
}

fn note_durable(file: &Path, current: &str) {
    let Some(key) = document_key(file) else {
        return;
    };
    let known = cache().lock().get(&key).cloned();
    match known {
        Some(Remembered::Absent | Remembered::Foreign) => {}
        Some(Remembered::Persisted(token)) => {
            let current_hash = agent_doc_hash::content_hash(current);
            let persisted_hash = token.split('\n').next().unwrap_or("");
            if persisted_hash == current_hash {
                delete_record(file, &key);
            } else {
                cache().lock().insert(key, Remembered::Foreign);
            }
        }
        None => match load_for_sync(file) {
            Ok(None) => {
                cache().lock().insert(key, Remembered::Absent);
            }
            Ok(Some(record)) if record.content == current => delete_record(file, &key),
            Ok(Some(_)) => {
                cache().lock().insert(key, Remembered::Foreign);
            }
            Err(err) => log_failure(file, "read", &err),
        },
    }
}

fn load_for_sync(file: &Path) -> Result<Option<Record>> {
    let Some(path) = record_path(file) else {
        return Ok(None);
    };
    read_record(&path)
}

fn delete_record(file: &Path, key: &str) {
    let Some(path) = record_path(file) else {
        return;
    };
    match std::fs::remove_file(&path) {
        Ok(()) => {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "crdt_unsaved_canonical_durable file={} action=clear (#unsavedcanonicalcarry)",
                    file.display(),
                ),
            );
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            log_failure(
                file,
                "clear",
                &anyhow::Error::from(err).context(format!("failed to clear {}", path.display())),
            );
            return;
        }
    }
    cache().lock().insert(key.to_string(), Remembered::Absent);
}

fn forget(file: &Path) {
    let Some(key) = document_key(file) else {
        return;
    };
    if cache().lock().get(&key) == Some(&Remembered::Absent) {
        return;
    }
    delete_record(file, &key);
}

/// Choose the cold-seed text. Disk that still matches the recorded baseline
/// yields the carried cut. Disk that already equals it, or that has moved, does
/// not.
pub(crate) fn cold_seed(file: &Path, disk: &str) -> ColdSeed {
    let Some(path) = record_path(file) else {
        return ColdSeed::Disk;
    };
    let record = match read_record(&path) {
        Ok(Some(record)) => record,
        Ok(None) => return ColdSeed::Disk,
        Err(err) => {
            log_failure(file, "seed_read", &err);
            return ColdSeed::Disk;
        }
    };
    let disk_hash = agent_doc_hash::content_hash(disk);
    if record.content == disk {
        if let Some(key) = document_key(file) {
            delete_record(file, &key);
        }
        return ColdSeed::Disk;
    }
    if record.disk_hash == disk_hash {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "crdt_unsaved_canonical_carried file={} content_hash={} content_len={} disk_hash={} component_structure_fenced={} action=seed_carried_canonical (#unsavedcanonicalcarry)",
                file.display(),
                agent_doc_hash::short_content_hash(&record.content),
                record.content.len(),
                agent_doc_hash::short_content_hash(disk),
                record.component_structure_fenced,
            ),
        );
        return ColdSeed::Carried {
            content: record.content,
            component_structure_fenced: record.component_structure_fenced,
        };
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "crdt_unsaved_canonical_disk_moved file={} content_hash={} content_len={} recorded_disk_hash={} observed_disk_hash={} action=seed_disk reason=disk_baseline_changed (#unsavedcanonicalcarry)",
            file.display(),
            agent_doc_hash::short_content_hash(&record.content),
            record.content.len(),
            &record.disk_hash[..record.disk_hash.len().min(12)],
            agent_doc_hash::short_content_hash(disk),
        ),
    );
    ColdSeed::Disk
}

#[cfg(test)]
pub(crate) fn record_exists_for_test(file: &Path) -> bool {
    record_path(file).is_some_and(|path| path.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "disk\n").unwrap();
        (dir, file)
    }

    #[test]
    fn a_record_round_trips_and_carries_only_while_disk_is_the_baseline() {
        let (_dir, file) = project();
        let disk = std::fs::read_to_string(&file).unwrap();
        assert!(matches!(super::cold_seed(&file, &disk), ColdSeed::Disk));
        super::persist(
            &file,
            "disk\nunsaved\n",
            &agent_doc_hash::content_hash("disk\nunsaved\n"),
            &agent_doc_hash::content_hash(&disk),
            false,
        );
        match super::cold_seed(&file, &disk) {
            ColdSeed::Carried {
                content,
                component_structure_fenced,
            } => {
                assert_eq!(content, "disk\nunsaved\n");
                assert!(!component_structure_fenced);
            }
            ColdSeed::Disk => panic!("the unsaved cut must seed ahead of unchanged disk"),
        }
        assert!(matches!(
            super::cold_seed(&file, "disk\nunsaved\n"),
            ColdSeed::Disk
        ));
        assert!(!super::record_exists_for_test(&file));
    }

    #[test]
    fn a_moved_disk_is_not_replaced_and_the_record_stays() {
        let (_dir, file) = project();
        let disk = std::fs::read_to_string(&file).unwrap();
        super::persist(
            &file,
            "disk\nunsaved\n",
            &agent_doc_hash::content_hash("disk\nunsaved\n"),
            &agent_doc_hash::content_hash(&disk),
            false,
        );
        assert!(matches!(
            super::cold_seed(&file, "other disk\n"),
            ColdSeed::Disk
        ));
        assert!(super::record_exists_for_test(&file));
    }

    #[test]
    fn a_hash_mismatch_does_not_seed_the_cut() {
        let (_dir, file) = project();
        let path = super::record_path(&file).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(
            &path,
            br#"{"content_hash":"nope","disk_hash":"x","component_structure_fenced":false,"content":"disk\nunsaved\n"}"#,
        )
        .unwrap();
        assert!(matches!(super::cold_seed(&file, "disk\n"), ColdSeed::Disk));
        assert!(path.exists(), "a corrupt record is left in place");
    }
}
