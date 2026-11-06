//! `#attachauthorattribution`: the durable copy of a document's binary-authored
//! canonical write run ([`BinaryWriteAttribution`]).
//!
//! The hub's in-memory run dies with its controller process, and the writes
//! that matter most outlive it: on 2026-11-05 the canonical an attach refused
//! belonged to a replacement controller, and the writes behind it may have been
//! its predecessor's. The record is therefore a small JSON file under
//! [`BINARY_WRITE_ATTRIBUTION_DIR`] keyed by the document state hash, rewritten
//! with each recorded write. It holds content hashes and `source` labels, never
//! document text.
//!
//! The record is content-addressed, so no process identity is involved: it only
//! ever answers for a canonical whose hash equals its tip. A replacement
//! controller whose canonical is any other text gets nothing from it, and its
//! next binary write starts a fresh run. A missing, unreadable, or malformed
//! record is unattributed (fail closed), and a failure to read or write one
//! never fails the document write it describes.

use agent_doc_document_realtime::crdt_relay::{BinaryWriteAttribution, RelayHub};
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};

/// Project-relative directory of the attribution records.
pub const BINARY_WRITE_ATTRIBUTION_DIR: &str = ".agent-doc/state/binary-write-attribution";

fn record_path(project_root: &Path, file: &Path) -> Result<PathBuf> {
    Ok(project_root
        .join(BINARY_WRITE_ATTRIBUTION_DIR)
        .join(format!("{}.json", agent_doc_fs::document_state_hash(file)?)))
}

/// Durably replace the record for `file` in `project_root`.
pub fn record(
    project_root: &Path,
    file: &Path,
    attribution: &BinaryWriteAttribution,
) -> Result<()> {
    let path = record_path(project_root, file)?;
    let payload =
        serde_json::to_vec(attribution).context("failed to encode binary write attribution")?;
    agent_doc_fs::write_atomic(&path, &payload)
        .with_context(|| format!("failed to record binary writes {}", path.display()))
}

/// Load the record for `file`. A missing record is `None`; an unreadable or
/// malformed one is an error the caller logs and treats as unattributed.
pub fn load(project_root: &Path, file: &Path) -> Result<Option<BinaryWriteAttribution>> {
    let path = record_path(project_root, file)?;
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(err)
                .with_context(|| format!("failed to read binary writes {}", path.display()));
        }
    };
    serde_json::from_slice(&bytes)
        .map(Some)
        .with_context(|| format!("malformed binary writes {}", path.display()))
}

/// Give a hub with no run of its own (a replacement controller) the durable one.
pub(crate) fn seed_hub(file: &Path, hub: &mut RelayHub) {
    if hub.binary_write_attribution().tip().is_some() {
        return;
    }
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return;
    };
    match load(&project_root, file) {
        Ok(Some(attribution)) => hub.seed_binary_write_attribution(attribution),
        Ok(None) => {}
        Err(err) => agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "binary_write_attribution_read_failed file={} error={err:#} action=unattributed (#attachauthorattribution)",
                file.display()
            ),
        ),
    }
}

/// Persist the hub's run. A failure is logged and leaves the document write
/// standing: losing the record only costs a later attach its proof.
pub(crate) fn persist_hub(file: &Path, hub: &RelayHub) {
    let Some(project_root) = agent_doc_project_root_io::project_root_containing(file) else {
        return;
    };
    if let Err(err) = record(&project_root, file, hub.binary_write_attribution()) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "binary_write_attribution_persist_failed file={} error={err:#} action=unattributed (#attachauthorattribution)",
                file.display()
            ),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let file = dir.path().join("plan.md");
        std::fs::write(&file, "plan\n").unwrap();
        (dir, file)
    }

    #[test]
    fn a_record_round_trips_and_holds_no_document_text() {
        let (dir, file) = project();
        assert_eq!(load(dir.path(), &file).unwrap(), None);
        let mut attribution = BinaryWriteAttribution::default();
        attribution.record("before", "after", "queue_claim_runtime_projection");
        record(dir.path(), &file, &attribution).unwrap();
        assert_eq!(load(dir.path(), &file).unwrap(), Some(attribution.clone()));
        assert_eq!(attribution.ancestors_of("after").len(), 1);
        let stored = std::fs::read_to_string(record_path(dir.path(), &file).unwrap()).unwrap();
        assert!(!stored.contains("plan"), "{stored}");
    }

    #[test]
    fn a_malformed_record_is_an_error_not_an_attribution() {
        let (dir, file) = project();
        let path = record_path(dir.path(), &file).unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, b"{not json").unwrap();
        assert!(load(dir.path(), &file).is_err());
    }
}
