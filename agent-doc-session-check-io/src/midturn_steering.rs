//! Mid-turn operator steering delivery (`#midturn-steering`).
//!
//! The deterministic core lives in
//! [`agent_doc_document_realtime::midturn_steering`]; this module owns the
//! durable watermark, the cheap observation gate, and the harness hook
//! envelope.
//!
//! **Why a stateless compare and not a Lazily actor edge.** The delivery
//! surface is a harness `PostToolUse` hook: a one-shot process spawned after
//! every tool call, with no long-lived scope of its own and a hard latency
//! budget. Joining the controller's `TurnScope` would cost an IPC round trip
//! per tool call and couple every tool call to controller liveness. So the
//! hook is the narrow actorless boundary `#lazily-reactive-first` allows: it
//! compares the document against durable, cycle-scoped state (the watermark
//! preflight seeds in `state.db` when it admits the cycle) and writes the
//! advanced watermark back. The cycle itself is consulted only on the rare
//! path where something is ready to surface, which is also where a closed or
//! superseded cycle silences the watermark for good.
//!
//! Storage: `state.db` `project_runtime_state`, one base record per document
//! (seeded by preflight) plus one progress record per consumer (`hook`, `cli`,
//! `follow`), so a polling CLI never steals steering from the in-turn hook.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Serialize;

use agent_doc_document_realtime::midturn_steering::{
    self as core, DEFAULT_STEERING_DEBOUNCE_MS, ObserveContext, SteeringItem, SteeringWatermark,
};

const KEY_PREFIX: &str = "midturn_steering";

/// The in-turn harness hook consumer.
pub const CONSUMER_HOOK: &str = "hook";
/// `agent-doc steering <FILE>` polling consumer.
pub const CONSUMER_CLI: &str = "cli";
/// `agent-doc steering --follow <FILE>` stream consumer.
pub const CONSUMER_FOLLOW: &str = "follow";

/// What one observation produced for a consumer.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SteeringReport {
    pub document: String,
    pub cycle_id: String,
    pub items: Vec<SteeringItem>,
    pub pending: usize,
}

impl SteeringReport {
    /// Agent-facing context, `None` when nothing is ready.
    pub fn render(&self) -> Option<String> {
        core::render_steering_context(&self.document, &self.items, self.pending)
    }
}

fn document_key(file: &Path) -> String {
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    agent_doc_hash::content_hash(&canonical.display().to_string())
}

fn state_key(kind: &str, file: &Path) -> String {
    format!("{KEY_PREFIX}:{kind}:{}", document_key(file))
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn project_root(file: &Path) -> Option<PathBuf> {
    agent_doc_fs::find_project_root(file)
}

/// Seed the cycle watermark from preflight's admitted document.
///
/// `current_item` is the queue prompt this cycle selected (if any) and
/// `session_presets` the preset names this cycle's prompt requested; both
/// scope later classification. Re-seeding the same cycle (re-entrant
/// preflight) replaces the base, and consumers re-initialize because their
/// recorded baseline hash no longer matches.
pub fn seed_for_cycle(
    file: &Path,
    cycle_id: &str,
    baseline: &str,
    current_item: Option<&str>,
    session_presets: Vec<String>,
) -> Result<()> {
    let Some(root) = project_root(file) else {
        return Ok(());
    };
    let watermark = SteeringWatermark::seed(cycle_id, baseline, current_item, session_presets);
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
        &conn,
        &state_key("base", file),
        &serde_json::to_string(&watermark)?,
        now_ms(),
    )
}

fn load_watermark(
    conn: &agent_doc_sqlite::state_store::Connection,
    key: &str,
) -> Result<Option<SteeringWatermark>> {
    agent_doc_sqlite::state_store::load_project_runtime_state_from_db(conn, key)?
        .map(|raw| serde_json::from_str(&raw).context("parse mid-turn steering watermark"))
        .transpose()
}

fn stat_fingerprint(meta: &std::fs::Metadata) -> String {
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    format!("{mtime}:{}", meta.len())
}

fn mtime_ms(meta: &std::fs::Metadata) -> Option<u64> {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
}

/// Resolve the debounce window: frontmatter, then project config, then default.
pub fn debounce_ms_for(file: &Path, content: &str) -> u64 {
    let frontmatter = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(fm, _)| fm.steering_debounce_ms);
    resolve_debounce_ms(
        frontmatter,
        agent_doc_project_config_io::load_project_for_doc(file).agent_doc_steering_debounce_ms,
    )
}

/// Pure precedence for [`debounce_ms_for`].
pub fn resolve_debounce_ms(frontmatter: Option<u64>, project: Option<u64>) -> u64 {
    frontmatter
        .or(project)
        .unwrap_or(DEFAULT_STEERING_DEBOUNCE_MS)
}

/// Whether the cycle still accepts mid-turn steering: the same cycle, open,
/// and no response captured yet. After capture, closeout `session-check` owns
/// steering (`realtime_steering_closeout_guidance`).
fn cycle_accepts_steering(cycle: &agent_doc_cycle_state_io::CycleState, cycle_id: &str) -> bool {
    cycle.cycle_id == cycle_id
        && matches!(cycle.phase, agent_doc_turn::CyclePhase::PreflightStarted)
        && cycle.capture_id.is_none()
        && cycle.response_sha256.is_none()
}

fn binary_owned_ids(cycle: &agent_doc_cycle_state_io::CycleState) -> BTreeSet<String> {
    cycle
        .pending_actionable_ids
        .iter()
        .chain(cycle.pending_added_ids.iter())
        .chain(cycle.requested_added_ids.iter())
        .map(|id| id.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|id| !id.is_empty())
        .collect()
}

/// Observe `file` for `consumer`. Returns `Ok(None)` when there is no active
/// cycle watermark, the cycle closed, or the document is unchanged; otherwise
/// a report (possibly with zero ready items while edits are still settling).
///
/// When `advance` is false the watermark is not written (a peek).
pub fn observe(file: &Path, consumer: &str, advance: bool) -> Result<Option<SteeringReport>> {
    let Some(root) = project_root(file) else {
        return Ok(None);
    };
    if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
        return Ok(None);
    }
    let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
    let Some(base) = load_watermark(&conn, &state_key("base", file))? else {
        return Ok(None);
    };
    let consumer_key = state_key(consumer, file);
    let mut watermark = match load_watermark(&conn, &consumer_key)? {
        Some(existing)
            if existing.cycle_id == base.cycle_id && existing.baseline == base.baseline =>
        {
            existing
        }
        _ => base,
    };
    if watermark.closed {
        return Ok(None);
    }
    let persist = |watermark: &SteeringWatermark| -> Result<()> {
        if advance {
            agent_doc_sqlite::state_store::upsert_project_runtime_state_in_db(
                &conn,
                &consumer_key,
                &serde_json::to_string(watermark)?,
                now_ms(),
            )?;
        }
        Ok(())
    };

    // Hot-path gate: unchanged file stat and nothing settling → no read.
    let meta = std::fs::metadata(file).with_context(|| format!("stat {}", file.display()))?;
    let fingerprint = stat_fingerprint(&meta);
    if watermark.pending.is_empty()
        && watermark.last_observed_stat.as_deref() == Some(fingerprint.as_str())
    {
        return Ok(None);
    }
    let content =
        std::fs::read_to_string(file).with_context(|| format!("read {}", file.display()))?;
    if watermark.pending.is_empty()
        && watermark.last_observed_content_hash.as_deref()
            == Some(core::content_hash(&content).as_str())
    {
        watermark.last_observed_stat = Some(fingerprint);
        persist(&watermark)?;
        return Ok(None);
    }

    let debounce_ms = debounce_ms_for(file, &content);
    let empty = BTreeSet::new();
    let ctx = ObserveContext {
        now_ms: now_ms(),
        document_changed_ms: mtime_ms(&meta),
        debounce_ms,
        binary_owned_queue_ids: &empty,
    };
    let mut observation = core::observe(&watermark, &content, &ctx);
    if !observation.ready.is_empty() {
        // Rare path: confirm the cycle is still open before handing anything
        // to the agent, and exclude this cycle's own queue bookkeeping.
        let cycle = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
        match cycle {
            Some(cycle) if cycle_accepts_steering(&cycle, &watermark.cycle_id) => {
                let owned = binary_owned_ids(&cycle);
                if !owned.is_empty() {
                    observation = core::observe(
                        &watermark,
                        &content,
                        &ObserveContext {
                            binary_owned_queue_ids: &owned,
                            ..ctx
                        },
                    );
                }
            }
            _ => {
                watermark.closed = true;
                persist(&watermark)?;
                return Ok(None);
            }
        }
    }
    let mut next = observation.next;
    next.last_observed_stat = Some(fingerprint);
    persist(&next)?;
    if advance && !observation.ready.is_empty() {
        let dispatches = observation
            .ready
            .iter()
            .map(|item| item.dispatch.as_str())
            .collect::<Vec<_>>()
            .join(",");
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "midturn_steering_surfaced file={} consumer={consumer} cycle={} count={} pending={} dispatch={dispatches}",
                file.display(),
                next.cycle_id,
                observation.ready.len(),
                observation.pending,
            ),
        );
    }
    Ok(Some(SteeringReport {
        document: file.display().to_string(),
        cycle_id: next.cycle_id,
        items: observation.ready,
        pending: observation.pending,
    }))
}

/// Harness `PostToolUse` payload fields this hook reads. Claude Code and Codex
/// both send `session_id` and `cwd`.
#[derive(Debug, Clone, serde::Deserialize)]
pub struct PostToolUseInput {
    pub session_id: String,
    pub cwd: String,
}

/// Resolve the document this harness session is driving, from the binding
/// the `UserPromptSubmit` hook recorded.
pub fn session_document(input: &PostToolUseInput) -> Result<Option<PathBuf>> {
    let roots = agent_doc_codex_hook_io::project_roots_for(Path::new(&input.cwd));
    if roots.is_empty() {
        return Ok(None);
    }
    if !roots
        .iter()
        .any(|root| agent_doc_sqlite::state_store::state_db_path(root).exists())
    {
        return Ok(None);
    }
    let Some((_, state)) = agent_doc_codex_hook_io::load_state_any(&roots, &input.session_id)?
    else {
        return Ok(None);
    };
    if state.preflight_admitted == Some(false) {
        return Ok(None);
    }
    let doc = PathBuf::from(&state.doc_path);
    Ok(doc.is_file().then_some(doc))
}

/// The hook envelope Claude Code and Codex inject into the running turn.
pub fn post_tool_use_output(context: &str) -> serde_json::Value {
    serde_json::json!({
        "hookSpecificOutput": {
            "hookEventName": "PostToolUse",
            "additionalContext": context,
        }
    })
}

/// Pure hook decision: the JSON to print, or `None` for silence.
pub fn post_tool_use_response(payload: &str) -> Result<Option<serde_json::Value>> {
    let input: PostToolUseInput =
        serde_json::from_str(payload).context("parse PostToolUse payload")?;
    let Some(file) = session_document(&input)? else {
        return Ok(None);
    };
    let report = match observe(&file, CONSUMER_HOOK, true) {
        Ok(report) => report,
        Err(err) => {
            agent_doc_ops_log_io::log_op(
                &file,
                &format!(
                    "midturn_steering_hook_error file={} error={err:#}",
                    file.display()
                ),
            );
            return Err(err);
        }
    };
    Ok(report
        .and_then(|report| report.render())
        .map(|context| post_tool_use_output(&context)))
}

/// `agent-doc hook steering-post-tool-use` entry point. Never fails the tool
/// call: every error is reported on stderr (and ops.log when the document is
/// known) and the hook prints nothing.
pub fn handle_post_tool_use() -> Result<()> {
    use std::io::Read;
    let mut payload = String::new();
    if let Err(err) = std::io::stdin().read_to_string(&mut payload) {
        eprintln!("[agent-doc] steering hook payload read failed: {err}");
        return Ok(());
    }
    match post_tool_use_response(&payload) {
        Ok(Some(output)) => println!("{output}"),
        Ok(None) => {}
        Err(err) => eprintln!("[agent-doc] steering hook skipped: {err:#}"),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hook_envelope_matches_the_harness_contract() {
        let value = post_tool_use_output("steer");
        assert_eq!(value["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        assert_eq!(value["hookSpecificOutput"]["additionalContext"], "steer");
    }

    #[test]
    fn debounce_precedence_is_frontmatter_then_project_then_default() {
        assert_eq!(resolve_debounce_ms(Some(10), Some(20)), 10);
        assert_eq!(resolve_debounce_ms(None, Some(20)), 20);
        assert_eq!(
            resolve_debounce_ms(None, None),
            DEFAULT_STEERING_DEBOUNCE_MS
        );
    }

    #[test]
    fn hook_is_silent_outside_an_agent_doc_project() {
        let dir = tempfile::tempdir().unwrap();
        let payload = serde_json::json!({
            "session_id": "s1",
            "cwd": dir.path(),
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
        })
        .to_string();
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);
    }

    #[test]
    fn seeded_cycle_surfaces_once_then_stays_silent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        seed_for_cycle(&file, "cycle-1", baseline, Some("current task"), Vec::new()).unwrap();

        // Unchanged document: silent, no cycle lookup needed.
        assert_eq!(observe(&file, CONSUMER_CLI, true).unwrap(), None);

        // A peek at a change never needs the cycle when nothing is ready yet;
        // with a ready item and no open cycle the watermark closes silently.
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- new work\n"),
        )
        .unwrap();
        assert_eq!(observe(&file, CONSUMER_CLI, true).unwrap(), None);
        // Closed watermark stays silent afterwards.
        assert_eq!(observe(&file, CONSUMER_CLI, true).unwrap(), None);
    }

    #[test]
    fn open_cycle_surfaces_steering_once_through_the_hook_consumer() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let file = dir.path().join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        std::fs::write(
            &file,
            baseline.replace("- current task\n", "- current task\n- new work\n"),
        )
        .unwrap();
        let report = observe(&file, CONSUMER_HOOK, true)
            .unwrap()
            .expect("report");
        assert_eq!(report.items.len(), 1);
        assert_eq!(report.items[0].verbatim, "new work");
        let context = report.render().unwrap();
        assert!(
            context.contains("dispatch=drain_after_current"),
            "{context}"
        );
        // Exactly once: the same document never re-injects.
        assert_eq!(observe(&file, CONSUMER_HOOK, true).unwrap(), None);
        // Consumers are independent: the CLI still sees it once.
        assert_eq!(
            observe(&file, CONSUMER_CLI, true)
                .unwrap()
                .unwrap()
                .items
                .len(),
            1
        );
    }

    #[test]
    fn post_tool_use_hook_emits_the_harness_envelope_then_stays_silent() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let root = dir.path().canonicalize().unwrap();
        let file = root.join("plan.md");
        let baseline = "---\nagent_doc_steering_debounce_ms: 0\n---\n# S\n\n<!-- agent:queue -->\n- current task\n<!-- /agent:queue -->\n";
        std::fs::write(&file, baseline).unwrap();
        agent_doc_codex_hook_io::save_state(
            &root,
            &agent_doc_codex_hook_io::SessionState {
                session_id: "sess-1".into(),
                identity_origin: agent_doc_codex_hook_io::SessionIdentityOrigin::HarnessHook,
                doc_path: file.display().to_string(),
                last_turn_id: String::new(),
                last_prompt: "agent-doc plan.md".into(),
                last_auto_queue_head: None,
                last_context_clear_at: None,
                last_prompt_cycle: None,
                preflight_admitted: None,
                updated_at: 1,
            },
        )
        .unwrap();
        let payload = serde_json::json!({
            "session_id": "sess-1",
            "cwd": root,
            "hook_event_name": "PostToolUse",
            "tool_name": "Bash",
            "tool_input": {"command": "cargo build"},
            "tool_response": {"stdout": "ok"},
        })
        .to_string();
        // No seeded cycle yet: silent.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);

        let cycle =
            agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(baseline))
                .unwrap();
        seed_for_cycle(
            &file,
            &cycle.cycle_id,
            baseline,
            Some("current task"),
            Vec::new(),
        )
        .unwrap();
        // Seeded, nothing new: silent.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);

        std::fs::write(
            &file,
            baseline.replace(
                "- current task\n",
                "- current task\n- #subagents fix issue 111\n",
            ),
        )
        .unwrap();
        let output = post_tool_use_response(&payload)
            .unwrap()
            .expect("steering output");
        assert_eq!(output["hookSpecificOutput"]["hookEventName"], "PostToolUse");
        let context = output["hookSpecificOutput"]["additionalContext"]
            .as_str()
            .unwrap();
        assert!(context.starts_with(core::STEERING_MARKER), "{context}");
        assert!(context.contains("dispatch=subagent"), "{context}");
        assert!(context.contains("#subagents fix issue 111"), "{context}");
        // Exactly once.
        assert_eq!(post_tool_use_response(&payload).unwrap(), None);
    }
}
