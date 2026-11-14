//! Side effects of a harness subagent-end (`#workerquotaautodetect`).
//!
//! The ledger write is the effect the parent sees on its next read. Hook JSON
//! never uses `decision: "block"`: that would make the exhausted subagent keep
//! going. Quota ends become quota-blocked. A worker that stopped its turn while
//! its own background work still runs is alive (`#waitingfreezelive`): Claude
//! Code tells the parent it "is waiting on its own background work" and
//! resumes it when that work reports, so the stop records a heartbeat and
//! clears an earlier freeze of that generation. Every other bound worker end
//! freezes that exact generation until the coordinator integrates, resumes, or
//! explicitly abandons it. A repeated Grok Stop plus `SubagentStop` is quiet.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use agent_doc_harness::background_work::{
    OwnBackgroundWork, WorkerBackgroundLaunches, own_background_work,
};
use agent_doc_harness::subagent_end::{
    SubagentEnd, SubagentEndParse, parse_subagent_end, usage_limit_from_end,
};
use agent_doc_harness::usage_limit::HarnessUsageLimit;
use agent_doc_queue::worker_quota::{WorkerQuotaBlock, quota_blocked_next_step};
pub use agent_doc_queue_io::worker_quota_autodetect::SubagentQuotaBlockApplied;
use agent_doc_queue_io::worker_quota_autodetect::{
    SubagentFreezeAutodetect, SubagentLivenessAutodetect, SubagentQuotaAutodetect,
    apply_subagent_freeze_autodetect, apply_subagent_liveness_autodetect,
    apply_subagent_quota_autodetect,
};
use anyhow::Result;
use serde_json::{Value, json};

const TRANSCRIPT_TAIL_BYTES: u64 = 64 * 1024;

pub(crate) fn observe_payload(payload: &str) -> Result<Option<SubagentQuotaBlockApplied>> {
    let value = serde_json::from_str::<Value>(payload).unwrap_or(Value::Null);
    observe_value(&value)
}

pub(crate) fn observe_value(value: &Value) -> Result<Option<SubagentQuotaBlockApplied>> {
    let signal = match parse_subagent_end(value) {
        Ok(signal) => signal,
        Err(SubagentEndParse::NotASubagentEnd) => return Ok(None),
        Err(SubagentEndParse::MissingWorkerId) => {
            eprintln!(
                "[agent-doc] subagent-end hook had no worker id (`subagentId` or `agent_id`); the claim was left unchanged."
            );
            return Ok(None);
        }
    };
    let tail = signal
        .transcript_path
        .as_deref()
        .and_then(|path| read_transcript_tail(Path::new(path)));
    let limit = usage_limit_from_end(
        signal.last_message.as_deref(),
        tail.as_deref(),
        signal.structured_reason.as_deref(),
    );
    if limit.is_none() {
        return observe_normal_end(&signal);
    }
    let limit = limit.expect("checked above");
    let block = worker_block(&signal, &limit);
    let scan_paths = scan_paths(&signal);
    let preferred = preferred_session_doc(&signal, &scan_paths);
    match apply_subagent_quota_autodetect(
        &scan_paths,
        preferred.as_deref(),
        &signal.worker_id,
        block,
    )? {
        SubagentQuotaAutodetect::Applied(applied) if applied.announce => Ok(Some(*applied)),
        SubagentQuotaAutodetect::Applied(_) | SubagentQuotaAutodetect::NoMatch => Ok(None),
        SubagentQuotaAutodetect::Ambiguous => {
            eprintln!(
                "[agent-doc] subagent-end quota autodetect matched worker `{}` on more than one claim with the same start; no claim was changed.",
                signal.worker_id
            );
            Ok(None)
        }
    }
}

fn observe_normal_end(signal: &SubagentEnd) -> Result<Option<SubagentQuotaBlockApplied>> {
    let scan_paths = scan_paths(signal);
    let preferred = preferred_session_doc(signal, &scan_paths);
    if let Some(work) = waiting_on_own_background_work(signal) {
        observe_waiting_end(signal, &work, &scan_paths, preferred.as_deref())?;
        return Ok(None);
    }
    let reason = normal_end_reason(signal);
    match apply_subagent_freeze_autodetect(
        &scan_paths,
        preferred.as_deref(),
        &signal.worker_id,
        &reason,
    )? {
        SubagentFreezeAutodetect::Applied(applied) if applied.announce => {
            eprintln!(
                "[agent-doc] froze generation {} for ended worker `{}`; the coordinator may report its handback, resume it, or explicitly abandon it.",
                applied.claim.generation, signal.worker_id
            );
        }
        SubagentFreezeAutodetect::Applied(_) | SubagentFreezeAutodetect::NoMatch => {}
        SubagentFreezeAutodetect::Ambiguous => eprintln!(
            "[agent-doc] subagent-end freeze matched worker `{}` on more than one claim with the same start; no claim was changed.",
            signal.worker_id
        ),
    }
    Ok(None)
}

/// The worker's own background work still in flight at this stop
/// (`#waitingfreezelive`). The payload's `background_tasks` is session-wide,
/// so only the worker's own `agent_transcript_path` attributes it; without
/// that transcript the stop is not proof of life.
fn waiting_on_own_background_work(signal: &SubagentEnd) -> Option<OwnBackgroundWork> {
    let mut launches = signal
        .agent_transcript_path
        .as_deref()
        .and_then(|path| read_background_launches(Path::new(path)))?;
    launches.launched.retain(|id| *id != signal.worker_id);
    own_background_work(signal.background_tasks.as_deref(), Some(&launches))
}

/// Stream the whole transcript: a launch can sit far above the tail, and only
/// the few lines that can name a background task are parsed.
fn read_background_launches(path: &Path) -> Option<WorkerBackgroundLaunches> {
    let file = std::fs::File::open(path).ok()?;
    let mut launches = WorkerBackgroundLaunches::default();
    for line in BufReader::new(file).split(b'\n') {
        let line = line.ok()?;
        let line = String::from_utf8_lossy(&line);
        launches.observe_line(&line);
    }
    Some(launches)
}

fn observe_waiting_end(
    signal: &SubagentEnd,
    work: &OwnBackgroundWork,
    scan_paths: &[PathBuf],
    preferred: Option<&Path>,
) -> Result<()> {
    let ids = work.task_ids().join(", ");
    match apply_subagent_liveness_autodetect(scan_paths, preferred, &signal.worker_id) {
        Ok(SubagentLivenessAutodetect::Applied(applied)) => eprintln!(
            "[agent-doc] worker `{}` stopped its turn while its own background work ({ids}) is still running; that is proof of life, so generation {} keeps its claim and slot{} (#waitingfreezelive).",
            signal.worker_id,
            applied.claim.generation,
            if applied.cleared_freeze {
                " and the earlier freeze of it was cleared"
            } else {
                ""
            },
        ),
        Ok(SubagentLivenessAutodetect::NoMatch) => {}
        Ok(SubagentLivenessAutodetect::Ambiguous) => eprintln!(
            "[agent-doc] subagent-end proof of life matched worker `{}` on more than one claim with the same start; no claim was changed.",
            signal.worker_id
        ),
        // A refused heartbeat (stale generation, abandoned or reported
        // attempt) leaves the ledger as it was. It must not fall through to a
        // freeze: the worker is still running.
        Err(err) => eprintln!(
            "[agent-doc] worker `{}` is waiting on its own background work ({ids}); its claim was left unchanged: {err:#}",
            signal.worker_id
        ),
    }
    Ok(())
}

/// `#lostrowreportreopen`: the format is shared with the report path, which
/// reads the recorded hand-back back out of this freeze.
fn normal_end_reason(signal: &SubagentEnd) -> String {
    agent_doc_queue::queue_claim::subagent_end_freeze_reason(
        signal
            .last_message
            .as_deref()
            .or(signal.structured_reason.as_deref()),
    )
}

pub(crate) fn subagent_stop_from_payload(
    payload: &str,
) -> Result<(Value, Option<SubagentQuotaBlockApplied>)> {
    let value = match serde_json::from_str::<Value>(payload) {
        Ok(value) => value,
        Err(err) => {
            eprintln!(
                "[agent-doc] subagent-end hook payload is not JSON ({err}); allowing the stop."
            );
            return Ok((json!({}), None));
        }
    };
    let applied = observe_value(&value)?;
    let output = match &applied {
        Some(applied) => announced_hook_output(applied),
        None => json!({}),
    };
    Ok((output, applied))
}

pub fn handle_subagent_stop() -> Result<Option<SubagentQuotaBlockApplied>> {
    let payload = match super::read_stdin_payload() {
        Ok(payload) => payload,
        Err(err) => {
            eprintln!(
                "[agent-doc] subagent-end hook could not read its payload: {err:#}. Allowing the stop."
            );
            println!("{{}}");
            return Ok(None);
        }
    };
    let (output, applied) = match subagent_stop_from_payload(&payload) {
        Ok(result) => result,
        Err(err) => {
            eprintln!(
                "[agent-doc] subagent-end claim preservation failed: {err:#}. Allowing the stop."
            );
            (json!({}), None)
        }
    };
    println!("{}", serde_json::to_string(&output)?);
    Ok(applied)
}

pub(crate) fn announced_hook_output(applied: &SubagentQuotaBlockApplied) -> Value {
    subagent_stop_output(&quota_blocked_next_step(
        &applied.file.display().to_string(),
        &applied.claim,
    ))
}

fn subagent_stop_output(step: &str) -> Value {
    json!({
        "hookSpecificOutput": {
            "hookEventName": "SubagentStop",
            "additionalContext": step,
        }
    })
}

fn worker_block(signal: &SubagentEnd, limit: &HarnessUsageLimit) -> WorkerQuotaBlock {
    WorkerQuotaBlock {
        harness: limit.harness.clone(),
        limit: limit.limit.clone(),
        evidence: limit.evidence.clone(),
        reset_hint: limit.reset_hint.clone(),
        worktree: signal.worktree.clone(),
        branch: signal.branch.clone(),
        blocked_at_secs: 0,
    }
}

fn scan_paths(signal: &SubagentEnd) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for path in [
        signal.cwd.as_deref(),
        signal.workspace_root.as_deref(),
        signal.transcript_path.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        paths.push(PathBuf::from(path));
    }
    if let Ok(cwd) = std::env::current_dir() {
        paths.push(cwd);
    }
    paths
}

fn preferred_session_doc(signal: &SubagentEnd, scan_paths: &[PathBuf]) -> Option<PathBuf> {
    let session = signal
        .parent_session_id
        .as_deref()
        .or(signal.session_id.as_deref())?;
    let mut roots = Vec::new();
    for path in scan_paths {
        for root in agent_doc_codex_hook_io::project_roots_for(path) {
            agent_doc_codex_hook_io::push_unique_root(&mut roots, root);
        }
    }
    let (_root, state) = agent_doc_codex_hook_io::load_state_any(&roots, session).ok()??;
    let path = PathBuf::from(state.doc_path);
    path.is_file().then_some(path)
}

fn read_transcript_tail(path: &Path) -> Option<String> {
    let mut file = std::fs::File::open(path).ok()?;
    let len = file.metadata().ok()?.len();
    let start = len.saturating_sub(TRANSCRIPT_TAIL_BYTES);
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    file.read_to_end(&mut buf).ok()?;
    if start > 0
        && let Some(split) = buf.iter().position(|byte| *byte == b'\n')
    {
        buf.drain(..=split);
    }
    Some(String::from_utf8_lossy(&buf).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_dual_spelling_payload_without_a_bound_claim_allows_the_stop() {
        let payload = r#"{
            "hookEventName": "subagent_stop",
            "hook_event_name": "SubagentStop",
            "subagentId": "worker-sdk",
            "subagent_id": "worker-sdk",
            "subagentType": "general",
            "lastAssistantMessage": "Done.",
            "last_assistant_message": "Done."
        }"#;
        let (output, applied) = subagent_stop_from_payload(payload).unwrap();
        assert!(applied.is_none());
        assert_eq!(output, json!({}));
        assert!(output.get("decision").is_none());
    }

    #[test]
    fn a_still_running_child_is_recorded_as_a_nonterminal_wrapper_exit() {
        let signal = parse_subagent_end(&json!({
            "hook_event_name": "SubagentStop",
            "agent_id": "worker-sdk",
            "last_assistant_message": "Task task-mwbqehs0-m7oon3 is still running. Use /codex:status before continuing it."
        }))
        .unwrap();
        assert_eq!(
            normal_end_reason(&signal),
            "wrapper exited while a child task was still running: Task task-mwbqehs0-m7oon3 is still running. Use /codex:status before continuing it."
        );
    }

    #[test]
    fn a_subagent_end_freezes_the_bound_claim_before_the_parent_sees_the_handback() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(
            &doc,
            "# task\n\n<!-- agent:queue -->\n- finish sdk\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&doc, "finish sdk", "subagent:sdk", 600)
                .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "finish sdk",
            "subagent:sdk",
            "worker-sdk",
            600,
            claim.generation,
        )
        .unwrap();

        let payload = json!({
            "hook_event_name": "SubagentStop",
            "agent_id": "worker-sdk",
            "last_assistant_message": "Task task-mwbqehs0-m7oon3 is still running. Use /codex:status before continuing it.",
            "cwd": dir.path(),
        })
        .to_string();
        let (output, quota) = subagent_stop_from_payload(&payload).unwrap();
        assert_eq!(output, json!({}));
        assert!(quota.is_none());

        let stored = agent_doc_queue_io::queue_claim::load_ledger(&doc).unwrap();
        let claim = &stored.claims[0];
        assert_eq!(claim.generation, 1);
        assert_eq!(claim.worker.as_ref().unwrap().worker_id, "worker-sdk");
        assert!(
            claim
                .frozen
                .as_ref()
                .unwrap()
                .reason
                .contains("child task was still running")
        );

        let rebound = agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "finish sdk",
            "subagent:sdk",
            "worker-sdk-resume",
            600,
            claim.generation,
        )
        .unwrap();
        assert_eq!(rebound.generation, claim.generation);
        assert_eq!(
            rebound.worker.as_ref().unwrap().worker_id,
            "worker-sdk-resume"
        );
        assert!(rebound.frozen.is_none());
    }

    /// A started `#gh410`-shaped attempt plus the worker's transcript, in which
    /// it backgrounded `make tmux-ci` (task `b3rik1w2m`) before ending its turn.
    fn waiting_worker_fixture() -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("task.md");
        std::fs::write(
            &doc,
            "# task\n\n<!-- agent:queue -->\n- do [#gh410]\n<!-- /agent:queue -->\n",
        )
        .unwrap();
        let (_, claim) =
            agent_doc_queue_io::queue_claim::claim(&doc, "do [#gh410]", "subagent:gh410", 7200)
                .unwrap();
        agent_doc_queue_io::queue_claim::start_attempt(
            &doc,
            "do [#gh410]",
            "subagent:gh410",
            "a5cfda4a2bfde1278",
            7200,
            claim.generation,
        )
        .unwrap();
        let transcript = dir.path().join("agent-a5cfda4a2bfde1278.jsonl");
        let lines = [
            json!({"type": "assistant", "message": {"role": "assistant", "content": [{
                "type": "tool_use", "id": "toolu_01XUSGotTNf2YYQWHiu2YCpW", "name": "Bash",
                "input": {"command": "make tmux-ci", "run_in_background": true}}]}}),
            json!({"type": "user", "message": {"role": "user", "content": [{
                "tool_use_id": "toolu_01XUSGotTNf2YYQWHiu2YCpW", "type": "tool_result",
                "content": "Command running in background with ID: b3rik1w2m."}]},
                "toolUseResult": {"stdout": "", "stderr": "", "interrupted": false,
                                  "backgroundTaskId": "b3rik1w2m"}}),
            json!({"type": "assistant", "message": {"role": "assistant", "content": [{
                "type": "text",
                "text": "The commit is in. Waiting on `make tmux-ci` before I push."}]}}),
        ];
        let body = lines
            .iter()
            .map(Value::to_string)
            .collect::<Vec<_>>()
            .join("\n");
        std::fs::write(&transcript, body + "\n").unwrap();
        (dir, doc, transcript)
    }

    /// The `SubagentStop` Claude Code sent when it told the coordinator
    /// `Agent "Fix agent-doc GH issue 410" is waiting on its own background work`.
    fn waiting_stop_payload(dir: &Path, transcript: &Path, in_flight: &[&str]) -> String {
        let tasks = in_flight
            .iter()
            .map(|id| {
                json!({"id": id, "type": "shell", "status": "running",
                             "description": "Run make tmux-ci in worktree",
                             "command": "make tmux-ci"})
            })
            .chain(std::iter::once(json!({
                "id": "a01da8a28a9c64248", "type": "subagent", "status": "running",
                "description": "a sibling worker", "agent_type": "general-purpose"})))
            .collect::<Vec<_>>();
        json!({
            "hook_event_name": "SubagentStop",
            "stop_hook_active": false,
            "agent_id": "a5cfda4a2bfde1278",
            "agent_type": "general-purpose",
            "agent_transcript_path": transcript,
            "last_assistant_message": "The commit is in. Waiting on `make tmux-ci` before I push.",
            "background_tasks": tasks,
            "session_crons": [],
            "cwd": dir,
        })
        .to_string()
    }

    fn only_claim(doc: &Path) -> agent_doc_queue::queue_claim::QueueClaim {
        agent_doc_queue_io::queue_claim::load_ledger(doc)
            .unwrap()
            .claims
            .into_iter()
            .next()
            .expect("claim")
    }

    /// `#waitingfreezelive`: a worker that ends its turn while its own
    /// background `make tmux-ci` runs is alive. The stop records a heartbeat;
    /// it does not freeze the attempt, fail its continue, or let recovery
    /// offer the row for a duplicate dispatch.
    #[test]
    fn a_stop_waiting_on_its_own_background_work_is_proof_of_life_not_a_freeze() {
        let (dir, doc, transcript) = waiting_worker_fixture();
        let payload = waiting_stop_payload(dir.path(), &transcript, &["b3rik1w2m"]);
        let (output, quota) = subagent_stop_from_payload(&payload).unwrap();
        assert_eq!(output, json!({}));
        assert!(quota.is_none());

        let claim = only_claim(&doc);
        assert!(claim.frozen.is_none(), "{claim:#?}");
        assert_eq!(
            claim.state,
            agent_doc_queue::queue_claim::QueueClaimState::Running
        );
        assert_eq!(claim.generation, 1);
        assert!(claim.heartbeat_at_secs.is_some(), "{claim:#?}");

        let recovered =
            agent_doc_queue_io::lost_running::recover_lost_running_automatically(&doc, true)
                .unwrap();
        assert!(recovered.is_empty(), "{recovered:#?}");
        let claim = only_claim(&doc);
        assert!(claim.lost_continue.is_none(), "{claim:#?}");
        assert!(!agent_doc_queue::lost_running_recovery::retry_authorized(
            &claim
        ));
        assert!(claim.blocks_dispatch(agent_doc_queue_io::queue_claim::claim_clock_now_secs()));
    }

    /// The 2026-11-13 ledger: an older hook froze the waiting worker and the
    /// next recovery pass failed its continue ("harness freeze worker is
    /// gone"). The worker's next waiting stop is proof of life and clears both.
    #[test]
    fn a_later_waiting_stop_clears_a_freeze_an_earlier_stop_recorded() {
        let (dir, doc, transcript) = waiting_worker_fixture();
        agent_doc_queue_io::queue_claim::freeze_attempt(
            &doc,
            "do [#gh410]",
            Some(1),
            "subagent ended before coordinator integration: The commit is in. Waiting on `make tmux-ci` before I push.",
        )
        .unwrap();
        agent_doc_queue_io::lost_running::recover_lost_running_automatically(&doc, true).unwrap();
        let frozen = only_claim(&doc);
        assert!(frozen.frozen.is_some());
        assert_eq!(
            frozen
                .lost_continue
                .as_ref()
                .map(|record| record.reason.as_str()),
            Some("harness freeze worker is gone; continue refused")
        );
        assert!(agent_doc_queue::lost_running_recovery::retry_authorized(
            &frozen
        ));

        let payload = waiting_stop_payload(dir.path(), &transcript, &["b3rik1w2m"]);
        subagent_stop_from_payload(&payload).unwrap();
        let claim = only_claim(&doc);
        assert!(claim.frozen.is_none(), "{claim:#?}");
        assert!(claim.lost_continue.is_none(), "{claim:#?}");
        assert_eq!(claim.generation, 1);
        assert!(!agent_doc_queue::lost_running_recovery::retry_authorized(
            &claim
        ));
    }

    /// Only the worker's own work counts: the payload lists every in-flight
    /// task in the session. With none of its own left, the stop still freezes.
    #[test]
    fn a_stop_with_only_other_workers_background_work_still_freezes() {
        let (dir, doc, transcript) = waiting_worker_fixture();
        let payload = waiting_stop_payload(dir.path(), &transcript, &[]);
        subagent_stop_from_payload(&payload).unwrap();
        let claim = only_claim(&doc);
        assert!(
            claim
                .frozen
                .as_ref()
                .unwrap()
                .reason
                .starts_with("subagent ended before coordinator integration"),
            "{claim:#?}"
        );
    }

    /// The parent's `transcript_path` launched this very worker as an async
    /// agent, and the harness lists that worker in flight. Without the
    /// worker's own transcript nothing attributes background work to it.
    #[test]
    fn the_parent_transcript_never_proves_the_worker_alive() {
        let (dir, doc, _) = waiting_worker_fixture();
        let parent = dir.path().join("parent.jsonl");
        std::fs::write(
            &parent,
            json!({"type": "user", "toolUseResult": {
                "isAsync": true, "status": "async_launched", "agentId": "a5cfda4a2bfde1278"}})
            .to_string()
                + "\n",
        )
        .unwrap();
        let payload = json!({
            "hook_event_name": "SubagentStop",
            "agent_id": "a5cfda4a2bfde1278",
            "agent_transcript_path": "",
            "transcript_path": parent,
            "last_assistant_message": "Done.",
            "background_tasks": [{"id": "a5cfda4a2bfde1278", "type": "subagent",
                                  "status": "running", "description": "Fix #410"}],
            "cwd": dir.path(),
        })
        .to_string();
        subagent_stop_from_payload(&payload).unwrap();
        assert!(only_claim(&doc).frozen.is_some());
    }

    #[test]
    fn a_limit_with_no_matching_claim_still_allows_the_stop() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join(".agent-doc")).unwrap();
        let payload = serde_json::json!({
            "hook_event_name": "SubagentStop",
            "agent_id": "worker-sdk",
            "last_assistant_message": "You've hit your session limit · resets 9pm",
            "cwd": dir.path(),
        })
        .to_string();
        let (output, applied) = subagent_stop_from_payload(&payload).unwrap();
        assert!(applied.is_none());
        assert_eq!(output, json!({}));
    }
}
