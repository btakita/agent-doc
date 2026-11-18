//! Project a Codex coordinator's native completion event onto its exact queue attempt.

use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use agent_doc_queue::queue_claim::{
    QueueClaim, QueueClaimState, SUBAGENT_END_HANDBACK_REASON_PREFIX, subagent_end_freeze_reason,
};
use anyhow::Result;
use serde_json::Value;

use crate::sync_payload::{
    codex_thread_id_is_safe, collect_codex_rollout_candidates, recover_codex_turn_handback,
};

const TAIL_BYTES: u64 = 8 * 1024 * 1024;

struct WorkerActivity {
    worker: String,
    thread: String,
    event: String,
    kind: String,
    at_ms: u64,
}

impl WorkerActivity {
    fn parse(line: &str, coordinator: &str) -> Option<Self> {
        if !line.contains("SubAgentActivity") {
            return None;
        }
        let record: Value = serde_json::from_str(line).ok()?;
        let payload = record.get("payload")?;
        let item = payload.get("item")?;
        if record.get("type")?.as_str()? != "event_msg"
            || payload.get("type")?.as_str()? != "item_completed"
            || payload.get("thread_id")?.as_str()? != coordinator
            || payload.get("turn_id")?.as_str()?.is_empty()
            || item.get("type")?.as_str()? != "SubAgentActivity"
        {
            return None;
        }
        let thread = item.get("agent_thread_id")?.as_str()?;
        codex_thread_id_is_safe(thread).then_some(())?;
        Some(Self {
            worker: item.get("agent_path")?.as_str()?.to_owned(),
            thread: thread.to_owned(),
            event: item.get("id")?.as_str()?.to_owned(),
            kind: item.get("kind")?.as_str()?.to_owned(),
            at_ms: payload.get("completed_at_ms")?.as_u64()?,
        })
    }
}

/// Use the exact coordinator thread, worker binding, and claim-to-start
/// window. A later coordinator turn may recover the same attempt. A reused
/// path or an interaction after completion does not prove it stopped.
fn exact_completion<'a>(
    activities: &'a [WorkerActivity],
    claim: &QueueClaim,
) -> Option<(&'a str, &'a str)> {
    let binding = claim.worker.as_ref()?;
    let mut matching = activities.iter().rev().filter(|activity| {
        activity.worker == binding.worker_id || activity.thread == binding.worker_id
    });
    let completed = matching.next()?;
    if completed.kind != "completed" {
        return None;
    }
    let mut launches = matching.filter(|activity| {
        activity.kind == "started"
            && (claim.claimed_at_secs..=binding.started_at_secs).contains(&(activity.at_ms / 1000))
    });
    let launch = launches.next()?;
    if launch.thread != completed.thread
        || completed.at_ms < launch.at_ms
        || launches.next().is_some()
    {
        return None;
    }
    let turn = completed.event.strip_prefix("subagent-completed-")?;
    codex_thread_id_is_safe(turn).then_some((&completed.thread, turn))
}

/// The Codex Stop hook owns this read of native harness evidence. Queue
/// reporters and all other Stop/session-check consumers still use the same
/// durable freeze and report-owed lifecycle.
pub fn preserve_at_stop(file: &Path, coordinator: &str) -> Result<()> {
    let Some(home) = std::env::var_os("CODEX_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".codex")))
    else {
        return Ok(());
    };
    preserve_from_sessions(file, &home.join("sessions"), coordinator)
}

pub(crate) fn recover_path_bound_handback(
    sessions: &Path,
    claim: &QueueClaim,
    coordinator: &str,
) -> Result<Option<String>> {
    let activities = coordinator_activities(sessions, coordinator, None)?;
    let Some((worker_thread, worker_turn)) = exact_completion(&activities, claim) else {
        return Ok(None);
    };
    Ok(
        recover_codex_turn_handback(sessions, claim, coordinator, worker_thread, worker_turn)
            .filter(|text| terminal_freeze_reason(text).is_some()),
    )
}

fn preserve_from_sessions(file: &Path, sessions: &Path, coordinator: &str) -> Result<()> {
    if !codex_thread_id_is_safe(coordinator) {
        return Ok(());
    }
    let ledger = crate::queue_claim::load_ledger_read_only(file)?;
    let running: Vec<_> = ledger
        .claims
        .iter()
        .filter(|claim| {
            claim.state == QueueClaimState::Running
                && claim.worker.is_some()
                && claim.frozen.is_none()
                && claim.owner_session.as_ref().is_some_and(|owner| {
                    owner.harness.as_deref() == Some("codex")
                        && owner.session_id.as_deref() == Some(coordinator)
                })
        })
        .collect();
    if running.is_empty() {
        return Ok(());
    }
    let activities = coordinator_activities(sessions, coordinator, Some(TAIL_BYTES))?;

    for claim in running {
        let Some((worker_thread, worker_turn)) = exact_completion(&activities, claim) else {
            continue;
        };
        let response =
            recover_codex_turn_handback(sessions, claim, coordinator, worker_thread, worker_turn);
        let Some(response) = response else {
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "codex_worker_completion_handback_unavailable owner={} generation={} worker_thread={worker_thread} worker_turn={worker_turn}",
                    claim.owner, claim.generation
                ),
            );
            continue;
        };
        if let Some(reason) = terminal_freeze_reason(&response) {
            crate::queue_claim::freeze_bound_attempt(
                file,
                &claim.item_text,
                Some(claim.generation),
                &reason,
            )?;
        }
    }
    Ok(())
}

fn terminal_freeze_reason(response: &str) -> Option<String> {
    let reason = subagent_end_freeze_reason(Some(response));
    reason
        .starts_with(SUBAGENT_END_HANDBACK_REASON_PREFIX)
        .then_some(reason)
}

fn coordinator_activities(
    sessions: &Path,
    coordinator: &str,
    tail_bytes: Option<u64>,
) -> Result<Vec<WorkerActivity>> {
    if !codex_thread_id_is_safe(coordinator) {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    collect_codex_rollout_candidates(sessions, coordinator, &mut candidates);
    let [source] = candidates.as_slice() else {
        return Ok(Vec::new());
    };
    let mut transcript = BufReader::new(std::fs::File::open(source)?);
    let mut first = String::new();
    transcript.read_line(&mut first)?;
    let Ok(meta) = serde_json::from_str::<Value>(&first) else {
        return Ok(Vec::new());
    };
    if meta.get("type").and_then(Value::as_str) != Some("session_meta")
        || meta.pointer("/payload/id").and_then(Value::as_str) != Some(coordinator)
    {
        return Ok(Vec::new());
    }

    if let Some(tail_bytes) = tail_bytes {
        let end = transcript.seek(SeekFrom::End(0))?;
        let offset = end.saturating_sub(tail_bytes);
        transcript.seek(SeekFrom::Start(offset))?;
        if offset > 0 {
            transcript.read_until(b'\n', &mut Vec::new())?;
        }
    }
    let limit = tail_bytes.unwrap_or(u64::MAX);
    let mut activities = Vec::new();
    for line in transcript.take(limit).lines() {
        activities.extend(WorkerActivity::parse(&line?, coordinator));
    }
    Ok(activities)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_queue::queue_claim::ClaimOwnerSession;
    use agent_doc_queue::transient_closeout::{
        codex_worker_capture_failure, normal_closeout_failure_with,
    };
    use serde_json::json;

    fn write_rollout(path: &Path, events: &[Value]) {
        let jsonl = events
            .iter()
            .map(|event| format!("{event}\n"))
            .collect::<String>();
        std::fs::write(path, jsonl).unwrap();
    }

    #[test]
    fn codex_stalled_nested_row_waits_and_recovers_its_finished_worker_without_redispatch() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let sessions = root.join("sessions");
        std::fs::create_dir(&sessions).unwrap();
        std::fs::create_dir(root.join(".agent-doc")).unwrap();
        let file = root.join("session.md");
        let doc = |marker: &str| {
            format!(
                "<!-- agent:run id=\"docs\" subagents go -->\n- ### https://github.com/sample-org/sample-docs/pull/3\n  - {marker}#address [#q-adr]\n<!-- /agent:run -->\n"
            )
        };
        let coordinator = "coordinator-thread";
        let turn = "coordinator-turn";
        let worker = "/root/address";
        let child = "native-address-worker";
        let child_turn = "address-turn";
        let response = "Commit verified and pushed to the existing branch.";
        let parent_rollout = sessions.join(format!("rollout-fixture-{coordinator}.jsonl"));
        let worker_rollout = sessions.join(format!("rollout-fixture-{child}.jsonl"));
        let item = "https://github.com/sample-org/sample-docs/pull/3: #address [#q-adr]";
        let owner_session = Some(ClaimOwnerSession {
            harness: Some("codex".into()),
            session_id: Some(coordinator.into()),
            pane: None,
            pid: None,
        });
        std::fs::write(&file, doc("📥 ")).unwrap();
        let (_, claimed) = crate::queue_claim::claim_with_session(
            &file,
            item,
            "subagent:address",
            600,
            owner_session.clone(),
        )
        .unwrap();
        let generation = claimed.generation;
        let running = crate::queue_claim::start_attempt_with_owner_session(
            &file,
            item,
            "subagent:address",
            worker,
            600,
            generation,
            owner_session,
        )
        .unwrap();
        let content = doc("🚧 [#subagent:address#1] ");
        std::fs::write(&file, &content).unwrap();

        let evidence = crate::transient_closeout::closeout_evidence(&file, &content);
        assert!(
            codex_worker_capture_failure(&content, &evidence)
                .unwrap()
                .unwrap()
                .contains("wait for its hand-back")
        );
        assert!(
            normal_closeout_failure_with(&content, &evidence)
                .unwrap()
                .is_some()
        );
        let pending = doc("📥 ");
        let evidence = crate::transient_closeout::closeout_evidence(&file, &pending);
        assert!(
            codex_worker_capture_failure(&pending, &evidence)
                .unwrap()
                .is_some()
        );

        let root_metadata = json!({"type": "session_meta", "payload": {"id": coordinator}});
        let activity = |kind: &str, id: &str, target: &str, at_secs| {
            json!({
                "type": "event_msg",
                "payload": {
                    "type": "item_completed",
                    "thread_id": coordinator,
                    "turn_id": target,
                    "completed_at_ms": at_secs * 1000u64,
                    "item": {"type": "SubAgentActivity", "kind": kind, "id": id,
                             "agent_path": worker, "agent_thread_id": child}
                }
            })
        };
        let launch = activity("started", "spawn-call", turn, claimed.claimed_at_secs);
        let completed = activity(
            "completed",
            "subagent-completed-address-turn",
            turn,
            running.worker.unwrap().started_at_secs,
        );
        let resumed = activity("interacted", "new-work", turn, claimed.claimed_at_secs + 1);
        write_rollout(
            &worker_rollout,
            &[
                json!({"type": "session_meta", "payload": {"id": child, "parent_thread_id": coordinator,
                   "session_id": coordinator, "thread_source": "subagent",
                   "source": {"subagent": {"thread_spawn": {"parent_thread_id": coordinator}}}}}),
                json!({"type": "event_msg", "payload": {"type": "task_started", "turn_id": "forked-root-turn"}}),
                json!({"type": "event_msg", "payload": {"type": "task_complete", "turn_id": "forked-root-turn", "last_agent_message": "Unrelated inherited result."}}),
                json!({"type": "event_msg", "payload": {"type": "task_started", "turn_id": child_turn}}),
                json!({"type": "response_item", "payload": {"type": "message", "role": "assistant",
                   "phase": "final_answer", "internal_chat_message_metadata_passthrough": {"turn_id": child_turn},
                   "content": [{"type": "output_text", "text": response}]}}),
                json!({"type": "event_msg", "payload": {"type": "task_complete", "turn_id": child_turn, "last_agent_message": response}}),
            ],
        );
        write_rollout(
            &parent_rollout,
            &[
                root_metadata.clone(),
                launch.clone(),
                completed.clone(),
                resumed,
            ],
        );
        preserve_from_sessions(&file, &sessions, coordinator).unwrap();
        let read_claim = || {
            crate::queue_claim::load_ledger_read_only(&file)
                .unwrap()
                .claims
                .remove(0)
        };
        assert!(read_claim().frozen.is_none());

        // The coordinator later starts another turn without relaunching this
        // worker. Its historical start/completion pair still names the live
        // generation and can be recovered without a replacement.
        write_rollout(
            &parent_rollout,
            &[
                root_metadata,
                launch,
                completed,
                json!({"type": "event_msg", "payload": {
                    "type": "task_started", "thread_id": coordinator, "turn_id": "next-turn"
                }}),
            ],
        );
        assert_eq!(
            recover_path_bound_handback(&sessions, &read_claim(), coordinator)
                .unwrap()
                .as_deref(),
            Some(response)
        );
        preserve_from_sessions(&file, &sessions, coordinator).unwrap();
        let preserved = read_claim();
        assert_eq!(preserved.generation, generation);
        assert_eq!(preserved.state, QueueClaimState::Running);
        assert_eq!(
            preserved.frozen.unwrap().recorded_end_handback(),
            Some(response)
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
        let evidence = crate::transient_closeout::closeout_evidence(&file, &content);
        let failure = normal_closeout_failure_with(&content, &evidence)
            .unwrap()
            .unwrap();
        assert!(failure.contains("queue report-complete"), "{failure}");
        assert!(failure.contains("--generation 1"), "{failure}");
    }
}
