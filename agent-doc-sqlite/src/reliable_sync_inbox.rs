//! Controller-side durable receipt journal for reliable-sync liveness.
//!
//! The sender outbox may prune a frame only after the controller acknowledges it.
//! Consequently the receiver must make the liveness projection recoverable before
//! returning that acknowledgement. This module owns that SQLite commit boundary.

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result, anyhow};
use rusqlite::{Connection, Transaction, params};

const BUSY_TIMEOUT: Duration = Duration::from_secs(30);

const INBOX_SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS reliable_sync_inbox_cursor (
    document_hash TEXT PRIMARY KEY,
    ack_through INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS reliable_sync_liveness_journal (
    source_key TEXT NOT NULL,
    epoch INTEGER NOT NULL,
    ops_json TEXT NOT NULL,
    PRIMARY KEY (source_key, epoch)
);
"#;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReliableSyncCursor {
    pub document_hash: String,
    pub ack_through: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReliableSyncLivenessRecord {
    pub source_key: String,
    pub epoch: u64,
    pub ops_json: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReliableSyncInboxSnapshot {
    pub cursors: Vec<ReliableSyncCursor>,
    pub liveness: Vec<ReliableSyncLivenessRecord>,
}

fn open(path: &Path) -> Result<Connection> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| {
            format!("create reliable-sync inbox directory {}", parent.display())
        })?;
    }
    let connection = Connection::open(path)
        .with_context(|| format!("open reliable-sync inbox {}", path.display()))?;
    connection.busy_timeout(BUSY_TIMEOUT)?;
    connection.execute_batch(INBOX_SCHEMA)?;
    Ok(connection)
}

fn sqlite_epoch(epoch: u64) -> Result<i64> {
    i64::try_from(epoch).map_err(|_| anyhow!("reliable-sync epoch {epoch} exceeds SQLite INTEGER"))
}

fn rust_epoch(epoch: i64) -> Result<u64> {
    u64::try_from(epoch).map_err(|_| anyhow!("negative reliable-sync epoch {epoch} in SQLite"))
}

fn insert_liveness_record(
    tx: &Transaction<'_>,
    source_key: &str,
    epoch: i64,
    ops_json: &str,
) -> Result<()> {
    tx.execute(
        "INSERT OR IGNORE INTO reliable_sync_liveness_journal \
         (source_key, epoch, ops_json) VALUES (?1, ?2, ?3)",
        params![source_key, epoch, ops_json],
    )?;
    let stored: String = tx.query_row(
        "SELECT ops_json FROM reliable_sync_liveness_journal \
         WHERE source_key = ?1 AND epoch = ?2",
        params![source_key, epoch],
        |row| row.get(0),
    )?;
    if stored != ops_json {
        return Err(anyhow!(
            "reliable-sync epoch reuse for source {source_key} at {epoch} carried different liveness ops"
        ));
    }
    Ok(())
}

/// Atomically durably record an inbound frame and advance its receive cursor.
///
/// `liveness_ops_json` is present only for liveness frames. The cursor is still
/// persisted for document-op and other reliable-sync frames, but those payloads
/// have their own canonical durability path and do not enter this journal.
pub fn record_remote_frame(
    path: &Path,
    document_hash: &str,
    epoch: u64,
    liveness_ops_json: Option<&str>,
) -> Result<u64> {
    let mut connection = open(path)?;
    let tx = connection.transaction()?;
    let epoch_i64 = sqlite_epoch(epoch)?;
    // Keep the max join inside the UPSERT. A read-then-write max can still be
    // computed from a stale snapshot when two controller handles race; the SQL
    // expression evaluates against the row that actually wins the write lock.
    let ack_through_i64 = tx.query_row(
        "INSERT INTO reliable_sync_inbox_cursor (document_hash, ack_through) \
         VALUES (?1, ?2) \
         ON CONFLICT(document_hash) DO UPDATE SET \
             ack_through = MAX(reliable_sync_inbox_cursor.ack_through, excluded.ack_through) \
         RETURNING ack_through",
        params![document_hash, epoch_i64],
        |row| row.get::<_, i64>(0),
    )?;
    let ack_through = rust_epoch(ack_through_i64)?;
    if let Some(ops_json) = liveness_ops_json {
        insert_liveness_record(&tx, document_hash, epoch_i64, ops_json)?;
    }
    tx.commit()?;
    Ok(ack_through)
}

/// Durably record a controller-originated liveness batch (for example an
/// OS-observed editor-process exit). There is no remote cursor to acknowledge.
pub fn record_local_liveness(
    path: &Path,
    source_key: &str,
    epoch: u64,
    ops_json: &str,
) -> Result<()> {
    let mut connection = open(path)?;
    let tx = connection.transaction()?;
    insert_liveness_record(&tx, source_key, sqlite_epoch(epoch)?, ops_json)?;
    tx.commit()?;
    Ok(())
}

/// Load the receiver cursors and every liveness fact needed to rebuild the
/// controller projection after a process recycle.
pub fn load(path: &Path) -> Result<ReliableSyncInboxSnapshot> {
    if !path.exists() {
        return Ok(ReliableSyncInboxSnapshot::default());
    }
    let connection = open(path)?;
    let cursors = {
        let mut statement = connection.prepare(
            "SELECT document_hash, ack_through FROM reliable_sync_inbox_cursor \
             ORDER BY document_hash",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        rows.map(|row| {
            let (document_hash, ack_through) = row?;
            Ok(ReliableSyncCursor {
                document_hash,
                ack_through: rust_epoch(ack_through)?,
            })
        })
        .collect::<Result<Vec<_>>>()?
    };
    let liveness = {
        let mut statement = connection.prepare(
            "SELECT source_key, epoch, ops_json FROM reliable_sync_liveness_journal \
             ORDER BY rowid",
        )?;
        let rows = statement.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, i64>(1)?,
                row.get::<_, String>(2)?,
            ))
        })?;
        rows.map(|row| {
            let (source_key, epoch, ops_json) = row?;
            Ok(ReliableSyncLivenessRecord {
                source_key,
                epoch: rust_epoch(epoch)?,
                ops_json,
            })
        })
        .collect::<Result<Vec<_>>>()?
    };
    Ok(ReliableSyncInboxSnapshot { cursors, liveness })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_cursor_is_monotone_across_stale_writers_and_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reliable-sync.db");

        assert_eq!(record_remote_frame(&path, "doc", 9, None).unwrap(), 9);
        assert_eq!(record_remote_frame(&path, "doc", 3, None).unwrap(), 9);
        assert_eq!(record_remote_frame(&path, "doc", 12, None).unwrap(), 12);

        let snapshot = load(&path).unwrap();
        assert_eq!(
            snapshot.cursors,
            vec![ReliableSyncCursor {
                document_hash: "doc".into(),
                ack_through: 12,
            }]
        );
    }

    #[test]
    fn acknowledged_liveness_survives_reopen_and_duplicate_delivery() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reliable-sync.db");
        let ops = r#"[{"Open":{"document_hash":"doc","pid":7,"tag":"t1"}}]"#;

        assert_eq!(record_remote_frame(&path, "doc", 1, Some(ops)).unwrap(), 1);
        assert_eq!(record_remote_frame(&path, "doc", 1, Some(ops)).unwrap(), 1);

        let snapshot = load(&path).unwrap();
        assert_eq!(snapshot.liveness.len(), 1);
        assert_eq!(snapshot.liveness[0].ops_json, ops);
    }

    #[test]
    fn epoch_reuse_with_a_different_payload_is_rejected_without_cursor_advance() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("reliable-sync.db");
        record_remote_frame(&path, "doc", 2, Some("[]")).unwrap();

        let error = record_remote_frame(&path, "doc", 2, Some("[1]"))
            .expect_err("different payload at the same epoch must fail");
        assert!(error.to_string().contains("epoch reuse"));
        assert_eq!(load(&path).unwrap().cursors[0].ack_through, 2);
    }
}
