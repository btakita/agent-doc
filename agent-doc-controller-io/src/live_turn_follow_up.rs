//! One queued follow-up for a running owner turn (`#steercoalesce`).
//!
//! A hookless harness queues every Enter. The pool refill and the steering
//! paste share one compact reference: later snapshots update its payload and
//! do not submit again while that reference is still unconsumed.

use std::path::Path;

use agent_doc_controller::live_turn_follow_up::{
    LiveTurnFollowUpSlot, live_turn_follow_up_payload, merge_steering,
};
use agent_doc_sqlite::state_store::{
    clear_project_runtime_state_in_db, list_project_runtime_state_from_db,
    load_project_runtime_state_from_db, open_state_db, state_db_path,
    upsert_project_runtime_state_in_db,
};
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// Wall-clock ceiling for one compact steering reference. The notice text says
/// "within 5m"; refreshing an outstanding reference keeps that sentence true
/// for the owner cycle that is still running.
pub const STEERING_SURROGATE_TTL_MS: u64 = 5 * 60 * 1_000;

const STEERING_SURROGATE_KEY_PREFIX: &str = "steering-surrogate:";
const LIVE_TURN_FOLLOW_UP_KEY_PREFIX: &str = "live-turn-follow-up:";
const MIDTURN_STEERING_BASE_PREFIX: &str = "midturn_steering:base:";

/// Composer text for one running turn, plus whether it still needs an Enter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LiveTurnFollowUpStage {
    /// The compact `agent-doc steering --token` line. This is what a hookless
    /// composer receives, not the pool dump or the steering diff.
    pub notice: String,
    /// False once a paste of `notice` has been proven. A later slot update
    /// then stays behind the same token.
    pub needs_queue_paste: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct SteeringSurrogate {
    document: String,
    cycle_id: String,
    payload: String,
    expires_at_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
struct LiveTurnFollowUpLedger {
    cycle_id: String,
    token: String,
    pasted: bool,
    #[serde(default)]
    pool: Option<String>,
    #[serde(default)]
    steering: Option<String>,
}

#[derive(Debug, Deserialize)]
struct CycleOnly {
    cycle_id: String,
}

/// Active owner cycle for `file`, when preflight or admit seeded one.
///
/// `None` means the caller pastes its own text. A missing watermark is not a
/// second queue row.
pub fn current_steering_cycle_id(file: &Path) -> Result<Option<String>> {
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    if !state_db_path(&root).exists() {
        return Ok(None);
    }
    let document = document_identity(file);
    let conn = open_state_db(&root)?;
    let Some(raw) = load_project_runtime_state_from_db(&conn, &base_key(&document))? else {
        return Ok(None);
    };
    let cycle: CycleOnly = serde_json::from_str(&raw).context("parse steering cycle")?;
    let cycle = cycle.cycle_id.trim();
    if cycle.is_empty() {
        Ok(None)
    } else {
        Ok(Some(cycle.to_string()))
    }
}

/// Fold `text` into the running turn's one reference.
///
/// The same unconsumed token is rewritten in place. A missing, expired, or
/// different-cycle reference mints a new token that still needs one Enter.
/// Pool text replaces the pool slot. Steering text accumulates.
pub fn stage_live_turn_follow_up(
    file: &Path,
    cycle_id: &str,
    slot: LiveTurnFollowUpSlot,
    text: &str,
    now_ms: u64,
) -> Result<LiveTurnFollowUpStage> {
    anyhow::ensure!(
        !cycle_id.trim().is_empty(),
        "cannot issue compact steering without an active cycle"
    );
    anyhow::ensure!(!text.trim().is_empty(), "live-turn follow-up text is empty");
    let root = project_root(file).context("steering document is outside a project")?;
    let document = canonical_document(file)?;
    let conn = open_state_db(&root)?;
    gc_expired_surrogates(&conn, now_ms)?;

    let mut pool = None;
    let mut steering = None;
    let mut token = None;
    let mut pasted = false;
    if let Some(existing) = load_ledger(&conn, &document)?
        && existing.cycle_id == cycle_id
    {
        pool = existing.pool;
        if let Some(record) = load_surrogate(&conn, &existing.token)?
            && record.expires_at_ms > now_ms
            && record.document == document
            && record.cycle_id == cycle_id
        {
            steering = existing.steering;
            token = Some(existing.token);
            pasted = existing.pasted;
        }
    }
    match slot {
        LiveTurnFollowUpSlot::Pool => pool = Some(text.trim().to_string()),
        LiveTurnFollowUpSlot::Steering => {
            let merged = merge_steering(steering.as_deref(), text);
            steering = (!merged.is_empty()).then_some(merged);
        }
    }
    let payload = live_turn_follow_up_payload(pool.as_deref(), steering.as_deref())
        .context("live-turn follow-up payload is empty")?;
    let token = match token {
        Some(token) => token,
        None => {
            pasted = false;
            new_token(cycle_id, now_ms, text, &document)
        }
    };
    let record = SteeringSurrogate {
        document: document.clone(),
        cycle_id: cycle_id.to_string(),
        payload,
        expires_at_ms: now_ms.saturating_add(STEERING_SURROGATE_TTL_MS),
    };
    upsert_project_runtime_state_in_db(
        &conn,
        &surrogate_key(&token),
        &serde_json::to_string(&record)?,
        now_ms,
    )?;
    let ledger = LiveTurnFollowUpLedger {
        cycle_id: cycle_id.to_string(),
        token: token.clone(),
        pasted,
        pool,
        steering,
    };
    upsert_project_runtime_state_in_db(
        &conn,
        &ledger_key(&document),
        &serde_json::to_string(&ledger)?,
        now_ms,
    )?;
    Ok(LiveTurnFollowUpStage {
        notice: notice(&document, &token),
        needs_queue_paste: !pasted,
    })
}

/// The compact reference was proven to leave the composer.
///
/// A failed paste must not call this. The next attempt then submits the same
/// notice instead of opening a second queue row.
pub fn mark_live_turn_follow_up_queued(file: &Path, cycle_id: &str) -> Result<()> {
    let root = project_root(file).context("steering document is outside a project")?;
    let document = canonical_document(file)?;
    let conn = open_state_db(&root)?;
    let Some(mut ledger) = load_ledger(&conn, &document)? else {
        anyhow::bail!("no live-turn follow-up to mark queued");
    };
    anyhow::ensure!(
        ledger.cycle_id == cycle_id,
        "live-turn follow-up cycle changed before the paste was marked"
    );
    if ledger.pasted {
        return Ok(());
    }
    ledger.pasted = true;
    let now_ms = wall_now_ms();
    upsert_project_runtime_state_in_db(
        &conn,
        &ledger_key(&document),
        &serde_json::to_string(&ledger)?,
        now_ms,
    )?;
    Ok(())
}

/// Keep an unconsumed reference alive while this owner cycle is still current.
///
/// A quiet gap longer than five minutes must not mint a second token for the
/// same turn. A cycle change does not refresh; the abandoned reference then
/// expires on the existing ceiling.
pub fn refresh_outstanding_live_turn_follow_up(file: &Path, now_ms: u64) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    if !state_db_path(&root).exists() {
        return Ok(());
    }
    let document = canonical_document(file)?;
    let conn = open_state_db(&root)?;
    let Some(ledger) = load_ledger(&conn, &document)? else {
        return Ok(());
    };
    let Some(current) = load_cycle_id(&conn, &document)? else {
        return Ok(());
    };
    if ledger.cycle_id != current {
        return Ok(());
    }
    let Some(raw) = load_project_runtime_state_from_db(&conn, &surrogate_key(&ledger.token))?
    else {
        return Ok(());
    };
    let mut record: SteeringSurrogate =
        serde_json::from_str(&raw).context("parse live-turn follow-up surrogate")?;
    if record.expires_at_ms <= now_ms
        || record.cycle_id != ledger.cycle_id
        || record.document != document
    {
        return Ok(());
    }
    record.expires_at_ms = now_ms.saturating_add(STEERING_SURROGATE_TTL_MS);
    upsert_project_runtime_state_in_db(
        &conn,
        &surrogate_key(&ledger.token),
        &serde_json::to_string(&record)?,
        now_ms,
    )?;
    Ok(())
}

fn load_cycle_id(
    conn: &agent_doc_sqlite::state_store::Connection,
    document: &str,
) -> Result<Option<String>> {
    let Some(raw) = load_project_runtime_state_from_db(conn, &base_key(document))? else {
        return Ok(None);
    };
    let cycle: CycleOnly = serde_json::from_str(&raw).context("parse steering cycle")?;
    let cycle = cycle.cycle_id.trim();
    if cycle.is_empty() {
        Ok(None)
    } else {
        Ok(Some(cycle.to_string()))
    }
}

fn load_ledger(
    conn: &agent_doc_sqlite::state_store::Connection,
    document: &str,
) -> Result<Option<LiveTurnFollowUpLedger>> {
    let Some(raw) = load_project_runtime_state_from_db(conn, &ledger_key(document))? else {
        return Ok(None);
    };
    serde_json::from_str(&raw)
        .context("parse live-turn follow-up ledger")
        .map(Some)
}

fn load_surrogate(
    conn: &agent_doc_sqlite::state_store::Connection,
    token: &str,
) -> Result<Option<SteeringSurrogate>> {
    let Some(raw) = load_project_runtime_state_from_db(conn, &surrogate_key(token))? else {
        return Ok(None);
    };
    serde_json::from_str(&raw)
        .context("parse live-turn follow-up surrogate")
        .map(Some)
}

fn gc_expired_surrogates(
    conn: &agent_doc_sqlite::state_store::Connection,
    now_ms: u64,
) -> Result<()> {
    for (key, raw, _) in list_project_runtime_state_from_db(conn, STEERING_SURROGATE_KEY_PREFIX)? {
        let expired = serde_json::from_str::<SteeringSurrogate>(&raw)
            .map(|record| record.expires_at_ms <= now_ms)
            .unwrap_or(true);
        if expired {
            clear_project_runtime_state_in_db(conn, &key)?;
        }
    }
    Ok(())
}

fn new_token(cycle_id: &str, now_ms: u64, text: &str, document: &str) -> String {
    let hash = agent_doc_hash::content_hash(&format!("{cycle_id}\n{now_ms}\n{text}\n{document}"));
    hash.get(..32).unwrap_or(&hash).to_string()
}

fn notice(document: &str, token: &str) -> String {
    format!(
        "[agent-doc] operator steering: run `agent-doc steering {} --token {}` or call MCP tool `agent_doc_steering` with the same file/token within 5m (one use)",
        shell_single_quote(document),
        shell_single_quote(token),
    )
}

fn shell_single_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn document_identity(file: &Path) -> String {
    file.canonicalize()
        .unwrap_or_else(|_| file.to_path_buf())
        .display()
        .to_string()
}

fn canonical_document(file: &Path) -> Result<String> {
    Ok(file
        .canonicalize()
        .with_context(|| format!("canonicalize steering document {}", file.display()))?
        .display()
        .to_string())
}

fn project_root(file: &Path) -> Option<std::path::PathBuf> {
    agent_doc_fs::find_project_root(file)
}

fn ledger_key(document: &str) -> String {
    format!(
        "{LIVE_TURN_FOLLOW_UP_KEY_PREFIX}{}",
        agent_doc_hash::content_hash(document)
    )
}

fn base_key(document: &str) -> String {
    format!(
        "{MIDTURN_STEERING_BASE_PREFIX}{}",
        agent_doc_hash::content_hash(document)
    )
}

fn surrogate_key(token: &str) -> String {
    format!("{STEERING_SURROGATE_KEY_PREFIX}{token}")
}

fn wall_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let file = root.join("plan.md");
        std::fs::write(&file, "# S\n").unwrap();
        (dir, file)
    }

    fn seed_cycle(file: &Path, cycle_id: &str) {
        let root = project_root(file).unwrap();
        let conn = open_state_db(&root).unwrap();
        let document = canonical_document(file).unwrap();
        let payload = serde_json::json!({ "cycle_id": cycle_id, "baseline": "# S\n" }).to_string();
        upsert_project_runtime_state_in_db(&conn, &base_key(&document), &payload, 0).unwrap();
    }

    fn token_of(notice: &str) -> &str {
        notice
            .split("--token '")
            .nth(1)
            .and_then(|tail| tail.split('\'').next())
            .expect("notice carries a quoted token")
    }

    fn payload_of(file: &Path, token: &str) -> String {
        let root = project_root(file).unwrap();
        let conn = open_state_db(&root).unwrap();
        let raw = load_project_runtime_state_from_db(&conn, &surrogate_key(token))
            .unwrap()
            .unwrap();
        serde_json::from_str::<SteeringSurrogate>(&raw)
            .unwrap()
            .payload
    }

    fn expires_of(file: &Path, token: &str) -> u64 {
        let root = project_root(file).unwrap();
        let conn = open_state_db(&root).unwrap();
        let raw = load_project_runtime_state_from_db(&conn, &surrogate_key(token))
            .unwrap()
            .unwrap();
        serde_json::from_str::<SteeringSurrogate>(&raw)
            .unwrap()
            .expires_at_ms
    }

    #[test]
    fn pool_then_steering_share_one_token_until_the_reference_is_consumed() {
        let (_dir, file) = project();
        seed_cycle(&file, "cycle-1");
        assert_eq!(
            current_steering_cycle_id(&file).unwrap().as_deref(),
            Some("cycle-1")
        );

        let pool = stage_live_turn_follow_up(
            &file,
            "cycle-1",
            LiveTurnFollowUpSlot::Pool,
            "pool body",
            1_000,
        )
        .unwrap();
        let steering = stage_live_turn_follow_up(
            &file,
            "cycle-1",
            LiveTurnFollowUpSlot::Steering,
            "steer body",
            1_500,
        )
        .unwrap();
        let token = token_of(&pool.notice).to_string();
        assert_eq!(token_of(&steering.notice), token);
        assert!(pool.needs_queue_paste);
        assert!(steering.needs_queue_paste);
        let payload = payload_of(&file, &token);
        assert!(
            payload
                .starts_with(agent_doc_controller::live_turn_follow_up::LIVE_TURN_FOLLOW_UP_MARKER)
        );
        assert!(payload.find("steer body").unwrap() < payload.find("pool body").unwrap());

        mark_live_turn_follow_up_queued(&file, "cycle-1").unwrap();
        let later = stage_live_turn_follow_up(
            &file,
            "cycle-1",
            LiveTurnFollowUpSlot::Pool,
            "pool next",
            2_000,
        )
        .unwrap();
        assert_eq!(token_of(&later.notice), token);
        assert!(!later.needs_queue_paste);
        let payload = payload_of(&file, &token);
        assert!(payload.contains("pool next"), "{payload}");
        assert!(!payload.contains("pool body"), "{payload}");

        clear_project_runtime_state_in_db(
            &open_state_db(&project_root(&file).unwrap()).unwrap(),
            &surrogate_key(&token),
        )
        .unwrap();
        let consumed = stage_live_turn_follow_up(
            &file,
            "cycle-1",
            LiveTurnFollowUpSlot::Steering,
            "after take",
            2_500,
        )
        .unwrap();
        assert_ne!(token_of(&consumed.notice), token);
        assert!(consumed.needs_queue_paste);
        let payload = payload_of(&file, token_of(&consumed.notice));
        assert!(payload.contains("pool next"), "{payload}");
        assert!(payload.contains("after take"), "{payload}");
        assert!(!payload.contains("steer body"), "{payload}");
    }

    #[test]
    fn one_slot_payload_stays_the_raw_text_and_a_new_cycle_resets_it() {
        let (_dir, file) = project();
        let first = stage_live_turn_follow_up(
            &file,
            "cycle-1",
            LiveTurnFollowUpSlot::Steering,
            "secret payload",
            100,
        )
        .unwrap();
        assert_eq!(payload_of(&file, token_of(&first.notice)), "secret payload");

        let reset = stage_live_turn_follow_up(
            &file,
            "cycle-2",
            LiveTurnFollowUpSlot::Steering,
            "only the new cycle",
            200,
        )
        .unwrap();
        assert_ne!(token_of(&reset.notice), token_of(&first.notice));
        assert_eq!(
            payload_of(&file, token_of(&reset.notice)),
            "only the new cycle"
        );
        assert!(reset.needs_queue_paste);
    }

    #[test]
    fn an_expired_reference_mints_a_new_token() {
        let (_dir, file) = project();
        let first = stage_live_turn_follow_up(
            &file,
            "cycle-1",
            LiveTurnFollowUpSlot::Pool,
            "pool body",
            100,
        )
        .unwrap();
        let again = stage_live_turn_follow_up(
            &file,
            "cycle-1",
            LiveTurnFollowUpSlot::Pool,
            "pool later",
            100 + STEERING_SURROGATE_TTL_MS,
        )
        .unwrap();
        assert_ne!(token_of(&again.notice), token_of(&first.notice));
        assert!(again.needs_queue_paste);
        assert_eq!(payload_of(&file, token_of(&again.notice)), "pool later");
    }

    #[test]
    fn refresh_extends_the_open_cycle_and_stops_after_the_cycle_changes() {
        let (_dir, file) = project();
        seed_cycle(&file, "cycle-1");
        let staged = stage_live_turn_follow_up(
            &file,
            "cycle-1",
            LiveTurnFollowUpSlot::Pool,
            "pool body",
            1_000,
        )
        .unwrap();
        let token = token_of(&staged.notice).to_string();
        mark_live_turn_follow_up_queued(&file, "cycle-1").unwrap();

        let refreshed_at = 50_000;
        refresh_outstanding_live_turn_follow_up(&file, refreshed_at).unwrap();
        assert_eq!(
            expires_of(&file, &token),
            refreshed_at + STEERING_SURROGATE_TTL_MS
        );

        seed_cycle(&file, "cycle-2");
        refresh_outstanding_live_turn_follow_up(&file, 80_000).unwrap();
        assert_eq!(
            expires_of(&file, &token),
            refreshed_at + STEERING_SURROGATE_TTL_MS
        );
    }
}
