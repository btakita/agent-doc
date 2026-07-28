//! Durable dynamic-context injection ledger.
//!
//! This module records which tsift pack chunks were expanded, referenced, or
//! suppressed for a document turn. SQLite owns durable replay memory; the live
//! prompt projection remains in `agent-doc-prompt-context` and is intentionally
//! not reconstructed here.

use std::collections::BTreeSet;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::state_store::{sqlite_i64, timestamp_secs};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextInjectionMode {
    Expanded,
    Referenced,
    SkippedDuplicate,
    StaleIgnored,
}

impl ContextInjectionMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Expanded => "expanded",
            Self::Referenced => "referenced",
            Self::SkippedDuplicate => "skipped_duplicate",
            Self::StaleIgnored => "stale_ignored",
        }
    }

    fn from_db(value: &str) -> Result<Self> {
        match value {
            "expanded" => Ok(Self::Expanded),
            "referenced" => Ok(Self::Referenced),
            "skipped_duplicate" => Ok(Self::SkippedDuplicate),
            "stale_ignored" => Ok(Self::StaleIgnored),
            other => bail!("unknown context injection mode `{other}`"),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextInjectionWrite {
    pub pack_id: String,
    pub chunk_id: String,
    pub content_hash: String,
    pub source_uri: String,
    pub range_start: Option<i64>,
    pub range_end: Option<i64>,
    pub injection_mode: ContextInjectionMode,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextManifestWrite {
    pub document_id: String,
    pub session_id: String,
    pub cycle_id: String,
    pub cycle_state: String,
    pub harness: String,
    pub prompt_fingerprint: String,
    pub pack_ids: Vec<String>,
    pub token_count: i64,
    pub injections: Vec<ContextInjectionWrite>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredContextManifest {
    pub document_id: String,
    pub session_id: String,
    pub cycle_id: String,
    pub harness: String,
    pub prompt_fingerprint: String,
    pub pack_ids: Vec<String>,
    pub chunk_ids: Vec<String>,
    pub token_count: i64,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredContextInjection {
    pub id: i64,
    pub document_id: String,
    pub session_id: String,
    pub cycle_id: String,
    pub harness: String,
    pub pack_id: String,
    pub chunk_id: String,
    pub content_hash: String,
    pub source_uri: String,
    pub range_start: Option<i64>,
    pub range_end: Option<i64>,
    pub injected_at: i64,
    pub injection_mode: ContextInjectionMode,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextLookupScope<'a> {
    Document {
        document_id: &'a str,
    },
    Session {
        document_id: &'a str,
        session_id: &'a str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextClearScope<'a> {
    Document {
        document_id: &'a str,
    },
    Session {
        document_id: &'a str,
        session_id: &'a str,
    },
    Cycle {
        document_id: &'a str,
        cycle_id: &'a str,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextDuplicateReason {
    ChunkId,
    ContentHash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextInjectionMatch {
    pub reason: ContextDuplicateReason,
    pub injection: StoredContextInjection,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ClearedContextRows {
    pub manifests: usize,
    pub injections: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextInjectionModeCount {
    pub injection_mode: ContextInjectionMode,
    pub count: usize,
}

/// Atomically records one context manifest, every chunk decision, and the
/// caller's current document-cycle state.
///
/// A retry with the exact same manifest is idempotent. Reusing a cycle id with
/// different manifest contents fails closed so a later retry cannot rewrite
/// the durable explanation of an earlier prompt.
pub fn record_context_manifest(
    conn: &mut Connection,
    manifest: &ContextManifestWrite,
) -> Result<()> {
    validate_manifest(manifest)?;
    let pack_ids_json =
        serde_json::to_string(&manifest.pack_ids).context("serialize context pack ids")?;
    let chunk_ids = manifest
        .injections
        .iter()
        .map(|injection| injection.chunk_id.clone())
        .collect::<Vec<_>>();
    let chunk_ids_json =
        serde_json::to_string(&chunk_ids).context("serialize context chunk ids")?;
    let now = sqlite_i64(timestamp_secs(), "context manifest timestamp")?;
    let tx = conn.transaction()?;

    tx.execute(
        r#"
        INSERT INTO document_cycles (
            document_id,
            cycle_id,
            state,
            updated_at
        )
        VALUES (?1, ?2, ?3, ?4)
        ON CONFLICT(document_id, cycle_id) DO UPDATE SET
            state = excluded.state,
            updated_at = excluded.updated_at
        "#,
        params![
            manifest.document_id,
            manifest.cycle_id,
            manifest.cycle_state,
            now
        ],
    )?;

    let manifest_preexisted = match context_manifest_for_cycle_in_tx(
        &tx,
        &manifest.document_id,
        &manifest.cycle_id,
    )? {
        Some(existing) => {
            let expected = StoredContextManifest {
                document_id: manifest.document_id.clone(),
                session_id: manifest.session_id.clone(),
                cycle_id: manifest.cycle_id.clone(),
                harness: manifest.harness.clone(),
                prompt_fingerprint: manifest.prompt_fingerprint.clone(),
                pack_ids: manifest.pack_ids.clone(),
                chunk_ids,
                token_count: manifest.token_count,
                created_at: existing.created_at,
            };
            if existing != expected {
                bail!(
                    "context manifest for document `{}` cycle `{}` already exists with different contents",
                    manifest.document_id,
                    manifest.cycle_id
                );
            }

            let existing_injections =
                context_injections_for_cycle(&tx, &manifest.document_id, &manifest.cycle_id)?;
            let injections_match = existing_injections.len() == manifest.injections.len()
                && existing_injections.iter().zip(&manifest.injections).all(
                    |(stored, requested)| {
                        stored.document_id == manifest.document_id
                            && stored.session_id == manifest.session_id
                            && stored.cycle_id == manifest.cycle_id
                            && stored.harness == manifest.harness
                            && stored.pack_id == requested.pack_id
                            && stored.chunk_id == requested.chunk_id
                            && stored.content_hash == requested.content_hash
                            && stored.source_uri == requested.source_uri
                            && stored.range_start == requested.range_start
                            && stored.range_end == requested.range_end
                            && stored.injection_mode == requested.injection_mode
                    },
                );
            if !injections_match {
                bail!(
                    "context injections for document `{}` cycle `{}` already exist with different contents",
                    manifest.document_id,
                    manifest.cycle_id
                );
            }
            true
        }
        None => {
            tx.execute(
                r#"
                INSERT INTO context_manifest (
                    document_id,
                    session_id,
                    cycle_id,
                    harness,
                    prompt_fingerprint,
                    pack_ids_json,
                    chunk_ids_json,
                    token_count,
                    created_at
                )
                VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                "#,
                params![
                    manifest.document_id,
                    manifest.session_id,
                    manifest.cycle_id,
                    manifest.harness,
                    manifest.prompt_fingerprint,
                    pack_ids_json,
                    chunk_ids_json,
                    manifest.token_count,
                    now
                ],
            )?;
            false
        }
    };

    if !manifest_preexisted {
        for injection in &manifest.injections {
            tx.execute(
                r#"
                    INSERT INTO context_injections (
                        document_id,
                        session_id,
                        cycle_id,
                        harness,
                        pack_id,
                        chunk_id,
                        content_hash,
                        source_uri,
                        range_start,
                        range_end,
                        injected_at,
                        injection_mode
                    )
                    VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)
                    "#,
                params![
                    manifest.document_id,
                    manifest.session_id,
                    manifest.cycle_id,
                    manifest.harness,
                    injection.pack_id,
                    injection.chunk_id,
                    injection.content_hash,
                    injection.source_uri,
                    injection.range_start,
                    injection.range_end,
                    now,
                    injection.injection_mode.as_str()
                ],
            )?;
        }
    }

    tx.commit()?;
    Ok(())
}

/// Finds prior context in exact-identity order: chunk id first, then matching
/// content from a differently named chunk or pack.
pub fn already_injected(
    conn: &Connection,
    scope: ContextLookupScope<'_>,
    chunk_id: &str,
    content_hash: &str,
) -> Result<Option<ContextInjectionMatch>> {
    if chunk_id.trim().is_empty() {
        bail!("context chunk id must not be empty");
    }
    if content_hash.trim().is_empty() {
        bail!("context content hash must not be empty");
    }

    if let Some(injection) = find_context_injection(conn, scope, "chunk_id", chunk_id)? {
        return Ok(Some(ContextInjectionMatch {
            reason: ContextDuplicateReason::ChunkId,
            injection,
        }));
    }
    if let Some(injection) = find_context_injection(conn, scope, "content_hash", content_hash)? {
        return Ok(Some(ContextInjectionMatch {
            reason: ContextDuplicateReason::ContentHash,
            injection,
        }));
    }
    Ok(None)
}

pub fn context_manifest_for_cycle(
    conn: &Connection,
    document_id: &str,
    cycle_id: &str,
) -> Result<Option<StoredContextManifest>> {
    context_manifest_for_cycle_in_tx(conn, document_id, cycle_id)
}

/// Returns the most recently recorded manifest in one document session.
///
/// Session-scoped lookup is the durable continuity boundary used by compact,
/// process restart, and transfer/extract projections. Explicit clear removes
/// these rows, so a subsequent cycle starts with a fresh injection scope.
pub fn latest_context_manifest_for_session(
    conn: &Connection,
    document_id: &str,
    session_id: &str,
) -> Result<Option<StoredContextManifest>> {
    let raw = conn
        .query_row(
            r#"
            SELECT
                document_id,
                session_id,
                cycle_id,
                harness,
                prompt_fingerprint,
                pack_ids_json,
                chunk_ids_json,
                token_count,
                created_at
            FROM context_manifest
            WHERE document_id = ?1 AND session_id = ?2
            ORDER BY created_at DESC, rowid DESC
            LIMIT 1
            "#,
            params![document_id, session_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?;
    stored_manifest_from_raw(raw)
}

pub fn context_injections_for_cycle(
    conn: &Connection,
    document_id: &str,
    cycle_id: &str,
) -> Result<Vec<StoredContextInjection>> {
    let mut statement = conn.prepare(
        r#"
        SELECT
            id,
            document_id,
            session_id,
            cycle_id,
            harness,
            pack_id,
            chunk_id,
            content_hash,
            source_uri,
            range_start,
            range_end,
            injected_at,
            injection_mode
        FROM context_injections
        WHERE document_id = ?1 AND cycle_id = ?2
        ORDER BY id
        "#,
    )?;
    let rows = statement
        .query_map(params![document_id, cycle_id], stored_injection_from_row)?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(rows)
}

pub fn context_injection_mode_counts_for_cycle(
    conn: &Connection,
    document_id: &str,
    cycle_id: &str,
) -> Result<Vec<ContextInjectionModeCount>> {
    let mut statement = conn.prepare(
        r#"
        SELECT injection_mode, COUNT(*)
        FROM context_injections
        WHERE document_id = ?1 AND cycle_id = ?2
        GROUP BY injection_mode
        ORDER BY injection_mode
        "#,
    )?;
    let raw = statement
        .query_map(params![document_id, cycle_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    raw.into_iter()
        .map(|(mode, count)| {
            Ok(ContextInjectionModeCount {
                injection_mode: ContextInjectionMode::from_db(&mode)?,
                count: usize::try_from(count).context("context injection count overflow")?,
            })
        })
        .collect()
}

/// Clears only context-ledger memory. Document cycle state remains owned by the
/// lifecycle state machine and is never deleted by a context boundary.
pub fn clear_context_scope(
    conn: &mut Connection,
    scope: ContextClearScope<'_>,
) -> Result<ClearedContextRows> {
    let tx = conn.transaction()?;
    let (injections, manifests) = match scope {
        ContextClearScope::Document { document_id } => (
            tx.execute(
                "DELETE FROM context_injections WHERE document_id = ?1",
                params![document_id],
            )?,
            tx.execute(
                "DELETE FROM context_manifest WHERE document_id = ?1",
                params![document_id],
            )?,
        ),
        ContextClearScope::Session {
            document_id,
            session_id,
        } => (
            tx.execute(
                "DELETE FROM context_injections WHERE document_id = ?1 AND session_id = ?2",
                params![document_id, session_id],
            )?,
            tx.execute(
                "DELETE FROM context_manifest WHERE document_id = ?1 AND session_id = ?2",
                params![document_id, session_id],
            )?,
        ),
        ContextClearScope::Cycle {
            document_id,
            cycle_id,
        } => (
            tx.execute(
                "DELETE FROM context_injections WHERE document_id = ?1 AND cycle_id = ?2",
                params![document_id, cycle_id],
            )?,
            tx.execute(
                "DELETE FROM context_manifest WHERE document_id = ?1 AND cycle_id = ?2",
                params![document_id, cycle_id],
            )?,
        ),
    };
    tx.commit()?;
    Ok(ClearedContextRows {
        manifests,
        injections,
    })
}

fn validate_manifest(manifest: &ContextManifestWrite) -> Result<()> {
    for (name, value) in [
        ("document id", manifest.document_id.as_str()),
        ("session id", manifest.session_id.as_str()),
        ("cycle id", manifest.cycle_id.as_str()),
        ("cycle state", manifest.cycle_state.as_str()),
        ("harness", manifest.harness.as_str()),
        ("prompt fingerprint", manifest.prompt_fingerprint.as_str()),
    ] {
        if value.trim().is_empty() {
            bail!("context {name} must not be empty");
        }
    }
    if manifest.token_count < 0 {
        bail!("context manifest token count must not be negative");
    }

    let mut pack_ids = BTreeSet::new();
    for pack_id in &manifest.pack_ids {
        if pack_id.trim().is_empty() {
            bail!("context pack id must not be empty");
        }
        if !pack_ids.insert(pack_id.as_str()) {
            bail!("context pack id `{pack_id}` is duplicated in the manifest");
        }
    }
    let mut injection_keys = BTreeSet::new();
    for injection in &manifest.injections {
        for (name, value) in [
            ("pack id", injection.pack_id.as_str()),
            ("chunk id", injection.chunk_id.as_str()),
            ("content hash", injection.content_hash.as_str()),
            ("source uri", injection.source_uri.as_str()),
        ] {
            if value.trim().is_empty() {
                bail!("context injection {name} must not be empty");
            }
        }
        if !pack_ids.contains(injection.pack_id.as_str()) {
            bail!(
                "context injection pack `{}` is missing from the manifest pack list",
                injection.pack_id
            );
        }
        if !injection_keys.insert((
            injection.pack_id.as_str(),
            injection.chunk_id.as_str(),
            injection.content_hash.as_str(),
            injection.injection_mode.as_str(),
        )) {
            bail!(
                "context injection `{}` from pack `{}` is duplicated in the manifest",
                injection.chunk_id,
                injection.pack_id
            );
        }
        if matches!(
            (injection.range_start, injection.range_end),
            (Some(start), Some(end)) if start > end
        ) {
            bail!(
                "context injection range start exceeds range end for chunk `{}`",
                injection.chunk_id
            );
        }
    }
    Ok(())
}

fn context_manifest_for_cycle_in_tx(
    conn: &Connection,
    document_id: &str,
    cycle_id: &str,
) -> Result<Option<StoredContextManifest>> {
    let raw = conn
        .query_row(
            r#"
            SELECT
                document_id,
                session_id,
                cycle_id,
                harness,
                prompt_fingerprint,
                pack_ids_json,
                chunk_ids_json,
                token_count,
                created_at
            FROM context_manifest
            WHERE document_id = ?1 AND cycle_id = ?2
            "#,
            params![document_id, cycle_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                    row.get::<_, String>(3)?,
                    row.get::<_, String>(4)?,
                    row.get::<_, String>(5)?,
                    row.get::<_, String>(6)?,
                    row.get::<_, i64>(7)?,
                    row.get::<_, i64>(8)?,
                ))
            },
        )
        .optional()?;
    stored_manifest_from_raw(raw)
}

#[allow(clippy::type_complexity)]
fn stored_manifest_from_raw(
    raw: Option<(
        String,
        String,
        String,
        String,
        String,
        String,
        String,
        i64,
        i64,
    )>,
) -> Result<Option<StoredContextManifest>> {
    raw.map(
        |(
            document_id,
            session_id,
            cycle_id,
            harness,
            prompt_fingerprint,
            pack_ids_json,
            chunk_ids_json,
            token_count,
            created_at,
        )| {
            Ok(StoredContextManifest {
                document_id,
                session_id,
                cycle_id,
                harness,
                prompt_fingerprint,
                pack_ids: serde_json::from_str(&pack_ids_json)
                    .context("decode context manifest pack ids")?,
                chunk_ids: serde_json::from_str(&chunk_ids_json)
                    .context("decode context manifest chunk ids")?,
                token_count,
                created_at,
            })
        },
    )
    .transpose()
}

fn find_context_injection(
    conn: &Connection,
    scope: ContextLookupScope<'_>,
    field: &str,
    value: &str,
) -> Result<Option<StoredContextInjection>> {
    let (sql, document_id, session_id) = match (scope, field) {
        (ContextLookupScope::Document { document_id }, "chunk_id") => (
            "SELECT id, document_id, session_id, cycle_id, harness, pack_id, chunk_id, \
             content_hash, source_uri, range_start, range_end, injected_at, injection_mode \
             FROM context_injections \
             WHERE document_id = ?1 AND chunk_id = ?2 ORDER BY id LIMIT 1",
            document_id,
            None,
        ),
        (ContextLookupScope::Document { document_id }, "content_hash") => (
            "SELECT id, document_id, session_id, cycle_id, harness, pack_id, chunk_id, \
             content_hash, source_uri, range_start, range_end, injected_at, injection_mode \
             FROM context_injections \
             WHERE document_id = ?1 AND content_hash = ?2 ORDER BY id LIMIT 1",
            document_id,
            None,
        ),
        (
            ContextLookupScope::Session {
                document_id,
                session_id,
            },
            "chunk_id",
        ) => (
            "SELECT id, document_id, session_id, cycle_id, harness, pack_id, chunk_id, \
             content_hash, source_uri, range_start, range_end, injected_at, injection_mode \
             FROM context_injections \
             WHERE document_id = ?1 AND session_id = ?2 AND chunk_id = ?3 \
             ORDER BY id LIMIT 1",
            document_id,
            Some(session_id),
        ),
        (
            ContextLookupScope::Session {
                document_id,
                session_id,
            },
            "content_hash",
        ) => (
            "SELECT id, document_id, session_id, cycle_id, harness, pack_id, chunk_id, \
             content_hash, source_uri, range_start, range_end, injected_at, injection_mode \
             FROM context_injections \
             WHERE document_id = ?1 AND session_id = ?2 AND content_hash = ?3 \
             ORDER BY id LIMIT 1",
            document_id,
            Some(session_id),
        ),
        (_, other) => bail!("unsupported context injection lookup field `{other}`"),
    };

    let stored = match session_id {
        Some(session_id) => conn
            .query_row(
                sql,
                params![document_id, session_id, value],
                stored_injection_from_row,
            )
            .optional()?,
        None => conn
            .query_row(sql, params![document_id, value], stored_injection_from_row)
            .optional()?,
    };
    Ok(stored)
}

fn stored_injection_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredContextInjection> {
    let mode = row.get::<_, String>(12)?;
    let injection_mode = ContextInjectionMode::from_db(&mode).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(12, rusqlite::types::Type::Text, error.into())
    })?;
    Ok(StoredContextInjection {
        id: row.get(0)?,
        document_id: row.get(1)?,
        session_id: row.get(2)?,
        cycle_id: row.get(3)?,
        harness: row.get(4)?,
        pack_id: row.get(5)?,
        chunk_id: row.get(6)?,
        content_hash: row.get(7)?,
        source_uri: row.get(8)?,
        range_start: row.get(9)?,
        range_end: row.get(10)?,
        injected_at: row.get(11)?,
        injection_mode,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_store::initialize_state_db;

    fn connection() -> Result<Connection> {
        let connection = Connection::open_in_memory()?;
        initialize_state_db(&connection)?;
        Ok(connection)
    }

    fn injection(pack_id: &str, chunk_id: &str, content_hash: &str) -> ContextInjectionWrite {
        ContextInjectionWrite {
            pack_id: pack_id.to_string(),
            chunk_id: chunk_id.to_string(),
            content_hash: content_hash.to_string(),
            source_uri: format!("src/{chunk_id}.md"),
            range_start: Some(10),
            range_end: Some(20),
            injection_mode: ContextInjectionMode::Expanded,
        }
    }

    fn manifest(
        session_id: &str,
        cycle_id: &str,
        prompt_fingerprint: &str,
        injections: Vec<ContextInjectionWrite>,
    ) -> ContextManifestWrite {
        let pack_ids = injections
            .iter()
            .map(|injection| injection.pack_id.clone())
            .collect::<BTreeSet<_>>()
            .into_iter()
            .collect();
        ContextManifestWrite {
            document_id: "doc-a".to_string(),
            session_id: session_id.to_string(),
            cycle_id: cycle_id.to_string(),
            cycle_state: "preflight_started".to_string(),
            harness: "codex".to_string(),
            prompt_fingerprint: prompt_fingerprint.to_string(),
            pack_ids,
            token_count: 42,
            injections,
        }
    }

    #[test]
    fn detects_same_chunk_id_and_same_content_from_another_pack() -> Result<()> {
        let mut connection = connection()?;
        record_context_manifest(
            &mut connection,
            &manifest(
                "session-a",
                "cycle-a",
                "prompt-a",
                vec![injection("pack-a", "chunk-a", "hash-a")],
            ),
        )?;
        let scope = ContextLookupScope::Session {
            document_id: "doc-a",
            session_id: "session-a",
        };

        let same_chunk = already_injected(&connection, scope, "chunk-a", "hash-new")?
            .context("same chunk should be found")?;
        assert_eq!(same_chunk.reason, ContextDuplicateReason::ChunkId);
        assert_eq!(same_chunk.injection.cycle_id, "cycle-a");

        let same_content = already_injected(&connection, scope, "chunk-b", "hash-a")?
            .context("same content should be found")?;
        assert_eq!(same_content.reason, ContextDuplicateReason::ContentHash);
        assert_eq!(same_content.injection.pack_id, "pack-a");

        assert!(already_injected(&connection, scope, "chunk-b", "hash-b")?.is_none());
        Ok(())
    }

    #[test]
    fn a_new_cycle_appends_without_mutating_prior_manifests() -> Result<()> {
        let mut connection = connection()?;
        let first = manifest(
            "session-a",
            "cycle-a",
            "prompt-a",
            vec![injection("pack-a", "chunk-a", "hash-a")],
        );
        let second = manifest(
            "session-a",
            "cycle-b",
            "prompt-b",
            vec![injection("pack-b", "chunk-b", "hash-b")],
        );
        record_context_manifest(&mut connection, &first)?;
        record_context_manifest(&mut connection, &second)?;
        record_context_manifest(&mut connection, &first)?;

        assert_eq!(
            context_manifest_for_cycle(&connection, "doc-a", "cycle-a")?
                .context("first manifest")?
                .prompt_fingerprint,
            "prompt-a"
        );
        assert_eq!(
            context_manifest_for_cycle(&connection, "doc-a", "cycle-b")?
                .context("second manifest")?
                .prompt_fingerprint,
            "prompt-b"
        );
        assert_eq!(
            context_injections_for_cycle(&connection, "doc-a", "cycle-a")?.len(),
            1
        );

        let mut conflicting = first;
        conflicting.prompt_fingerprint = "rewritten".to_string();
        assert!(record_context_manifest(&mut connection, &conflicting).is_err());
        assert_eq!(
            context_manifest_for_cycle(&connection, "doc-a", "cycle-a")?
                .context("preserved first manifest")?
                .prompt_fingerprint,
            "prompt-a"
        );

        let mut conflicting_injections = conflicting;
        conflicting_injections.prompt_fingerprint = "prompt-a".to_string();
        conflicting_injections.injections[0].content_hash = "rewritten-hash".to_string();
        assert!(record_context_manifest(&mut connection, &conflicting_injections).is_err());
        assert_eq!(
            context_injections_for_cycle(&connection, "doc-a", "cycle-a")?[0].content_hash,
            "hash-a"
        );
        Ok(())
    }

    #[test]
    fn latest_session_manifest_tracks_compaction_continuity_scope() -> Result<()> {
        let mut connection = connection()?;
        record_context_manifest(
            &mut connection,
            &manifest(
                "session-a",
                "cycle-a",
                "prompt-a",
                vec![injection("pack-a", "chunk-a", "hash-a")],
            ),
        )?;
        record_context_manifest(
            &mut connection,
            &manifest(
                "session-a",
                "cycle-b",
                "prompt-b",
                vec![injection("pack-b", "chunk-b", "hash-b")],
            ),
        )?;
        record_context_manifest(
            &mut connection,
            &manifest(
                "session-b",
                "cycle-other",
                "prompt-other",
                vec![injection("pack-other", "chunk-other", "hash-other")],
            ),
        )?;

        let latest = latest_context_manifest_for_session(&connection, "doc-a", "session-a")?
            .context("latest session manifest")?;
        assert_eq!(latest.cycle_id, "cycle-b");
        assert_eq!(latest.prompt_fingerprint, "prompt-b");
        assert!(latest_context_manifest_for_session(&connection, "doc-a", "missing")?.is_none());
        Ok(())
    }

    #[test]
    fn legacy_context_manifest_without_harness_converges_for_compaction_lookup() -> Result<()> {
        let mut connection = Connection::open_in_memory()?;
        connection.execute_batch(
            r#"
            CREATE TABLE context_manifest (
                document_id TEXT NOT NULL,
                session_id TEXT NOT NULL,
                cycle_id TEXT NOT NULL,
                prompt_fingerprint TEXT NOT NULL,
                pack_ids_json TEXT NOT NULL,
                chunk_ids_json TEXT NOT NULL,
                token_count INTEGER NOT NULL CHECK(token_count >= 0),
                created_at INTEGER NOT NULL,
                PRIMARY KEY (document_id, cycle_id)
            );

            INSERT INTO context_manifest (
                document_id,
                session_id,
                cycle_id,
                prompt_fingerprint,
                pack_ids_json,
                chunk_ids_json,
                token_count,
                created_at
            )
            VALUES (
                'doc-a',
                'session-a',
                'cycle-legacy',
                'prompt-legacy',
                '[]',
                '[]',
                0,
                1
            );
            "#,
        )?;

        initialize_state_db(&connection)?;

        let legacy = latest_context_manifest_for_session(&connection, "doc-a", "session-a")?
            .context("legacy manifest should remain readable after convergence")?;
        assert_eq!(legacy.cycle_id, "cycle-legacy");
        assert_eq!(legacy.harness, "unknown");

        record_context_manifest(
            &mut connection,
            &manifest(
                "session-a",
                "cycle-current",
                "prompt-current",
                vec![injection("pack-a", "chunk-a", "hash-a")],
            ),
        )?;
        let current = latest_context_manifest_for_session(&connection, "doc-a", "session-a")?
            .context("current manifest should be readable after convergence")?;
        assert_eq!(current.cycle_id, "cycle-current");
        assert_eq!(current.harness, "codex");
        Ok(())
    }

    #[test]
    fn manifest_cycle_and_injections_roll_back_together() -> Result<()> {
        let mut connection = connection()?;
        connection.execute_batch(
            r#"
            CREATE TRIGGER abort_context_injection
            BEFORE INSERT ON context_injections
            BEGIN
                SELECT RAISE(ABORT, 'injected test failure');
            END;
            "#,
        )?;
        let result = record_context_manifest(
            &mut connection,
            &manifest(
                "session-a",
                "cycle-rollback",
                "prompt-a",
                vec![injection("pack-a", "chunk-a", "hash-a")],
            ),
        );
        assert!(result.is_err());
        for table in ["document_cycles", "context_manifest", "context_injections"] {
            let count: i64 =
                connection.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })?;
            assert_eq!(count, 0, "{table} must roll back");
        }
        Ok(())
    }

    #[test]
    fn clear_context_scope_is_explicit_and_preserves_cycle_state() -> Result<()> {
        let mut connection = connection()?;
        for (session, cycle, chunk) in [
            ("session-a", "cycle-a", "chunk-a"),
            ("session-a", "cycle-b", "chunk-b"),
            ("session-b", "cycle-c", "chunk-c"),
        ] {
            record_context_manifest(
                &mut connection,
                &manifest(
                    session,
                    cycle,
                    &format!("prompt-{cycle}"),
                    vec![injection("pack-a", chunk, &format!("hash-{chunk}"))],
                ),
            )?;
        }

        let cleared = clear_context_scope(
            &mut connection,
            ContextClearScope::Session {
                document_id: "doc-a",
                session_id: "session-a",
            },
        )?;
        assert_eq!(
            cleared,
            ClearedContextRows {
                manifests: 2,
                injections: 2
            }
        );
        assert!(
            context_manifest_for_cycle(&connection, "doc-a", "cycle-a")?.is_none(),
            "cleared session manifests should be gone"
        );
        assert!(
            context_manifest_for_cycle(&connection, "doc-a", "cycle-c")?.is_some(),
            "other session should remain"
        );
        let cycle_rows: i64 =
            connection.query_row("SELECT COUNT(*) FROM document_cycles", [], |row| row.get(0))?;
        assert_eq!(cycle_rows, 3, "context clear must not own cycle state");
        Ok(())
    }

    #[test]
    fn duplicate_and_cycle_diagnostics_use_declared_indexes() -> Result<()> {
        let connection = connection()?;
        let plans = [
            (
                "SELECT id FROM context_injections \
                 WHERE document_id = 'doc' AND session_id = 'session' AND chunk_id = 'chunk' \
                 ORDER BY id LIMIT 1",
                "context_injections_session_chunk",
            ),
            (
                "SELECT id FROM context_injections \
                 WHERE document_id = 'doc' AND session_id = 'session' AND content_hash = 'hash' \
                 ORDER BY id LIMIT 1",
                "context_injections_session_content_hash",
            ),
            (
                "SELECT injection_mode, COUNT(*) FROM context_injections \
                 WHERE document_id = 'doc' AND cycle_id = 'cycle' GROUP BY injection_mode",
                "context_injections_cycle_mode",
            ),
        ];
        for (query, expected_index) in plans {
            let mut statement = connection.prepare(&format!("EXPLAIN QUERY PLAN {query}"))?;
            let details = statement
                .query_map([], |row| row.get::<_, String>(3))?
                .collect::<std::result::Result<Vec<_>, _>>()?
                .join("\n");
            assert!(
                details.contains(expected_index),
                "query should use {expected_index}; plan was {details}"
            );
        }
        Ok(())
    }

    #[test]
    fn diagnostics_report_injection_modes_for_a_cycle() -> Result<()> {
        let mut connection = connection()?;
        let mut expanded = injection("pack-a", "chunk-a", "hash-a");
        expanded.injection_mode = ContextInjectionMode::Expanded;
        let mut referenced = injection("pack-a", "chunk-b", "hash-b");
        referenced.injection_mode = ContextInjectionMode::Referenced;
        record_context_manifest(
            &mut connection,
            &manifest(
                "session-a",
                "cycle-a",
                "prompt-a",
                vec![expanded, referenced],
            ),
        )?;
        assert_eq!(
            context_injection_mode_counts_for_cycle(&connection, "doc-a", "cycle-a")?,
            vec![
                ContextInjectionModeCount {
                    injection_mode: ContextInjectionMode::Expanded,
                    count: 1,
                },
                ContextInjectionModeCount {
                    injection_mode: ContextInjectionMode::Referenced,
                    count: 1,
                },
            ]
        );
        Ok(())
    }
}
