//! # Module: archive_index
//!
//! ## Spec
//! - Maintains a derived sqlite index at `.agent-doc/archive-index.db` for compacted-turn lookup.
//! - Markdown archives under `.agent-doc/archives/*.md` remain canonical history; this DB is
//!   rebuildable and may be dropped/recreated from those files.
//! - `run_index(file, rebuild)` creates or refreshes the index for the target project.
//! - `run_search(file, query, backlog_id, session, limit, json, rebuild)` queries indexed archive
//!   chunks, biasing results toward the current document and recent archives.
//! - Search results operate on extracted turn/section chunks rather than full-file blobs.
//! - Compact-time indexing is best effort only; archive creation must still succeed when the DB
//!   update fails.
//!
//! ## Agentic Contracts
//! - The DB lives under the same project root that owns `.agent-doc/archives`.
//! - `rebuild: true` drops all derived rows and reindexes every archive markdown file on disk.
//! - Search requires at least one of `query`, `backlog_id`, or `session`.
//! - Backlog-id filters match exact `#id` references extracted from archive text.
//!
//! ## Evals
//! - rebuild_indexes_archives: archive markdown corpus produces sqlite rows for archives/turns/refs
//! - search_prefers_current_document_hits: same-query hits in the current doc sort above other docs
//! - compact_section_archives_are_chunked: `### Re:` archives index per-response sections
//! - bare_id_filter_accepts_missing_hash_prefix: `sqlarcidx` and `#sqlarcidx` behave the same

use anyhow::{Context, Result};
use rusqlite::{Connection, params};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug)]
struct ArchiveRecord {
    archive_path: String,
    document_path: String,
    session_id: Option<String>,
    component: Option<String>,
    archived_at: String,
    source_snapshot_hash: Option<String>,
    archive_byte_size: i64,
    turns: Vec<ArchiveTurn>,
    refs: Vec<ArchiveRef>,
}

#[derive(Debug)]
struct ArchiveTurn {
    turn_ordinal: i64,
    speaker: String,
    text: String,
    normalized_text: String,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct ArchiveRef {
    kind: String,
    value: String,
}

#[derive(Debug, Serialize)]
struct SearchResult {
    archive_path: String,
    document_path: String,
    session_id: Option<String>,
    archived_at: String,
    speaker: String,
    turn_ordinal: i64,
    score: i64,
    ref_hits: Vec<String>,
    preview: String,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct IndexedArchiveTurn {
    pub archive_path: String,
    pub document_path: String,
    pub session_id: Option<String>,
    pub archived_at: String,
    pub speaker: String,
    pub turn_ordinal: i64,
    pub heading: String,
    pub preview: String,
    pub refs: Vec<String>,
    pub text: String,
}

#[derive(Debug)]
struct SearchOptions<'a> {
    query: Option<&'a str>,
    backlog_id: Option<&'a str>,
    session: Option<&'a str>,
    limit: usize,
}

pub fn run_index(file: &Path, rebuild: bool) -> Result<()> {
    let project_root = find_project_root(file)?;
    let db_path = db_path(&project_root);
    let indexed = if rebuild {
        rebuild_project_index(&project_root)?
    } else {
        sync_project_index(&project_root)?
    };
    println!("{} archive(s) indexed into {}", indexed, db_path.display());
    Ok(())
}

pub fn run_search(
    file: &Path,
    query: Option<&str>,
    backlog_id: Option<&str>,
    session: Option<&str>,
    limit: usize,
    json: bool,
    rebuild: bool,
) -> Result<()> {
    if query.is_none() && backlog_id.is_none() && session.is_none() {
        anyhow::bail!("archive-search requires --query, --id, or --session");
    }
    let project_root = find_project_root(file)?;
    if rebuild {
        rebuild_project_index(&project_root)?;
    } else {
        sync_project_index(&project_root)?;
    }
    let results = search_results(
        file,
        &SearchOptions {
            query,
            backlog_id,
            session,
            limit,
        },
    )?;
    if json {
        println!("{}", serde_json::to_string_pretty(&results)?);
    } else if results.is_empty() {
        println!("No archive hits.");
    } else {
        for result in results {
            let refs = if result.ref_hits.is_empty() {
                String::new()
            } else {
                format!(" refs={}", result.ref_hits.join(","))
            };
            println!(
                "{} score={} [{} #{}] {}{}",
                result.archived_at,
                result.score,
                result.speaker,
                result.turn_ordinal,
                result.archive_path,
                refs
            );
            println!("  {}", result.preview);
        }
    }
    Ok(())
}

pub fn index_archive(doc: &Path, archive_path: &Path) -> Result<()> {
    let project_root = find_project_root(doc)?;
    let db_path = db_path(&project_root);
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    ensure_schema(&conn)?;
    let record = parse_archive(doc, archive_path, &project_root)?;
    upsert_archive(&conn, &record)?;
    Ok(())
}

pub fn list_recent_turns(
    file: &Path,
    query: Option<&str>,
    backlog_id: Option<&str>,
    limit: usize,
) -> Result<Vec<IndexedArchiveTurn>> {
    let project_root = find_project_root(file)?;
    sync_project_index(&project_root)?;
    let current_doc = canonical_document_key(file, &project_root)?;
    let current_session = current_session_id(file)?;
    let db_path = db_path(&project_root);
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    ensure_schema(&conn)?;

    let query_norm = query.map(normalize_text);
    let backlog_id = backlog_id.map(normalize_backlog_id).transpose()?;

    let mut stmt = conn.prepare(
        "SELECT a.id,
                a.archive_path,
                a.document_path,
                a.session_id,
                a.archived_at,
                t.turn_ordinal,
                t.speaker,
                t.text,
                t.normalized_text
         FROM archive_turns t
         JOIN archives a ON a.id = t.archive_id
         WHERE a.document_path = ?1
           AND t.speaker = 'assistant'
           AND (?2 IS NULL OR instr(t.normalized_text, ?2) > 0)
           AND (?3 IS NULL OR EXISTS (
                SELECT 1 FROM archive_refs r
                WHERE r.archive_id = a.id
                  AND r.ref_kind = 'backlog_id'
                  AND r.ref_value = ?3
           ))
         ORDER BY a.archived_at DESC, t.turn_ordinal DESC",
    )?;

    let rows = stmt.query_map(
        params![current_doc, query_norm.as_deref(), backlog_id.as_deref()],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        },
    )?;

    let mut results = Vec::new();
    for row in rows {
        let (
            archive_id,
            archive_path,
            document_path,
            session_id,
            archived_at,
            turn_ordinal,
            speaker,
            text,
            normalized_text,
        ) = row?;
        let refs = archive_backlog_refs(&conn, archive_id)?;
        let score = score_result(
            &document_path,
            session_id.as_deref(),
            &normalized_text,
            query_norm.as_deref(),
            SearchScoreContext {
                has_backlog_filter: backlog_id.is_some(),
                has_ref_hit: backlog_id
                    .as_deref()
                    .map(|needle| refs.iter().any(|candidate| candidate == needle))
                    .unwrap_or(false),
                current_doc: &current_doc,
                current_session: current_session.as_deref(),
            },
        );
        results.push((
            score,
            IndexedArchiveTurn {
                heading: turn_heading(&text, &speaker, turn_ordinal),
                preview: preview_text(&text),
                archive_path,
                document_path,
                session_id,
                archived_at,
                speaker,
                turn_ordinal,
                refs,
                text,
            },
        ));
    }

    results.sort_by(|(score_a, a), (score_b, b)| {
        score_b
            .cmp(score_a)
            .then_with(|| b.archived_at.cmp(&a.archived_at))
            .then_with(|| b.turn_ordinal.cmp(&a.turn_ordinal))
    });
    results.truncate(limit);
    Ok(results.into_iter().map(|(_, turn)| turn).collect())
}

pub fn fetch_turn_window(
    file: &Path,
    archive_path: &str,
    turn_ordinal: i64,
    before: usize,
    after: usize,
) -> Result<Vec<IndexedArchiveTurn>> {
    let project_root = find_project_root(file)?;
    sync_project_index(&project_root)?;
    let db_path = db_path(&project_root);
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    ensure_schema(&conn)?;

    let archive_path = normalize_archive_path(&project_root, archive_path);
    let start = turn_ordinal.saturating_sub(before as i64);
    let end = turn_ordinal.saturating_add(after as i64);
    let mut stmt = conn.prepare(
        "SELECT a.id,
                a.archive_path,
                a.document_path,
                a.session_id,
                a.archived_at,
                t.turn_ordinal,
                t.speaker,
                t.text
         FROM archive_turns t
         JOIN archives a ON a.id = t.archive_id
         WHERE a.archive_path = ?1
           AND t.turn_ordinal BETWEEN ?2 AND ?3
         ORDER BY t.turn_ordinal ASC",
    )?;

    let turns = stmt
        .query_map(params![archive_path, start, end], |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
            ))
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;

    if turns.is_empty() {
        anyhow::bail!(
            "archive turn not found for {}#{}",
            archive_path,
            turn_ordinal
        );
    }

    let mut results = Vec::new();
    for (
        archive_id,
        archive_path,
        document_path,
        session_id,
        archived_at,
        turn_ordinal,
        speaker,
        text,
    ) in turns
    {
        results.push(IndexedArchiveTurn {
            heading: turn_heading(&text, &speaker, turn_ordinal),
            preview: preview_text(&text),
            refs: archive_backlog_refs(&conn, archive_id)?,
            archive_path,
            document_path,
            session_id,
            archived_at,
            speaker,
            turn_ordinal,
            text,
        });
    }
    Ok(results)
}

fn rebuild_project_index(project_root: &Path) -> Result<usize> {
    let db_path = db_path(project_root);
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    reset_schema(&conn)?;
    let archives = list_archive_files(project_root)?;
    for archive_path in &archives {
        let record = parse_archive_from_project_root(archive_path, project_root)?;
        upsert_archive(&conn, &record)?;
    }
    Ok(archives.len())
}

fn sync_project_index(project_root: &Path) -> Result<usize> {
    let db_path = db_path(project_root);
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    ensure_schema(&conn)?;
    let archives = list_archive_files(project_root)?;
    for archive_path in &archives {
        let record = parse_archive_from_project_root(archive_path, project_root)?;
        upsert_archive(&conn, &record)?;
    }
    Ok(archives.len())
}

fn search_results(file: &Path, options: &SearchOptions<'_>) -> Result<Vec<SearchResult>> {
    let project_root = find_project_root(file)?;
    let current_doc = canonical_document_key(file, &project_root)?;
    let current_session = current_session_id(file)?;
    let db_path = db_path(&project_root);
    let conn = Connection::open(&db_path)
        .with_context(|| format!("failed to open {}", db_path.display()))?;
    ensure_schema(&conn)?;

    let query_norm = options.query.map(normalize_text);
    let backlog_id = options.backlog_id.map(normalize_backlog_id).transpose()?;

    let mut stmt = conn.prepare(
        "SELECT a.id,
                a.archive_path,
                a.document_path,
                a.session_id,
                a.archived_at,
                t.turn_ordinal,
                t.speaker,
                t.text,
                t.normalized_text
         FROM archive_turns t
         JOIN archives a ON a.id = t.archive_id
         WHERE (?1 IS NULL OR instr(t.normalized_text, ?1) > 0)
           AND (?2 IS NULL OR a.session_id = ?2)
           AND (?3 IS NULL OR EXISTS (
                SELECT 1 FROM archive_refs r
                WHERE r.archive_id = a.id
                  AND r.ref_kind = 'backlog_id'
                  AND r.ref_value = ?3
           ))
         ORDER BY a.archived_at DESC, t.turn_ordinal DESC",
    )?;

    let rows = stmt.query_map(
        params![
            query_norm.as_deref(),
            options.session,
            backlog_id.as_deref()
        ],
        |row| {
            Ok((
                row.get::<_, i64>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, Option<String>>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, i64>(5)?,
                row.get::<_, String>(6)?,
                row.get::<_, String>(7)?,
                row.get::<_, String>(8)?,
            ))
        },
    )?;

    let mut results = Vec::new();
    for row in rows {
        let (
            archive_id,
            archive_path,
            document_path,
            session_id,
            archived_at,
            turn_ordinal,
            speaker,
            text,
            normalized_text,
        ) = row?;
        let ref_hits = ref_hits(&conn, archive_id, backlog_id.as_deref())?;
        let score = score_result(
            &document_path,
            session_id.as_deref(),
            &normalized_text,
            query_norm.as_deref(),
            SearchScoreContext {
                has_backlog_filter: backlog_id.is_some(),
                has_ref_hit: !ref_hits.is_empty(),
                current_doc: &current_doc,
                current_session: current_session.as_deref(),
            },
        );
        results.push(SearchResult {
            archive_path,
            document_path,
            session_id,
            archived_at,
            speaker,
            turn_ordinal,
            score,
            ref_hits,
            preview: preview_text(&text),
        });
    }

    results.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.archived_at.cmp(&a.archived_at))
            .then_with(|| a.turn_ordinal.cmp(&b.turn_ordinal))
    });
    results.truncate(options.limit);
    Ok(results)
}

struct SearchScoreContext<'a> {
    has_backlog_filter: bool,
    has_ref_hit: bool,
    current_doc: &'a str,
    current_session: Option<&'a str>,
}

fn score_result(
    document_path: &str,
    session_id: Option<&str>,
    normalized_text: &str,
    query_norm: Option<&str>,
    context: SearchScoreContext<'_>,
) -> i64 {
    let mut score = 0;
    if let Some(query) = query_norm
        && normalized_text.contains(query)
    {
        score += 40;
    }
    if context.has_backlog_filter && context.has_ref_hit {
        score += 100;
    }
    if document_path == context.current_doc {
        score += 30;
    }
    if session_id.is_some() && session_id == context.current_session {
        score += 15;
    }
    score
}

fn ref_hits(conn: &Connection, archive_id: i64, backlog_id: Option<&str>) -> Result<Vec<String>> {
    let Some(backlog_id) = backlog_id else {
        return Ok(Vec::new());
    };
    let mut stmt = conn.prepare(
        "SELECT ref_value FROM archive_refs
         WHERE archive_id = ?1 AND ref_kind = 'backlog_id' AND ref_value = ?2
         ORDER BY ref_value",
    )?;
    let hits = stmt
        .query_map(params![archive_id, backlog_id], |row| {
            row.get::<_, String>(0)
        })?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(hits)
}

fn archive_backlog_refs(conn: &Connection, archive_id: i64) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(
        "SELECT ref_value FROM archive_refs
         WHERE archive_id = ?1 AND ref_kind = 'backlog_id'
         ORDER BY ref_value",
    )?;
    let hits = stmt
        .query_map(params![archive_id], |row| row.get::<_, String>(0))?
        .collect::<std::result::Result<Vec<_>, _>>()?;
    Ok(hits)
}

fn ensure_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "PRAGMA foreign_keys = ON;
         CREATE TABLE IF NOT EXISTS archives (
             id INTEGER PRIMARY KEY,
             archive_path TEXT NOT NULL UNIQUE,
             document_path TEXT NOT NULL,
             session_id TEXT,
             component TEXT,
             archived_at TEXT NOT NULL,
             source_snapshot_hash TEXT,
             archive_byte_size INTEGER NOT NULL,
             indexed_at TEXT NOT NULL
         );
         CREATE TABLE IF NOT EXISTS archive_turns (
             id INTEGER PRIMARY KEY,
             archive_id INTEGER NOT NULL REFERENCES archives(id) ON DELETE CASCADE,
             turn_ordinal INTEGER NOT NULL,
             speaker TEXT NOT NULL,
             text TEXT NOT NULL,
             normalized_text TEXT NOT NULL,
             UNIQUE(archive_id, turn_ordinal, speaker)
         );
         CREATE TABLE IF NOT EXISTS archive_refs (
             archive_id INTEGER NOT NULL REFERENCES archives(id) ON DELETE CASCADE,
             ref_kind TEXT NOT NULL,
             ref_value TEXT NOT NULL,
             UNIQUE(archive_id, ref_kind, ref_value)
         );
         CREATE INDEX IF NOT EXISTS idx_archive_turns_normalized_text
             ON archive_turns(normalized_text);
         CREATE INDEX IF NOT EXISTS idx_archive_refs_lookup
             ON archive_refs(ref_kind, ref_value);
         CREATE INDEX IF NOT EXISTS idx_archives_document_archived_at
             ON archives(document_path, archived_at DESC);",
    )?;
    Ok(())
}

fn reset_schema(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        "DROP TABLE IF EXISTS archive_refs;
         DROP TABLE IF EXISTS archive_turns;
         DROP TABLE IF EXISTS archives;",
    )?;
    ensure_schema(conn)
}

fn upsert_archive(conn: &Connection, record: &ArchiveRecord) -> Result<()> {
    conn.execute(
        "INSERT INTO archives (
             archive_path, document_path, session_id, component, archived_at,
             source_snapshot_hash, archive_byte_size, indexed_at
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(archive_path) DO UPDATE SET
             document_path = excluded.document_path,
             session_id = excluded.session_id,
             component = excluded.component,
             archived_at = excluded.archived_at,
             source_snapshot_hash = excluded.source_snapshot_hash,
             archive_byte_size = excluded.archive_byte_size,
             indexed_at = excluded.indexed_at",
        params![
            record.archive_path,
            record.document_path,
            record.session_id,
            record.component,
            record.archived_at,
            record.source_snapshot_hash,
            record.archive_byte_size,
            timestamp_now(),
        ],
    )?;
    let archive_id: i64 = conn.query_row(
        "SELECT id FROM archives WHERE archive_path = ?1",
        params![record.archive_path],
        |row| row.get(0),
    )?;

    conn.execute(
        "DELETE FROM archive_turns WHERE archive_id = ?1",
        params![archive_id],
    )?;
    conn.execute(
        "DELETE FROM archive_refs WHERE archive_id = ?1",
        params![archive_id],
    )?;

    for turn in &record.turns {
        conn.execute(
            "INSERT INTO archive_turns (
                 archive_id, turn_ordinal, speaker, text, normalized_text
             ) VALUES (?1, ?2, ?3, ?4, ?5)",
            params![
                archive_id,
                turn.turn_ordinal,
                turn.speaker,
                turn.text,
                turn.normalized_text
            ],
        )?;
    }

    for archive_ref in &record.refs {
        conn.execute(
            "INSERT OR IGNORE INTO archive_refs (archive_id, ref_kind, ref_value)
             VALUES (?1, ?2, ?3)",
            params![archive_id, archive_ref.kind, archive_ref.value],
        )?;
    }
    Ok(())
}

fn list_archive_files(project_root: &Path) -> Result<Vec<PathBuf>> {
    let archive_dir = project_root.join(".agent-doc/archives");
    if !archive_dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for entry in fs::read_dir(&archive_dir)
        .with_context(|| format!("failed to read {}", archive_dir.display()))?
    {
        let entry = entry?;
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            files.push(path);
        }
    }
    files.sort();
    Ok(files)
}

fn parse_archive(doc: &Path, archive_path: &Path, project_root: &Path) -> Result<ArchiveRecord> {
    let raw = fs::read_to_string(archive_path)
        .with_context(|| format!("failed to read {}", archive_path.display()))?;
    let (frontmatter, body) = split_archive_frontmatter(&raw)?;
    let document_path = frontmatter
        .get("document")
        .cloned()
        .unwrap_or(canonical_document_key(doc, project_root)?);
    Ok(ArchiveRecord {
        archive_path: relative_to_root(archive_path, project_root),
        document_path,
        session_id: frontmatter.get("session").cloned(),
        component: frontmatter.get("component").cloned(),
        archived_at: frontmatter
            .get("archived_at")
            .cloned()
            .unwrap_or_else(timestamp_now),
        source_snapshot_hash: archive_snapshot_hash(archive_path),
        archive_byte_size: raw.len() as i64,
        turns: parse_turns(&body),
        refs: extract_refs(&body),
    })
}

fn parse_archive_from_project_root(
    archive_path: &Path,
    project_root: &Path,
) -> Result<ArchiveRecord> {
    let raw = fs::read_to_string(archive_path)
        .with_context(|| format!("failed to read {}", archive_path.display()))?;
    let (frontmatter, body) = split_archive_frontmatter(&raw)?;
    let document_path = frontmatter
        .get("document")
        .cloned()
        .unwrap_or_else(|| "<unknown>".to_string());
    Ok(ArchiveRecord {
        archive_path: relative_to_root(archive_path, project_root),
        document_path,
        session_id: frontmatter.get("session").cloned(),
        component: frontmatter.get("component").cloned(),
        archived_at: frontmatter
            .get("archived_at")
            .cloned()
            .unwrap_or_else(timestamp_now),
        source_snapshot_hash: archive_snapshot_hash(archive_path),
        archive_byte_size: raw.len() as i64,
        turns: parse_turns(&body),
        refs: extract_refs(&body),
    })
}

fn split_archive_frontmatter(raw: &str) -> Result<(BTreeMap<String, String>, String)> {
    let Some(rest) = raw.strip_prefix("---\n") else {
        anyhow::bail!("archive missing frontmatter header");
    };
    let Some((frontmatter, body)) = rest.split_once("\n---\n") else {
        anyhow::bail!("archive missing closing frontmatter delimiter");
    };
    let mut map = BTreeMap::new();
    for line in frontmatter.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        map.insert(key.trim().to_string(), value.trim().to_string());
    }
    Ok((map, body.trim().to_string()))
}

fn parse_turns(body: &str) -> Vec<ArchiveTurn> {
    let exchange_turns = parse_exchange_turns(body);
    if !exchange_turns.is_empty() {
        return exchange_turns;
    }

    let (preamble, sections) = crate::compact::parse_topic_sections(body);
    if !sections.is_empty() {
        let mut turns = Vec::new();
        let mut ordinal = 1;
        if !preamble.trim().is_empty() {
            turns.push(ArchiveTurn {
                turn_ordinal: ordinal,
                speaker: "assistant".to_string(),
                text: preamble.trim().to_string(),
                normalized_text: normalize_text(preamble.trim()),
            });
            ordinal += 1;
        }
        for section in sections {
            let trimmed = section.trim();
            if trimmed.is_empty() {
                continue;
            }
            turns.push(ArchiveTurn {
                turn_ordinal: ordinal,
                speaker: "assistant".to_string(),
                text: trimmed.to_string(),
                normalized_text: normalize_text(trimmed),
            });
            ordinal += 1;
        }
        return turns;
    }

    let trimmed = body.trim();
    if trimmed.is_empty() {
        Vec::new()
    } else {
        vec![ArchiveTurn {
            turn_ordinal: 1,
            speaker: "assistant".to_string(),
            text: trimmed.to_string(),
            normalized_text: normalize_text(trimmed),
        }]
    }
}

fn parse_exchange_turns(body: &str) -> Vec<ArchiveTurn> {
    let mut turns = Vec::new();
    let mut current_speaker: Option<&str> = None;
    let mut current = String::new();
    let mut ordinal = 1;
    let mut in_code_block = false;

    for line in body.lines() {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
        }
        if !in_code_block && (line == "## User" || line == "## Assistant") {
            flush_turn(&mut turns, &mut current_speaker, &mut current, &mut ordinal);
            current_speaker = Some(if line == "## User" {
                "user"
            } else {
                "assistant"
            });
            continue;
        }
        if current_speaker.is_some() {
            current.push_str(line);
            current.push('\n');
        }
    }
    flush_turn(&mut turns, &mut current_speaker, &mut current, &mut ordinal);
    turns
}

fn flush_turn(
    turns: &mut Vec<ArchiveTurn>,
    current_speaker: &mut Option<&str>,
    current: &mut String,
    ordinal: &mut i64,
) {
    let Some(speaker) = current_speaker.take() else {
        return;
    };
    let trimmed = current.trim();
    if trimmed.is_empty() {
        current.clear();
        return;
    }
    turns.push(ArchiveTurn {
        turn_ordinal: *ordinal,
        speaker: speaker.to_string(),
        text: trimmed.to_string(),
        normalized_text: normalize_text(trimmed),
    });
    *ordinal += 1;
    current.clear();
}

fn extract_refs(body: &str) -> Vec<ArchiveRef> {
    let mut refs = BTreeSet::new();
    for backlog_id in extract_backlog_ids(body) {
        refs.insert(ArchiveRef {
            kind: "backlog_id".to_string(),
            value: backlog_id,
        });
    }
    for plan_path in extract_plan_paths(body) {
        refs.insert(ArchiveRef {
            kind: "plan_path".to_string(),
            value: plan_path,
        });
    }
    refs.into_iter().collect()
}

fn extract_backlog_ids(text: &str) -> BTreeSet<String> {
    let chars: Vec<char> = text.chars().collect();
    let mut found = BTreeSet::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '#'
            && (i == 0 || chars[i - 1] != '#')
            && i + 1 < chars.len()
            && is_backlog_id_char(chars[i + 1])
        {
            let mut j = i + 1;
            while j < chars.len() && is_backlog_id_char(chars[j]) {
                j += 1;
            }
            found.insert(chars[i..j].iter().collect());
            i = j;
            continue;
        }
        i += 1;
    }
    found
}

fn extract_plan_paths(text: &str) -> BTreeSet<String> {
    text.split_whitespace()
        .filter_map(|token| {
            let cleaned =
                token.trim_matches(|c: char| matches!(c, '`' | '"' | '\'' | '(' | ')' | ',' | '.'));
            if cleaned.contains('/') && cleaned.ends_with(".md") {
                Some(cleaned.to_string())
            } else {
                None
            }
        })
        .collect()
}

fn preview_text(text: &str) -> String {
    let collapsed = normalize_text(text);
    let mut chars = collapsed.chars();
    let preview: String = chars.by_ref().take(120).collect();
    if chars.next().is_some() {
        format!("{}...", preview.chars().take(117).collect::<String>())
    } else {
        preview
    }
}

fn turn_heading(text: &str, speaker: &str, turn_ordinal: i64) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
        .unwrap_or_else(|| format!("{} turn #{}", speaker, turn_ordinal))
}

fn normalize_text(text: &str) -> String {
    text.split_whitespace()
        .map(|segment| segment.to_ascii_lowercase())
        .collect::<Vec<_>>()
        .join(" ")
}

fn normalize_backlog_id(raw: &str) -> Result<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        anyhow::bail!("backlog id cannot be empty");
    }
    Ok(if trimmed.starts_with('#') {
        trimmed.to_string()
    } else {
        format!("#{trimmed}")
    })
}

fn is_backlog_id_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_' || ch == '-'
}

fn current_session_id(file: &Path) -> Result<Option<String>> {
    let content =
        fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))?;
    let Ok((frontmatter, _)) = crate::frontmatter::parse(&content) else {
        return Ok(None);
    };
    Ok(frontmatter.session)
}

fn archive_snapshot_hash(path: &Path) -> Option<String> {
    let stem = path.file_stem()?.to_str()?;
    let hash: String = stem
        .chars()
        .take_while(|ch| ch.is_ascii_hexdigit())
        .collect();
    if hash.is_empty() { None } else { Some(hash) }
}

fn canonical_document_key(file: &Path, project_root: &Path) -> Result<String> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    Ok(relative_to_root(&canonical, project_root))
}

fn relative_to_root(path: &Path, project_root: &Path) -> String {
    path.strip_prefix(project_root)
        .map(|rel| rel.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| path.to_string_lossy().replace('\\', "/"))
}

fn normalize_archive_path(project_root: &Path, archive_path: &str) -> String {
    let path = Path::new(archive_path);
    if path.is_absolute() {
        relative_to_root(path, project_root)
    } else {
        archive_path.replace('\\', "/")
    }
}

fn db_path(project_root: &Path) -> PathBuf {
    project_root.join(".agent-doc/archive-index.db")
}

fn find_project_root(file: &Path) -> Result<PathBuf> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let mut dir = canonical.parent();
    while let Some(candidate) = dir {
        if candidate.join(".agent-doc").is_dir() {
            return Ok(candidate.to_path_buf());
        }
        dir = candidate.parent();
    }
    anyhow::bail!("failed to find project root for {}", file.display());
}

fn timestamp_now() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();
    let secs = now.as_secs();
    let days = secs / 86_400;
    let time_of_day = secs % 86_400;
    let hours = time_of_day / 3_600;
    let minutes = (time_of_day % 3_600) / 60;
    let seconds = time_of_day % 60;

    let mut year = 1970_i64;
    let mut remaining_days = days as i64;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if remaining_days < days_in_year {
            break;
        }
        remaining_days -= days_in_year;
        year += 1;
    }
    let month_days: &[i64] = if is_leap_year(year) {
        &[31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        &[31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 0;
    for &month_days in month_days {
        if remaining_days < month_days {
            break;
        }
        remaining_days -= month_days;
        month += 1;
    }
    format!(
        "{:04}{:02}{:02}-{:02}{:02}{:02}",
        year,
        month + 1,
        remaining_days + 1,
        hours,
        minutes,
        seconds
    )
}

fn is_leap_year(year: i64) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use rusqlite::Connection;

    #[test]
    fn bare_id_filter_accepts_missing_hash_prefix() {
        assert_eq!(normalize_backlog_id("sqlarcidx").unwrap(), "#sqlarcidx");
        assert_eq!(normalize_backlog_id("#sqlarcidx").unwrap(), "#sqlarcidx");
    }

    #[test]
    fn compact_section_archives_are_chunked() {
        let body = "### Session Summary\n\nArchived context.\n\n### Re: one — gpt-5\n\nFirst body.\n\n### Re: two — gpt-5\n\nSecond body.\n";
        let turns = parse_turns(body);
        assert_eq!(turns.len(), 3);
        assert!(turns[0].text.contains("Session Summary"));
        assert!(turns[1].text.contains("### Re: one"));
        assert!(turns[2].text.contains("### Re: two"));
    }

    #[test]
    fn rebuild_indexes_archives() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        let doc = root.join("tasks/session.md");
        fs::create_dir_all(doc.parent().unwrap()).unwrap();
        fs::write(
            &doc,
            "---\nagent_doc_session: current-session\n---\n\nbody\n",
        )
        .unwrap();
        let archive = root.join(".agent-doc/archives/hash-20260506-000000.md");
        fs::write(
            &archive,
            concat!(
                "---\n",
                "archived_from: compact\n",
                "archived_at: 20260506-000000\n",
                "component: exchange\n",
                "document: tasks/session.md\n",
                "session: current-session\n",
                "---\n\n",
                "## User\n\nDo #sqlarcidx.\n\n",
                "## Assistant\n\nPlan: tasks/agent-doc/plan-sqlite-compacted-turn-archive.md\n"
            ),
        )
        .unwrap();

        let indexed = rebuild_project_index(root).unwrap();
        assert_eq!(indexed, 1);

        let conn = Connection::open(root.join(".agent-doc/archive-index.db")).unwrap();
        let archive_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM archives", [], |row| row.get(0))
            .unwrap();
        let turn_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM archive_turns", [], |row| row.get(0))
            .unwrap();
        let ref_count: i64 = conn
            .query_row("SELECT COUNT(*) FROM archive_refs", [], |row| row.get(0))
            .unwrap();
        assert_eq!(archive_count, 1);
        assert_eq!(turn_count, 2);
        assert!(ref_count >= 2);
    }

    #[test]
    fn search_prefers_current_document_hits() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        fs::create_dir_all(root.join(".agent-doc/archives")).unwrap();
        let current_doc = root.join("tasks/current.md");
        let other_doc = root.join("tasks/other.md");
        fs::create_dir_all(current_doc.parent().unwrap()).unwrap();
        fs::write(
            &current_doc,
            "---\nagent_doc_session: current-session\n---\n\nbody\n",
        )
        .unwrap();
        fs::write(
            &other_doc,
            "---\nagent_doc_session: other-session\n---\n\nbody\n",
        )
        .unwrap();

        fs::write(
            root.join(".agent-doc/archives/a-20260506-000000.md"),
            concat!(
                "---\n",
                "archived_from: compact\n",
                "archived_at: 20260506-000000\n",
                "component: exchange\n",
                "document: tasks/current.md\n",
                "session: current-session\n",
                "---\n\n",
                "## Assistant\n\nNeed #sqlarcidx and sqlite lookup.\n"
            ),
        )
        .unwrap();
        fs::write(
            root.join(".agent-doc/archives/b-20260506-000100.md"),
            concat!(
                "---\n",
                "archived_from: compact\n",
                "archived_at: 20260506-000100\n",
                "component: exchange\n",
                "document: tasks/other.md\n",
                "session: other-session\n",
                "---\n\n",
                "## Assistant\n\nNeed #sqlarcidx and sqlite lookup.\n"
            ),
        )
        .unwrap();

        rebuild_project_index(root).unwrap();
        let results = search_results(
            &current_doc,
            &SearchOptions {
                query: Some("sqlite lookup"),
                backlog_id: Some("#sqlarcidx"),
                session: None,
                limit: 10,
            },
        )
        .unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0].document_path, "tasks/current.md");
        assert!(results[0].score > results[1].score);
    }
}
