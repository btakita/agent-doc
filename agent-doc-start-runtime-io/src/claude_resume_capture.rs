//! `#clauderesumeafterclear`: keep `resume.claude` on the conversation the
//! supervised Claude Code harness is actually running.
//!
//! A pane-launched Claude session reports its conversation id to agent-doc only
//! through hooks. `start` mints and records the id at launch, but an operator
//! `/clear` (and any non-resume startup) begins a NEW conversation under a new
//! id. Before this capture the document kept the pre-clear id, so every later
//! relaunch and Restart Agent resumed the pre-clear conversation (observed
//! 2026-10-20: `resume` stayed `ba846c4c` after the `/clear` that started
//! `e62ea89f`). Codex already follows its thread through the hook ledger
//! projection; this is the Claude counterpart, driven by the `SessionStart`
//! hook (`agent-doc turn-status idle`).
//!
//! The write goes through [`record_document_resume_id_for_harness`] — the
//! binary-owned, realtime-authority frontmatter write — and only after
//! [`agent_doc_turn_status_io::resolve_claude_session_start_resume`] proves the
//! document and pane. Every unprovable shape is a skip, never a guess.
//!
//! `#claudecrashresumeafterclear`: the frontmatter pointer alone does not reach
//! a crash restart. A supervisor that auto-restarts its crashed Claude child
//! resumes its in-memory lineage, which still names the pre-`/clear` id, and
//! must never refresh lineage from (possibly lagging or hand-edited)
//! frontmatter. The hook therefore also records the proof itself — the
//! supervisor, generation, and exact harness child (pid + kernel start time) —
//! as a `document_runtime_state` row, and the respawning supervisor adopts the
//! id only when that provenance names the child it just lost
//! ([`claude_respawn_session_start`]).

use std::path::Path;

use agent_doc_sqlite::state_store::{self, Connection, DocumentRuntimeStateRecord};
use agent_doc_supervisor::session_lineage::{
    CLAUDE_SESSION_START_STATE_KIND, ClaudeSessionStartCapture, HarnessChildIdentity,
    HarnessSessionLineage, claude_respawn_session_start_adoption,
    parse_claude_session_start_capture,
};

use crate::record_document_resume_id_for_harness;

/// Outcome of one hook payload, for logging and tests.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeResumeCaptureOutcome {
    /// The payload is not a new-conversation `SessionStart` (most hooks).
    NotApplicable,
    /// A new conversation was reported but its ownership was not proven.
    Refused(&'static str),
    /// `resume.claude` now names the new id (`changed=false`: it already did).
    Recorded { changed: bool },
    /// The id reached realtime authority and its live editor projection owns
    /// the remaining persistence. No operator action is required.
    Deferred,
    /// Ownership was proven but the authority write failed.
    WriteFailed(String),
}

/// `turn-status idle` payload hook: record a proven new Claude conversation id
/// on the pane's document. Best-effort; never fails the harness hook.
pub fn capture_claude_session_start_resume(base: &Path, pane: &str, payload: &str) {
    let ancestors = agent_doc_turn_status_io::hook_process_ancestors();
    let _ = capture_claude_session_start_resume_with_process_view(
        base,
        pane,
        payload,
        &ancestors,
        &agent_doc_turn_status_io::process_start_ticks,
        now_ms(),
    );
}

/// [`capture_claude_session_start_resume`] with the hook process ancestry
/// injected; harness start times come from the live `/proc`.
pub fn capture_claude_session_start_resume_with_ancestors(
    base: &Path,
    pane: &str,
    payload: &str,
    ancestors: &[(u32, String)],
) -> ClaudeResumeCaptureOutcome {
    capture_claude_session_start_resume_with_process_view(
        base,
        pane,
        payload,
        ancestors,
        &agent_doc_turn_status_io::process_start_ticks,
        now_ms(),
    )
}

/// [`capture_claude_session_start_resume`] with the whole process view
/// injected (tests): the hook's ancestry, each pid's kernel start time, and the
/// clock.
pub fn capture_claude_session_start_resume_with_process_view(
    base: &Path,
    pane: &str,
    payload: &str,
    ancestors: &[(u32, String)],
    start_ticks: &dyn Fn(u32) -> Option<u64>,
    recorded_at_ms: u64,
) -> ClaudeResumeCaptureOutcome {
    let capture = match agent_doc_turn_status_io::resolve_claude_session_start_resume(
        base, pane, payload, ancestors,
    ) {
        Ok(capture) => capture,
        Err(skip) => {
            let Some(candidate) = skip.candidate else {
                return ClaudeResumeCaptureOutcome::NotApplicable;
            };
            let line = format!(
                "claude_session_start_resume_refused pane={pane} id={} source={} reason={}",
                candidate.session_id, candidate.source, skip.reason
            );
            eprintln!("[turn-status] {line}");
            if let Some(document) = skip.document.as_deref() {
                agent_doc_ops_log_io::log_op(document, &line);
            }
            return ClaudeResumeCaptureOutcome::Refused(skip.reason);
        }
    };
    // Provenance first: a crash right after the `/clear` must resume the new
    // conversation even when the frontmatter write below stalls or fails.
    record_session_start_provenance(base, pane, &capture, start_ticks, recorded_at_ms);
    match record_document_resume_id_for_harness(&capture.document, "claude", &capture.session_id) {
        Ok(changed) => {
            agent_doc_ops_log_io::log_op(
                &capture.document,
                &format!(
                    "claude_session_start_resume_recorded pane={pane} id={} source={} changed={changed}",
                    capture.session_id, capture.source
                ),
            );
            ClaudeResumeCaptureOutcome::Recorded { changed }
        }
        Err(err) => {
            let error = format!("{err:#}").replace('\n', "\\n");
            if agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(&error) {
                let line = format!(
                    "claude_session_start_resume_record_deferred pane={pane} id={} source={} recovery=retained_async_editor_delivery operator_action=none",
                    capture.session_id, capture.source,
                );
                eprintln!("[turn-status] {line}");
                agent_doc_ops_log_io::log_op(&capture.document, &line);
                return ClaudeResumeCaptureOutcome::Deferred;
            }
            let line = format!(
                "claude_session_start_resume_record_failed pane={pane} id={} source={} error={error}",
                capture.session_id, capture.source
            );
            eprintln!("[turn-status] {line}");
            agent_doc_ops_log_io::log_op(&capture.document, &line);
            ClaudeResumeCaptureOutcome::WriteFailed(error)
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn session_start_key(document_id: &str) -> String {
    agent_doc_fs::document_state_hash_from_str(document_id)
}

/// Persist the hook's proof as a [`ClaudeSessionStartCapture`]. An unreadable
/// harness start time leaves no record (a respawn then keeps its lineage).
fn record_session_start_provenance(
    base: &Path,
    pane: &str,
    capture: &agent_doc_turn_status_io::ClaudeResumeCapture,
    start_ticks: &dyn Fn(u32) -> Option<u64>,
    recorded_at_ms: u64,
) {
    let outcome = start_ticks(capture.harness_pid)
        .ok_or_else(|| "harness_start_time_unreadable".to_string())
        .and_then(|harness_start_ticks| {
            let record = ClaudeSessionStartCapture {
                session_id: capture.session_id.clone(),
                source: capture.source.to_string(),
                supervisor_pid: capture.supervisor_pid,
                generation: capture.generation,
                harness_pid: capture.harness_pid,
                harness_start_ticks,
                recorded_at_ms,
            };
            state_store::open_state_db(base)
                .and_then(|conn| {
                    record_claude_session_start_in_db(&conn, &capture.document_id, &record)
                })
                .map_err(|err| format!("{err:#}").replace('\n', "\\n"))
        });
    if let Err(reason) = outcome {
        let line = format!(
            "claude_session_start_provenance_unrecorded pane={pane} id={} harness_pid={} reason={reason}",
            capture.session_id, capture.harness_pid
        );
        eprintln!("[turn-status] {line}");
        agent_doc_ops_log_io::log_op(&capture.document, &line);
    }
}

/// Upsert the document's latest proven Claude `SessionStart` capture.
pub fn record_claude_session_start_in_db(
    conn: &Connection,
    document_id: &str,
    capture: &ClaudeSessionStartCapture,
) -> anyhow::Result<()> {
    state_store::upsert_document_runtime_state_in_db(
        conn,
        &DocumentRuntimeStateRecord {
            document_hash: session_start_key(document_id),
            state_kind: CLAUDE_SESSION_START_STATE_KIND.to_string(),
            canonical_path: document_id.to_string(),
            payload_json: serde_json::to_string(capture)?,
            updated_at_ms: capture.recorded_at_ms,
        },
    )
}

/// Load the document's latest proven Claude `SessionStart` capture, if any.
pub fn load_claude_session_start_from_db(
    conn: &Connection,
    document_id: &str,
) -> anyhow::Result<Option<ClaudeSessionStartCapture>> {
    Ok(state_store::load_document_runtime_state_from_db(
        conn,
        &session_start_key(document_id),
        CLAUDE_SESSION_START_STATE_KIND,
    )?
    .and_then(|row| parse_claude_session_start_capture(&row.payload_json)))
}

/// Outcome of [`claude_respawn_session_start`], for the supervisor's logs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaudeRespawnSessionStart {
    /// No proven capture for this document (the common case): keep lineage.
    NoCapture,
    /// A capture exists but its provenance does not name the exited child.
    Refused {
        reason: &'static str,
        capture_id: String,
    },
    /// The lineage now names the exited child's own conversation.
    Adopted {
        id: String,
        prior: Option<String>,
        changed: bool,
    },
    /// `state.db` could not be read; lineage kept (fail closed).
    Unreadable(String),
}

/// `#claudecrashresumeafterclear`: at the respawn edge of a Claude child,
/// re-read the durably recorded `SessionStart` capture and adopt it into the
/// supervisor's lineage when its provenance names the child that just exited.
///
/// This is a single read at a child-lifecycle edge — the respawn already
/// re-reads current frontmatter and the hook/controller projection here — not
/// polling. `document_id` is the actor's controller id; `supervisor_pid` is
/// this process; `exited_child` was sampled from `/proc` while the child lived.
pub fn claude_respawn_session_start(
    project_root: &Path,
    document_id: &str,
    supervisor_pid: u32,
    exited_child: Option<HarnessChildIdentity>,
    lineage: &mut HarnessSessionLineage,
) -> ClaudeRespawnSessionStart {
    let loaded = state_store::open_state_db(project_root).and_then(|conn| {
        let capture = load_claude_session_start_from_db(&conn, document_id)?;
        let lease_pid = match capture {
            Some(_) => state_store::load_document_actor_supervisor_from_db(&conn, document_id)?
                .and_then(|actor| actor.supervisor_pid),
            None => None,
        };
        Ok((capture, lease_pid))
    });
    let (capture, lease_pid) = match loaded {
        Ok(loaded) => loaded,
        Err(err) => {
            return ClaudeRespawnSessionStart::Unreadable(format!("{err:#}").replace('\n', "\\n"));
        }
    };
    let Some(capture) = capture else {
        return ClaudeRespawnSessionStart::NoCapture;
    };
    match claude_respawn_session_start_adoption(
        Some(&capture),
        supervisor_pid,
        lease_pid,
        exited_child,
    ) {
        Ok(id) => {
            let prior = lineage.active_id().map(str::to_string);
            let changed = lineage.adopt_exited_child_session_start(id);
            ClaudeRespawnSessionStart::Adopted {
                id: id.to_string(),
                prior,
                changed,
            }
        }
        Err(reason) => ClaudeRespawnSessionStart::Refused {
            reason,
            capture_id: capture.session_id,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_frontmatter::frontmatter;

    #[test]
    fn retained_live_projection_is_a_deferred_resume_record_not_a_failure() {
        let error = format!(
            "write retained [{}] [{}]",
            agent_doc_turn::write_ownership::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
            agent_doc_turn::write_ownership::RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN,
        );
        assert!(
            agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(&error)
        );
        assert!(!error.contains("STRANDED"));
        assert!(!error.contains("agent-doc commit"));
    }

    fn seed(base: &Path, doc: &Path, pane: &str, supervisor_pid: u32) {
        let conn = agent_doc_sqlite::state_store::open_state_db(base).unwrap();
        let id = doc.to_string_lossy().into_owned();
        conn.execute(
            "INSERT INTO documents (document_id, canonical_path, session_id, generation, pane_id, \
             window_id, harness_id, actor_state) VALUES (?1, ?1, 'doc-session', 3, ?2, '@1', 'claude-code', 'ready')",
            (&id, pane),
        )
        .unwrap();
        conn.execute(
            "INSERT INTO supervisor_leases (document_id, generation, supervisor_pid) VALUES (?1, 3, ?2)",
            (&id, supervisor_pid),
        )
        .unwrap();
    }

    /// Deterministic `/proc` view: harness pid `p` started at tick `p + 1000`.
    fn ticks(pid: u32) -> Option<u64> {
        Some(u64::from(pid) + 1000)
    }

    fn hook(
        base: &Path,
        pane: &str,
        payload: &str,
        ancestors: &[(u32, String)],
    ) -> ClaudeResumeCaptureOutcome {
        capture_claude_session_start_resume_with_process_view(
            base, pane, payload, ancestors, &ticks, 1_000,
        )
    }

    fn child(pid: u32) -> Option<HarnessChildIdentity> {
        ticks(pid).map(|start_ticks| HarnessChildIdentity { pid, start_ticks })
    }

    fn chain(entries: &[(u32, &str)]) -> Vec<(u32, String)> {
        entries
            .iter()
            .map(|(pid, comm)| (*pid, comm.to_string()))
            .collect()
    }

    fn stored(base: &Path, doc: &Path) -> Option<ClaudeSessionStartCapture> {
        let conn = state_store::open_state_db(base).unwrap();
        load_claude_session_start_from_db(&conn, &doc.to_string_lossy()).unwrap()
    }

    fn respawn(
        base: &Path,
        doc: &Path,
        supervisor_pid: u32,
        exited: Option<HarnessChildIdentity>,
        lineage: &mut HarnessSessionLineage,
    ) -> ClaudeRespawnSessionStart {
        claude_respawn_session_start(
            base,
            &doc.to_string_lossy(),
            supervisor_pid,
            exited,
            lineage,
        )
    }

    fn payload(source: &str, id: &str) -> String {
        format!(
            r#"{{"hook_event_name":"SessionStart","source":"{source}","session_id":"{id}","transcript_path":"/h/.claude/projects/p/{id}.jsonl"}}"#
        )
    }

    fn resume_ids(doc: &Path) -> (Option<String>, Option<String>) {
        let content = std::fs::read_to_string(doc).unwrap();
        let (fm, _) = frontmatter::parse(&content).unwrap();
        (
            fm.resume_for_harness("claude").map(str::to_string),
            fm.resume_for_harness("codex").map(str::to_string),
        )
    }

    /// `#clauderesumeafterclear` regression (2026-10-20): `/clear` started
    /// `e62ea89f` but the document kept `ba846c4c`, so relaunch and Restart
    /// Agent both resumed the pre-clear conversation.
    #[test]
    fn clear_session_start_moves_resume_claude_to_the_new_conversation() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let doc = base.join("agent-doc-bugs.md");
        std::fs::write(
            &doc,
            "---\nagent: claude\nresume:\n  claude: ba846c4c\n  codex: codex-thread\n---\n\n# Bugs\n",
        )
        .unwrap();
        seed(&base, &doc, "%2", 1605424);
        let from_harness = vec![
            (2909304, "claude".to_string()),
            (1605424, "agent-doc".to_string()),
        ];

        assert_eq!(
            hook(&base, "%2", &payload("clear", "e62ea89f"), &from_harness),
            ClaudeResumeCaptureOutcome::Recorded { changed: true }
        );
        assert_eq!(
            resume_ids(&doc),
            (Some("e62ea89f".into()), Some("codex-thread".into())),
            "the clear moves only the Claude pointer"
        );
        assert!(std::fs::read_to_string(&doc).unwrap().ends_with("# Bugs\n"));

        // `resume` reloads the same conversation: no write, pointer unchanged.
        assert_eq!(
            hook(&base, "%2", &payload("resume", "ba846c4c"), &from_harness),
            ClaudeResumeCaptureOutcome::NotApplicable
        );
        assert_eq!(resume_ids(&doc).0.as_deref(), Some("e62ea89f"));

        // A startup that reports the id `start` already recorded is a no-op.
        assert_eq!(
            hook(&base, "%2", &payload("startup", "e62ea89f"), &from_harness),
            ClaudeResumeCaptureOutcome::Recorded { changed: false }
        );
    }

    /// `#clauderesumeafterclear`: a nested `claude -p` run from inside the
    /// pane reports a fresh `startup` id with the pane's `$TMUX_PANE`; it must
    /// never move the document's pointer.
    #[test]
    fn nested_harness_session_start_never_moves_resume_claude() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let doc = base.join("doc.md");
        std::fs::write(&doc, "---\nresume:\n  claude: main-session\n---\n").unwrap();
        seed(&base, &doc, "%3", 10);
        let nested = vec![
            (40, "claude".to_string()),
            (35, "zsh".to_string()),
            (20, "claude".to_string()),
            (10, "agent-doc".to_string()),
        ];
        assert_eq!(
            hook(
                &base,
                "%3",
                &payload("startup", "subagent-session"),
                &nested
            ),
            ClaudeResumeCaptureOutcome::Refused("hook_not_from_pane_supervisor_harness")
        );
        assert_eq!(resume_ids(&doc).0.as_deref(), Some("main-session"));
    }

    /// `#claudecrashresumeafterclear` regression (2026-10-20): after the
    /// `/clear` that started `e62ea89f`, the supervised Claude crashed and the
    /// supervisor's auto-restart resumed its in-memory `ba846c4c`. The respawn
    /// edge must re-read the SessionStart capture and resume `e62ea89f`. The
    /// ordering is injected step by step: respawn before the hook, hook, then
    /// respawn; then a second crash of the respawned child; then a `/clear`
    /// inside that child.
    #[test]
    fn crash_respawn_after_clear_resumes_the_session_start_id_not_pre_clear_lineage() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let doc = base.join("agent-doc-bugs.md");
        std::fs::write(
            &doc,
            "---\nagent: claude\nresume:\n  claude: ba846c4c\n---\n\n# Bugs\n",
        )
        .unwrap();
        let supervisor = 1605424;
        seed(&base, &doc, "%2", supervisor);
        // `start` minted `ba846c4c`; that is the supervisor's lineage.
        let mut lineage = HarnessSessionLineage::new(Some("ba846c4c".into()), None);
        let first_child = 2909304;

        // 1. The child crashes before any /clear: nothing to adopt.
        assert_eq!(
            respawn(&base, &doc, supervisor, child(first_child), &mut lineage),
            ClaudeRespawnSessionStart::NoCapture
        );
        assert_eq!(lineage.active_id(), Some("ba846c4c"));

        // 2. /clear inside that child: its SessionStart hook records the id and
        //    the provenance naming the exact child.
        let from_first = chain(&[
            (31, "sh"),
            (first_child, "claude"),
            (supervisor, "agent-doc"),
        ]);
        assert_eq!(
            hook(&base, "%2", &payload("clear", "e62ea89f"), &from_first),
            ClaudeResumeCaptureOutcome::Recorded { changed: true }
        );
        assert_eq!(
            stored(&base, &doc),
            Some(ClaudeSessionStartCapture {
                session_id: "e62ea89f".into(),
                source: "clear".into(),
                supervisor_pid: supervisor,
                generation: 3,
                harness_pid: first_child,
                harness_start_ticks: u64::from(first_child) + 1000,
                recorded_at_ms: 1_000,
            })
        );

        // 3. The child crashes; the respawn resumes the post-clear conversation.
        assert_eq!(
            respawn(&base, &doc, supervisor, child(first_child), &mut lineage),
            ClaudeRespawnSessionStart::Adopted {
                id: "e62ea89f".into(),
                prior: Some("ba846c4c".into()),
                changed: true,
            }
        );
        assert_eq!(lineage.active_id(), Some("e62ea89f"));

        // 4. The respawned child (`--resume e62ea89f`) crashes too, without a
        //    /clear: the capture names the previous child, so it is not
        //    re-adopted — and the lineage already holds the right id.
        let second_child = 3000001;
        assert_eq!(
            respawn(&base, &doc, supervisor, child(second_child), &mut lineage),
            ClaudeRespawnSessionStart::Refused {
                reason: "capture_from_other_harness_child",
                capture_id: "e62ea89f".into(),
            }
        );
        assert_eq!(lineage.active_id(), Some("e62ea89f"));

        // 5. A /clear inside the respawned child, then a crash: follow it again.
        let from_second = chain(&[(second_child, "claude"), (supervisor, "agent-doc")]);
        assert_eq!(
            hook(&base, "%2", &payload("clear", "f00dcafe"), &from_second),
            ClaudeResumeCaptureOutcome::Recorded { changed: true }
        );
        assert_eq!(
            respawn(&base, &doc, supervisor, child(second_child), &mut lineage),
            ClaudeRespawnSessionStart::Adopted {
                id: "f00dcafe".into(),
                prior: Some("e62ea89f".into()),
                changed: true,
            }
        );
        assert_eq!(lineage.active_id(), Some("f00dcafe"));
    }

    /// `#claudecrashresumeafterclear`: the respawn adopts a capture only when
    /// the hook's provenance still holds — as strict as the hook's own proof.
    #[test]
    fn crash_respawn_refuses_captures_whose_provenance_does_not_name_the_exited_child() {
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(base.join(".agent-doc")).unwrap();
        let doc = base.join("doc.md");
        std::fs::write(&doc, "---\nresume:\n  claude: main-session\n---\n").unwrap();
        let supervisor = 10;
        seed(&base, &doc, "%3", supervisor);
        let fresh = || HarnessSessionLineage::new(Some("main-session".into()), None);

        // A nested `claude -p` in the pane records nothing, so a crash of the
        // pane's own child keeps the main conversation.
        let nested = chain(&[
            (40, "claude"),
            (35, "zsh"),
            (20, "claude"),
            (supervisor, "agent-doc"),
        ]);
        assert_eq!(
            hook(
                &base,
                "%3",
                &payload("startup", "subagent-session"),
                &nested
            ),
            ClaudeResumeCaptureOutcome::Refused("hook_not_from_pane_supervisor_harness")
        );
        assert_eq!(stored(&base, &doc), None);
        let mut lineage = fresh();
        assert_eq!(
            respawn(&base, &doc, supervisor, child(20), &mut lineage),
            ClaudeRespawnSessionStart::NoCapture
        );
        assert_eq!(lineage.active_id(), Some("main-session"));

        // An unreadable harness start time leaves no provenance: the frontmatter
        // still moves (the hook's contract), but a respawn cannot adopt it.
        let direct = chain(&[(20, "claude"), (supervisor, "agent-doc")]);
        assert_eq!(
            capture_claude_session_start_resume_with_process_view(
                &base,
                "%3",
                &payload("clear", "unprovable"),
                &direct,
                &|_| None,
                1_000,
            ),
            ClaudeResumeCaptureOutcome::Recorded { changed: true }
        );
        assert_eq!(stored(&base, &doc), None);
        assert_eq!(
            respawn(&base, &doc, supervisor, child(20), &mut lineage),
            ClaudeRespawnSessionStart::NoCapture
        );

        assert_eq!(
            hook(&base, "%3", &payload("clear", "cleared-session"), &direct),
            ClaudeResumeCaptureOutcome::Recorded { changed: true }
        );
        let refused = |reason| ClaudeRespawnSessionStart::Refused {
            reason,
            capture_id: "cleared-session".into(),
        };
        // Another supervisor process (e.g. a later cold start) never adopts it.
        let mut lineage = fresh();
        assert_eq!(
            respawn(&base, &doc, 11, child(20), &mut lineage),
            refused("capture_from_other_supervisor")
        );
        // The exited child's identity was never sampled.
        assert_eq!(
            respawn(&base, &doc, supervisor, None, &mut lineage),
            refused("exited_child_identity_unknown")
        );
        // Same pid, different kernel start time: a recycled pid.
        assert_eq!(
            respawn(
                &base,
                &doc,
                supervisor,
                Some(HarnessChildIdentity {
                    pid: 20,
                    start_ticks: 1
                }),
                &mut lineage
            ),
            refused("capture_from_other_harness_child")
        );
        assert_eq!(lineage.active_id(), Some("main-session"));
        // The supervisor lost the document's current-generation lease.
        let conn = state_store::open_state_db(&base).unwrap();
        conn.execute("UPDATE documents SET generation = 4", ())
            .unwrap();
        assert_eq!(
            respawn(&base, &doc, supervisor, child(20), &mut lineage),
            refused("supervisor_not_current_lease_owner")
        );
        conn.execute(
            "INSERT INTO supervisor_leases (document_id, generation, supervisor_pid) VALUES (?1, 4, ?2)",
            (doc.to_string_lossy().into_owned(), supervisor),
        )
        .unwrap();
        // Re-registered under the next generation with the same pid (an
        // in-place re-exec keeps the pid and the child): still the owner.
        assert_eq!(
            respawn(&base, &doc, supervisor, child(20), &mut lineage),
            ClaudeRespawnSessionStart::Adopted {
                id: "cleared-session".into(),
                prior: Some("main-session".into()),
                changed: true,
            }
        );
    }
}
