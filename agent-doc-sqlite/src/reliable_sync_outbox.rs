//! SQLite-backed [`DurableOutbox`] for the reliable-sync plane (`#lzsync`,
//! sidecar-retirement Phase 3C).
//!
//! Implements lazily's `DurableOutbox` contract against a SQLite table so the
//! plugin→controller open-set / owner-lease push survives a controller recycle:
//! every frame is appended **before** it is sent, retained until the peer proves
//! receipt (`ack_through`), and replayed from the resume cursor on reconnect
//! (`replay_from`). Combined with the receiver's idempotent apply, this is the
//! at-least-once delivery with exactly-once effect that lets a push lost while
//! the controller was down be re-sent from the frontier.
//!
//! This is the **durable projection** that replaces the on-disk sidecars'
//! recycle-survival (plan invariant: *"Open-set + owner-lease survive a recycle
//! without the editor re-announcing"*). The `acked_through` cursor is persisted,
//! so a fresh process reconstructs exactly the un-acked suffix the sidecars used
//! to carry on disk.
//!
//! One [`SqliteOutbox`] is scoped to a single `document_hash` channel; rows for
//! every channel share the `reliable_sync_outbox` table keyed by
//! `(document_hash, epoch)`, and the per-channel ack cursor lives in
//! `reliable_sync_outbox_cursor`. The schema is created on construction
//! (`CREATE TABLE IF NOT EXISTS`), so the module is self-contained and does not
//! depend on the shared `state_store` schema init.

use lazily::{DurableOutbox, IpcMessage};
use rusqlite::{Connection, params};
use std::path::Path;

/// DDL for the two outbox tables. Idempotent; run once per connection at
/// construction. Kept here (not in `state_store::initialize_state_db`) so the
/// outbox owns its own storage and is testable against a throwaway database.
const OUTBOX_SCHEMA: &str = r#"
    CREATE TABLE IF NOT EXISTS reliable_sync_outbox (
        document_hash TEXT NOT NULL,
        epoch INTEGER NOT NULL,
        frame_json TEXT NOT NULL,
        PRIMARY KEY (document_hash, epoch)
    );

    CREATE TABLE IF NOT EXISTS reliable_sync_outbox_cursor (
        document_hash TEXT PRIMARY KEY,
        acked_through INTEGER NOT NULL DEFAULT 0
    );
"#;

/// Ensure the reliable-sync outbox tables exist on `conn` (idempotent).
pub fn ensure_outbox_schema(conn: &Connection) -> anyhow::Result<()> {
    conn.execute_batch(OUTBOX_SCHEMA)?;
    Ok(())
}

/// A per-`document_hash` [`DurableOutbox`] backed by SQLite.
///
/// Owns its own [`Connection`] (single-threaded, driven by one `SyncDriver`).
/// The trait methods are infallible by contract, so SQLite/serde failures are
/// logged loudly to stderr (never silently swallowed) and the in-memory ack
/// cursor is kept consistent with the last durable write.
pub struct SqliteOutbox {
    conn: Connection,
    document_hash: String,
    /// Cached mirror of the durable `acked_through` cursor (source of truth is
    /// the `reliable_sync_outbox_cursor` row).
    acked_through: u64,
}

impl SqliteOutbox {
    /// Open a per-document outbox at `path`, ensuring the schema and loading the
    /// durable ack cursor (the recycle-recovery entry point).
    pub fn open(path: &Path, document_hash: impl Into<String>) -> anyhow::Result<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let conn = Connection::open(path)?;
        Self::with_connection(conn, document_hash)
    }

    /// Wrap an existing connection (e.g. the shared controller state DB). Ensures
    /// the schema and loads the durable ack cursor.
    pub fn with_connection(
        conn: Connection,
        document_hash: impl Into<String>,
    ) -> anyhow::Result<Self> {
        ensure_outbox_schema(&conn)?;
        let document_hash = document_hash.into();
        let acked_through = load_cursor(&conn, &document_hash)?;
        Ok(Self {
            conn,
            document_hash,
            acked_through,
        })
    }

    /// The highest epoch the peer has acked (the durable retention cursor).
    pub fn acked_through(&self) -> u64 {
        self.acked_through
    }

    /// The channel this outbox is scoped to.
    pub fn document_hash(&self) -> &str {
        &self.document_hash
    }

    /// Persist the ack cursor row (`INSERT OR REPLACE`), returning the SQLite result.
    fn persist_cursor(&self) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR REPLACE INTO reliable_sync_outbox_cursor (document_hash, acked_through) \
             VALUES (?1, ?2)",
            params![self.document_hash, self.acked_through as i64],
        )?;
        Ok(())
    }
}

/// Read the durable ack cursor for `document_hash`, defaulting to 0 when absent.
fn load_cursor(conn: &Connection, document_hash: &str) -> anyhow::Result<u64> {
    let cursor: Option<i64> = conn
        .query_row(
            "SELECT acked_through FROM reliable_sync_outbox_cursor WHERE document_hash = ?1",
            params![document_hash],
            |row| row.get(0),
        )
        .ok();
    Ok(cursor.unwrap_or(0).max(0) as u64)
}

impl DurableOutbox for SqliteOutbox {
    fn append(&mut self, epoch: u64, msg: IpcMessage) {
        let frame_json = match serde_json::to_string(&msg) {
            Ok(json) => json,
            Err(e) => {
                eprintln!(
                    "[reliable-sync-outbox] {}: failed to serialize frame at epoch {epoch}: {e}",
                    self.document_hash
                );
                return;
            }
        };
        if let Err(e) = self.conn.execute(
            "INSERT OR REPLACE INTO reliable_sync_outbox (document_hash, epoch, frame_json) \
             VALUES (?1, ?2, ?3)",
            params![self.document_hash, epoch as i64, frame_json],
        ) {
            eprintln!(
                "[reliable-sync-outbox] {}: failed to append frame at epoch {epoch}: {e}",
                self.document_hash
            );
        }
    }

    fn ack_through(&mut self, epoch: u64) {
        if epoch > self.acked_through {
            self.acked_through = epoch;
        }
        if let Err(e) = self.persist_cursor() {
            eprintln!(
                "[reliable-sync-outbox] {}: failed to persist ack cursor {}: {e}",
                self.document_hash, self.acked_through
            );
        }
        // Prune every frame the peer has proven receipt of (epoch <= cursor).
        if let Err(e) = self.conn.execute(
            "DELETE FROM reliable_sync_outbox WHERE document_hash = ?1 AND epoch <= ?2",
            params![self.document_hash, self.acked_through as i64],
        ) {
            eprintln!(
                "[reliable-sync-outbox] {}: failed to prune acked frames <= {}: {e}",
                self.document_hash, self.acked_through
            );
        }
    }

    fn replay_from(&self, cursor: u64) -> Vec<(u64, IpcMessage)> {
        let mut stmt = match self.conn.prepare(
            "SELECT epoch, frame_json FROM reliable_sync_outbox \
             WHERE document_hash = ?1 AND epoch > ?2 ORDER BY epoch ASC",
        ) {
            Ok(stmt) => stmt,
            Err(e) => {
                eprintln!(
                    "[reliable-sync-outbox] {}: failed to prepare replay query: {e}",
                    self.document_hash
                );
                return Vec::new();
            }
        };
        let rows = stmt.query_map(params![self.document_hash, cursor as i64], |row| {
            let epoch: i64 = row.get(0)?;
            let frame_json: String = row.get(1)?;
            Ok((epoch as u64, frame_json))
        });
        let rows = match rows {
            Ok(rows) => rows,
            Err(e) => {
                eprintln!(
                    "[reliable-sync-outbox] {}: failed to run replay query: {e}",
                    self.document_hash
                );
                return Vec::new();
            }
        };
        let mut out = Vec::new();
        for row in rows {
            match row {
                Ok((epoch, frame_json)) => match serde_json::from_str::<IpcMessage>(&frame_json) {
                    Ok(msg) => out.push((epoch, msg)),
                    Err(e) => eprintln!(
                        "[reliable-sync-outbox] {}: failed to deserialize replay frame at epoch {epoch}: {e}",
                        self.document_hash
                    ),
                },
                Err(e) => eprintln!(
                    "[reliable-sync-outbox] {}: failed to read replay row: {e}",
                    self.document_hash
                ),
            }
        }
        out
    }

    fn retained_epochs(&self) -> Vec<u64> {
        let mut stmt = match self.conn.prepare(
            "SELECT epoch FROM reliable_sync_outbox WHERE document_hash = ?1 ORDER BY epoch ASC",
        ) {
            Ok(stmt) => stmt,
            Err(e) => {
                eprintln!(
                    "[reliable-sync-outbox] {}: failed to prepare retained query: {e}",
                    self.document_hash
                );
                return Vec::new();
            }
        };
        let rows = stmt.query_map(params![self.document_hash], |row| {
            let epoch: i64 = row.get(0)?;
            Ok(epoch as u64)
        });
        match rows {
            Ok(rows) => rows.filter_map(|r| r.ok()).collect(),
            Err(e) => {
                eprintln!(
                    "[reliable-sync-outbox] {}: failed to run retained query: {e}",
                    self.document_hash
                );
                Vec::new()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lazily::{Delta, OutboxAck};

    fn delta(base: u64, epoch: u64) -> IpcMessage {
        IpcMessage::Delta(Delta::new(base, epoch, vec![]))
    }

    fn outbox(dir: &Path, hash: &str) -> SqliteOutbox {
        SqliteOutbox::open(&dir.join("state.db"), hash).unwrap()
    }

    #[test]
    fn append_retains_until_acked_then_prunes() {
        let tmp = tempfile::tempdir().unwrap();
        let mut o = outbox(tmp.path(), "docA");
        o.append(1, delta(0, 1));
        o.append(2, delta(1, 2));
        o.append(3, delta(2, 3));
        assert_eq!(o.retained_epochs(), vec![1, 2, 3]);

        // Peer proves receipt through 2 → 1,2 pruned; 3 retained.
        o.ack_through(2);
        assert_eq!(o.acked_through(), 2);
        assert_eq!(o.retained_epochs(), vec![3]);
    }

    #[test]
    fn replay_from_cursor_returns_ascending_suffix() {
        let tmp = tempfile::tempdir().unwrap();
        let mut o = outbox(tmp.path(), "docA");
        for e in 1..=4 {
            o.append(e, delta(e - 1, e));
        }
        let replay: Vec<u64> = o.replay_from(2).into_iter().map(|(e, _)| e).collect();
        assert_eq!(replay, vec![3, 4]);
        // The frames round-trip back to the exact IpcMessage.
        let (_, msg) = &o.replay_from(2)[0];
        assert_eq!(msg, &delta(2, 3));
    }

    #[test]
    fn ack_through_is_monotonic() {
        let tmp = tempfile::tempdir().unwrap();
        let mut o = outbox(tmp.path(), "docA");
        o.append(1, delta(0, 1));
        o.append(2, delta(1, 2));
        o.ack_through(2);
        // A stale lower ack does not rewind the cursor or resurrect pruned frames.
        o.ack_through(1);
        assert_eq!(o.acked_through(), 2);
        assert_eq!(o.retained_epochs(), Vec::<u64>::new());
    }

    #[test]
    fn survives_reopen_crash_replay() {
        // The recycle-survival invariant: reconstruct the outbox from disk and the
        // un-acked suffix + durable ack cursor are exactly as they were.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.db");
        {
            let mut o = SqliteOutbox::open(&path, "docA").unwrap();
            o.append(1, delta(0, 1));
            o.append(2, delta(1, 2));
            o.append(3, delta(2, 3));
            o.ack_through(1); // 1 pruned; cursor persisted at 1
        } // drop: simulate a controller recycle

        let reopened = SqliteOutbox::open(&path, "docA").unwrap();
        assert_eq!(
            reopened.acked_through(),
            1,
            "ack cursor survived the recycle"
        );
        assert_eq!(
            reopened.retained_epochs(),
            vec![2, 3],
            "the un-acked suffix survived the recycle"
        );
        let replay: Vec<u64> = reopened
            .replay_from(1)
            .into_iter()
            .map(|(e, _)| e)
            .collect();
        assert_eq!(replay, vec![2, 3], "reconnect replays the durable suffix");
    }

    #[test]
    fn channels_are_isolated_by_document_hash() {
        // Per-doc isolation: one channel's frames never leak into another's.
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("state.db");
        let mut a = SqliteOutbox::open(&path, "docA").unwrap();
        let mut b = SqliteOutbox::open(&path, "docB").unwrap();
        a.append(1, delta(0, 1));
        a.append(2, delta(1, 2));
        b.append(1, delta(0, 1));
        assert_eq!(a.retained_epochs(), vec![1, 2]);
        assert_eq!(b.retained_epochs(), vec![1]);
        // Acking A does not touch B.
        a.ack_through(2);
        assert_eq!(a.retained_epochs(), Vec::<u64>::new());
        assert_eq!(b.retained_epochs(), vec![1]);
    }

    #[test]
    fn stores_control_frames_too() {
        // The outbox is frame-agnostic — control frames round-trip like data frames.
        let tmp = tempfile::tempdir().unwrap();
        let mut o = outbox(tmp.path(), "docA");
        o.append(5, IpcMessage::OutboxAck(OutboxAck { through_epoch: 5 }));
        let (_, msg) = &o.replay_from(0)[0];
        assert_eq!(msg, &IpcMessage::OutboxAck(OutboxAck { through_epoch: 5 }));
    }
}
