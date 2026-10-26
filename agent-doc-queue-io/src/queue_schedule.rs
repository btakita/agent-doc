//! Queue-schedule IO for drain paths and the CLI (`#queueschedule`).
//!
//! - [`schedule_held_heads`] is the drain gate: the heads a closed schedule
//!   window holds back. [`crate::queue_claim::claimed_items_for_content`]
//!   folds them into the exclusion set every drain path already honours
//!   (supervisor idle watch, in-session loop, preflight selection, Stop-hook
//!   continuation, subagent fan-out), exactly like heads the queue-level
//!   subagents attribute holds.
//! - [`restore_loop_items_with_effects`] is the binary-owned writer the
//!   controller's scheduler calls (through its runtime-effects port) to put a
//!   completed `loop` pass's items back.
//! - [`schedule_status`] backs `agent-doc queue schedule`.

use std::path::Path;

use anyhow::Result;
use serde::Serialize;

use crate::queue_cmd::QueueCommandEffects;
use agent_doc_queue::queue_schedule;

/// The heads of `content` a closed schedule window holds back.
///
/// Unscheduled content never touches `state.db`. A scheduled queue whose state
/// cannot be read is held (reported once per call): the schedule is an explicit
/// operator restriction, so the failure direction is "wait for the
/// controller", never "drain off-schedule".
pub fn schedule_held_heads(file: &Path, content: &str) -> Vec<String> {
    let schedules = match queue_schedule::identified_queue_schedules(content) {
        Ok(schedules) => schedules,
        Err(err) => {
            eprintln!(
                "[queue-schedule] WARNING: could not identify queue schedules for {}: {err:#}",
                file.display()
            );
            return Vec::new();
        }
    };
    let mut held = Vec::new();
    for schedule in schedules {
        match agent_doc_controller_io::queue_scheduler::load_schedule_state_for_queue(
            file,
            &schedule.queue_id,
        ) {
            Ok(state) => held.extend(
                queue_schedule::held_heads_for_queue(content, &schedule, state.as_ref())
                    .unwrap_or_default(),
            ),
            Err(err) => {
                eprintln!(
                    "[queue-schedule] WARNING: could not read queue schedule id={} for {}; holding only that queue's heads: {err:#}",
                    schedule.queue_id,
                    file.display()
                );
                held.extend(
                    queue_schedule::live_heads_for_queue(content, &schedule.queue_id)
                        .unwrap_or_default(),
                );
            }
        }
    }
    held
}

/// The [`agent_doc_queue::queue_head_entry::HeadEchoWindow`] of queue
/// `queue_id` in `content`: which history scopes the echoes that may answer
/// its live free-text heads. Reads `state.db` only for a `loop` queue; an
/// unreadable row holds the queue's heads (no echo answers them). Shared by
/// the preflight catch-up residue strike and the continuation-head skip.
pub fn head_echo_window(
    file: &Path,
    content: &str,
    queue_id: &str,
) -> agent_doc_queue::queue_head_entry::HeadEchoWindow {
    agent_doc_queue::queue_head_entry::HeadEchoWindow::for_queue(content, queue_id, || {
        agent_doc_controller_io::queue_scheduler::load_schedule_state_for_queue(file, queue_id)
            .ok()
            .flatten()
            .map(|state| state.window)
    })
}

/// Put `roster` back into `file`'s selected `agent:queue` through the
/// binary-owned write path (the live CRDT replica while an editor holds the
/// document, never a disk write behind it). Returns how many items were
/// restored (0 when every item is already live).
pub fn restore_loop_items_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
    roster: &[String],
) -> Result<usize> {
    let schedule = queue_schedule::identified_queue_schedules(
        &effects.current_document_content(file, "queue_schedule_loop_restore_lookup")?,
    )?
    .into_iter()
    .next()
    .ok_or_else(|| anyhow::anyhow!("no scheduled queue to restore"))?;
    restore_loop_items_for_queue_with_effects(effects, file, &schedule.queue_id, roster)
}

pub fn restore_loop_items_for_queue_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
    queue_id: &str,
    roster: &[String],
) -> Result<usize> {
    // `#sqedit-race`: one queue writer at a time, including the source read.
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let current = effects.current_document_content(file, "queue_schedule_loop_restore")?;
    restore_loop_items_for_queue_from_source_locked(effects, file, queue_id, roster, &current)
}

/// Restore `roster` from the exact authoritative queue cut that proved the
/// pass complete. The scheduler passes that observation through so a later
/// read cannot silently replace the compare-and-swap base with a stale
/// pre-strike projection (GH #348).
pub fn restore_loop_items_for_queue_from_source_with_effects(
    effects: &impl QueueCommandEffects,
    file: &Path,
    queue_id: &str,
    roster: &[String],
    source_content: &str,
) -> Result<usize> {
    // `#sqedit-race`: one queue writer at a time.
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    restore_loop_items_for_queue_from_source_locked(effects, file, queue_id, roster, source_content)
}

fn restore_loop_items_for_queue_from_source_locked(
    effects: &impl QueueCommandEffects,
    file: &Path,
    queue_id: &str,
    roster: &[String],
    source_content: &str,
) -> Result<usize> {
    let before = live_roster_count(source_content, queue_id, roster)?;
    let updated = queue_schedule::plan_loop_restore_for_queue(source_content, queue_id, roster)?
        .unwrap_or_else(|| {
            // GH #330: a retained restore can be visible in the live authority
            // before its delivery is settled. Do not turn that transient
            // visibility into `Ok(0)`: the convergence call's already-current
            // path still runs the delivery guard. Until that proof succeeds,
            // the scheduler must keep `restore_pending` and retry after backoff.
            source_content.to_string()
        });
    effects.converge_document_or_disk(
        file,
        &updated,
        source_content,
        "queue_schedule_loop_restore",
    )?;

    // The write transport may accept a semantic/exact no-op after rebasing
    // over a concurrently settling closeout. That transport receipt is not
    // proof that the loop roster is live. Re-read the authoritative document
    // and keep `restore_pending` when any item is still struck or absent.
    let settled = effects.current_document_content(file, "queue_schedule_loop_restore_verify")?;
    if queue_schedule::plan_loop_restore_for_queue(&settled, queue_id, roster)?.is_some() {
        anyhow::bail!(
            "queue loop restore for queue {queue_id} did not make its roster visible (source_hash={} target_hash={} observed_hash={})",
            agent_doc_hash::content_hash(source_content),
            agent_doc_hash::content_hash(&updated),
            agent_doc_hash::content_hash(&settled),
        );
    }
    let restored = live_roster_count(&settled, queue_id, roster)?.saturating_sub(before);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_schedule_loop_restore_write queue_id={queue_id} roster={} restored={restored}",
            roster.len()
        ),
    );
    Ok(restored)
}

fn live_roster_count(content: &str, queue_id: &str, roster: &[String]) -> Result<usize> {
    let live = queue_schedule::live_heads_for_queue(content, queue_id)?
        .into_iter()
        .map(|item| agent_doc_queue::queue_claim::claim_identity(&item))
        .collect::<std::collections::HashSet<_>>();
    let roster = roster
        .iter()
        .map(|item| agent_doc_queue::queue_claim::claim_identity(item))
        .collect::<std::collections::HashSet<_>>();
    Ok(live.intersection(&roster).count())
}

/// The document's schedule, as `agent-doc queue schedule` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "scheduled", rename_all = "snake_case")]
pub enum ScheduleReport {
    #[serde(rename = "false")]
    Unscheduled,
    #[serde(rename = "true")]
    Scheduled {
        /// Backward-compatible flat fields for a single valid schedule.
        #[serde(flatten)]
        legacy: Option<LegacyScheduleReport>,
        queues: Vec<QueueScheduleReport>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LegacyScheduleReport {
    #[serde(flatten)]
    pub status: queue_schedule::QueueScheduleStatus,
    pub held_heads: usize,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct QueueScheduleReport {
    pub queue_id: String,
    pub occurrence: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<queue_schedule::QueueScheduleStatus>,
    pub held_heads: usize,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<String>,
}

pub fn schedule_status(file: &Path, content: &str) -> Result<ScheduleReport> {
    let schedules = queue_schedule::identified_queue_schedules(content)?;
    if schedules.is_empty() {
        return Ok(ScheduleReport::Unscheduled);
    }
    let mut queues = Vec::new();
    for schedule in schedules {
        let state = agent_doc_controller_io::queue_scheduler::load_schedule_state_for_queue(
            file,
            &schedule.queue_id,
        )?;
        let held_heads =
            queue_schedule::held_heads_for_queue(content, &schedule, state.as_ref())?.len();
        let status = schedule
            .spec
            .as_ref()
            .filter(|_| schedule.is_valid())
            .map(|spec| {
                queue_schedule::status(
                    spec,
                    state.as_ref(),
                    &agent_doc_controller_io::queue_scheduler::SystemLocalOffset,
                )
            });
        queues.push(QueueScheduleReport {
            queue_id: schedule.queue_id,
            occurrence: schedule.occurrence,
            status,
            held_heads,
            errors: schedule
                .issues
                .iter()
                .filter(|issue| issue.fatal)
                .map(|issue| format!("{}: {}", issue.attr, issue.message))
                .collect(),
        });
    }
    let legacy = queues.as_slice().first().and_then(|queue| {
        (queues.len() == 1).then(|| {
            queue.status.clone().map(|status| LegacyScheduleReport {
                status,
                held_heads: queue.held_heads,
            })
        })?
    });
    Ok(ScheduleReport::Scheduled { legacy, queues })
}

/// Render [`ScheduleReport`] for a terminal.
pub fn render_schedule_report(file: &Path, report: &ScheduleReport) -> String {
    let ScheduleReport::Scheduled { queues, .. } = report else {
        return format!(
            "{}: agent:queue is not scheduled (no interval/time/loop attribute)",
            file.display()
        );
    };
    let when = |secs: Option<u64>| {
        secs.map_or_else(|| "-".to_string(), agent_doc_log_time::format_log_timestamp)
    };
    let mut lines = vec![format!(
        "{}: {} scheduled queue(s)",
        file.display(),
        queues.len()
    )];
    for queue in queues {
        let Some(status) = &queue.status else {
            lines.push(format!(
                "  [{}] invalid; {} head(s) held: {}",
                queue.queue_id,
                queue.held_heads,
                queue.errors.join("; ")
            ));
            continue;
        };
        let cadence = match (status.interval_secs, status.times.is_empty()) {
            (Some(interval), true) => format!("every {interval}s"),
            (Some(interval), false) => {
                format!("every {interval}s and at {}", status.times.join(","))
            }
            (None, false) => format!("at {}", status.times.join(",")),
            (None, true) => "after each pass".to_string(),
        };
        let passes = match status.loop_limit {
            queue_schedule::LoopLimit::Off => {
                format!("{} passes run", status.passes_completed)
            }
            queue_schedule::LoopLimit::Unbounded => {
                format!("loop {} passes run (unbounded)", status.passes_completed)
            }
            queue_schedule::LoopLimit::Passes(limit) => {
                format!("loop {}/{limit} passes run", status.passes_completed)
            }
        };
        let window = match status.current_pass {
            Some(pass) => format!("pass {pass} running"),
            None if status.pending_admission => "waiting for the project controller".to_string(),
            None if status.exhausted => "loop finished; items held".to_string(),
            None => format!("idle, {} head(s) held", queue.held_heads),
        };
        lines.extend([
            format!("  [{}] scheduled queue ({cadence})", queue.queue_id),
            format!("    state: {window}; {passes}"),
            format!(
                "    next run: {}; last tick: {}",
                when(status.next_run_secs),
                when(status.last_fired_at_secs)
            ),
        ]);
        if status.failures > 0 {
            lines.push(format!(
                "    backing off after {} failure(s) until {}",
                status.failures,
                when(status.backoff_until_secs)
            ));
        }
        if status.restore_pending {
            lines.push("    loop items waiting to be restored".to_string());
        }
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_cmd::QueueCommandConsumeOutcome;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct DiskEffects;

    struct RetainedNoopEffects {
        converges: AtomicUsize,
    }

    struct AcceptedStaleEffects;

    impl QueueCommandEffects for DiskEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }
        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            Ok(std::fs::write(file, target_content)?)
        }
        fn consume_free_text_queue_prompts_force_disk(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn strike_orphan_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!()
        }
        fn acknowledge_open_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!()
        }
        fn acknowledge_paused_free_text_queue_head(
            &self,
            _file: &Path,
            _expected_head: &str,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn prune_noise_queue_heads(&self, _file: &Path) -> Result<usize> {
            unreachable!()
        }
    }

    impl QueueCommandEffects for RetainedNoopEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }
        fn converge_document_or_disk(
            &self,
            _file: &Path,
            target_content: &str,
            source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            self.converges.fetch_add(1, Ordering::SeqCst);
            assert_eq!(target_content, source_content, "the item is already live");
            anyhow::bail!(
                "visible document write retained [{}] [recovery={}]",
                agent_doc_turn::write_ownership::RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN,
                agent_doc_turn::write_ownership::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
            )
        }
        fn consume_free_text_queue_prompts_force_disk(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn strike_orphan_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!()
        }
        fn acknowledge_open_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!()
        }
        fn acknowledge_paused_free_text_queue_head(
            &self,
            _file: &Path,
            _expected_head: &str,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn prune_noise_queue_heads(&self, _file: &Path) -> Result<usize> {
            unreachable!()
        }
    }

    impl QueueCommandEffects for AcceptedStaleEffects {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            Ok(std::fs::read_to_string(file)?)
        }
        fn converge_document_or_disk(
            &self,
            _file: &Path,
            target_content: &str,
            source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            assert!(source_content.contains("- ~~#upgrade~~\n"));
            assert!(target_content.contains("- #upgrade\n"));
            // Model a transport that accepted a semantic/exact no-op on a
            // newer struck authority cut instead of applying the restore.
            Ok(())
        }
        fn consume_free_text_queue_prompts_force_disk(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            _file: &Path,
            _count: usize,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn strike_orphan_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!()
        }
        fn acknowledge_open_id_backed_queue_head(&self, _file: &Path, _id: &str) -> Result<bool> {
            unreachable!()
        }
        fn acknowledge_paused_free_text_queue_head(
            &self,
            _file: &Path,
            _expected_head: &str,
        ) -> Result<Option<QueueCommandConsumeOutcome>> {
            unreachable!()
        }
        fn prune_noise_queue_heads(&self, _file: &Path) -> Result<usize> {
            unreachable!()
        }
    }

    fn project(content: &str) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, content).unwrap();
        (dir, doc)
    }

    fn doc(attrs: &str, body: &str) -> String {
        format!("# Plan\n\n<!-- agent:queue {attrs} -->\n{body}<!-- /agent:queue -->\n")
    }

    #[test]
    fn unscheduled_queue_holds_nothing_and_never_opens_state_db() {
        // Unscheduled content short-circuits before any state.db read.
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("plan.md");
        assert!(schedule_held_heads(&file, &doc("go", "- a\n")).is_empty());
    }

    #[test]
    fn unadmitted_schedule_holds_every_head() {
        let content = doc("interval=10m", "- a\n- b\n");
        let (_dir, file) = project(&content);
        assert_eq!(schedule_held_heads(&file, &content), vec!["a", "b"]);
        let claimed = crate::queue_claim::claimed_items_for_content(&file, &content);
        assert!(
            claimed.claims("a") && claimed.claims("b"),
            "drain paths skip held heads"
        );
        assert_eq!(
            agent_doc_queue::queue_continuation::live_drainable_continuation_head_excluding_claimed(
                &content,
                agent_doc_queue::queue_continuation::DrainScope::Supervisor,
                &claimed,
            ),
            None,
            "the supervisor does not drain a closed schedule window"
        );
    }

    #[test]
    fn hourly_issues_queue_does_not_hold_immediate_main_queue() {
        let content = concat!(
            "# Plan\n\n",
            "<!-- agent:queue id=main go -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues interval=1h loop -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, file) = project(content);
        assert_eq!(schedule_held_heads(&file, content), vec!["poll issues"]);
        let excluded = crate::queue_claim::claimed_items_for_content(&file, content);
        assert!(!excluded.claims("ship now"));
        assert!(excluded.claims("poll issues"));
        assert_eq!(
            agent_doc_queue::queue_continuation::live_drainable_continuation_head_excluding_claimed(
                content,
                agent_doc_queue::queue_continuation::DrainScope::Supervisor,
                &excluded,
            ),
            Some("ship now".to_string())
        );
        let report = schedule_status(&file, content).unwrap();
        let json = serde_json::to_value(report).unwrap();
        assert_eq!(json["queues"].as_array().unwrap().len(), 1);
        assert_eq!(json["queues"][0]["queue_id"], "issues");
        assert_eq!(json["queues"][0]["status"]["interval_secs"], 3_600);
    }

    #[test]
    fn status_identifies_each_scheduled_queue() {
        let content = concat!(
            "# Plan\n\n",
            "<!-- agent:queue id=main loop -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues interval=1h loop -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, file) = project(content);
        let json = serde_json::to_value(schedule_status(&file, content).unwrap()).unwrap();
        assert_eq!(json["queues"].as_array().unwrap().len(), 2);
        assert_eq!(json["queues"][0]["queue_id"], "main");
        assert_eq!(json["queues"][1]["queue_id"], "issues");
        assert_eq!(
            json["queues"][0]["status"]["interval_secs"],
            serde_json::Value::Null
        );
        assert_eq!(json["queues"][1]["status"]["interval_secs"], 3_600);
    }

    #[test]
    fn invalid_hourly_syntax_holds_only_the_issues_queue() {
        let content = concat!(
            "# Plan\n\n",
            "<!-- agent:queue id=main go -->\n",
            "- ship now\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:queue id=issues interval='1 hour' -->\n",
            "- poll issues\n",
            "<!-- /agent:queue -->\n",
        );
        let (_dir, file) = project(content);
        assert_eq!(schedule_held_heads(&file, content), vec!["poll issues"]);
        let json = serde_json::to_value(schedule_status(&file, content).unwrap()).unwrap();
        assert_eq!(json["queues"][0]["queue_id"], "issues");
        assert!(json["queues"][0]["status"].is_null());
        assert_eq!(json["queues"][0]["held_heads"], 1);
    }

    #[test]
    fn restore_writes_the_items_back_once() {
        // `b` has neither a live nor a struck row: it was removed from the
        // loop, so only `a` comes back (`#loopnoresurrect`).
        let content = doc("loop", "- ~~a~~\n");
        let (_dir, file) = project(&content);
        let roster = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            restore_loop_items_with_effects(&DiskEffects, &file, &roster).unwrap(),
            1
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            doc("loop", "- a\n")
        );
        assert_eq!(
            restore_loop_items_with_effects(&DiskEffects, &file, &roster).unwrap(),
            0
        );
    }

    /// `#loopnoresurrect` on the 2026-10-26 `agent-doc-bugs.md` shape: the
    /// retained 16-item `restore_pending` roster, applied after the operator
    /// cut the loop's rows into `agent:run`, leaves the document byte-identical
    /// (including the loop marker's unquoted attributes) and still succeeds, so
    /// the scheduler clears `restore_pending` instead of retrying forever.
    #[test]
    fn stale_restore_roster_is_a_byte_identical_no_op_on_the_two_queue_doc() {
        let content = concat!(
            "# Bugs\n\n",
            "## Queue: Main\n",
            "<!-- agent:run id=\"main\" subagents priority go -->\n",
            "- 📥 do [#layoutunprovenliveverify]\n",
            "- 🔄 release + publish\n",
            "<!-- /agent:run -->\n\n",
            "## Run: Issues\n",
            "<!-- agent:loop id=issues interval=1h priority -->\n",
            "<!-- /agent:loop -->\n",
        );
        let (_dir, file) = project(content);
        let roster: Vec<String> = [
            "Watch https://github.com/btakita/agent-doc/issues for new issues.",
            "do [#gh363]",
            "do [#upgradeideatest]",
            "[#coder-remote-install-02805]: done: but still not syncing",
            "📥 do [#layoutunprovenliveverify]",
            "🔄 release + publish",
        ]
        .iter()
        .map(|item| item.to_string())
        .collect();
        assert_eq!(
            restore_loop_items_for_queue_with_effects(&DiskEffects, &file, "issues", &roster)
                .unwrap(),
            0
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
    }

    #[test]
    fn already_live_restore_still_requires_delivery_convergence() {
        // GH #330: the first retained write made the restored row temporarily
        // visible. The retry used to return `Ok(0)` without proving delivery,
        // which cleared `restore_pending`; a later closeout projection could
        // then put the struck row back with no owner left to restore it.
        let content = doc("loop interval=1h", "- #upgrade\n");
        let (_dir, file) = project(&content);
        let effects = RetainedNoopEffects {
            converges: AtomicUsize::new(0),
        };

        let err = restore_loop_items_with_effects(&effects, &file, &["#upgrade".to_string()])
            .expect_err("unsettled delivery must keep the restore pending");

        assert!(
            agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(&format!(
                "{err:#}"
            )),
            "the scheduler needs the retained marker to back off and retry: {err:#}"
        );
        assert_eq!(effects.converges.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn accepted_transport_noop_does_not_claim_a_struck_roster_was_restored() {
        let struck = doc("loop interval=1h", "- ~~#upgrade~~\n");
        let (_dir, file) = project(&struck);

        let err = restore_loop_items_for_queue_from_source_with_effects(
            &AcceptedStaleEffects,
            &file,
            agent_doc_queue::queue_graph::DEFAULT_QUEUE_ID,
            &["#upgrade".to_string()],
            &struck,
        )
        .expect_err("a transport receipt cannot replace visible roster proof");

        assert!(
            err.to_string().contains("did not make its roster visible"),
            "{err:#}"
        );
        assert_eq!(std::fs::read_to_string(&file).unwrap(), struck);
    }

    #[test]
    fn status_reports_pending_admission_then_render() {
        let content = doc("interval=10m loop=3", "- a\n");
        let (_dir, file) = project(&content);
        let report = schedule_status(&file, &content).unwrap();
        let rendered = render_schedule_report(&file, &report);
        assert!(
            rendered.contains("waiting for the project controller"),
            "{rendered}"
        );
        assert!(rendered.contains("loop 0/3"), "{rendered}");
        let json = serde_json::to_value(&report).unwrap();
        assert_eq!(json["scheduled"], "true");
        assert_eq!(json["pending_admission"], true);
        assert_eq!(json["queues"][0]["queue_id"], "default");
        assert_eq!(json["queues"][0]["status"]["pending_admission"], true);
        let unscheduled = schedule_status(&file, &doc("go", "- a\n")).unwrap();
        assert_eq!(
            serde_json::to_value(&unscheduled).unwrap()["scheduled"],
            "false"
        );
    }

    /// `#idlessqueueidentity`: `agent-doc queue schedule` on the session
    /// shape whose operator deleted the `id=` attributes from one `agent:run`
    /// and one `agent:loop` used to fail with "occurrence 0 requires a
    /// nonempty explicit `id=`". The loop now reports as `loop`, and the old
    /// ids keep reporting as before.
    #[test]
    fn schedule_status_names_an_idless_loop_by_its_kind() {
        for (ids, loop_id) in [(false, "loop"), (true, "issues")] {
            let (run_id, loop_attr) = if ids {
                ("id=\"main\" ", "id=\"issues\" ")
            } else {
                ("", "")
            };
            let content = format!(
                "---\nagent_doc_session: idless\nqueue: go\n---\n\n\
## Run <!-- agent:run {run_id}subagents priority go -->\n\
- Why have the ☑️ run items not completed?\n\
<!-- /agent:run -->\n\n\
## Loop <!-- agent:loop {loop_attr}interval=\"1h\" priority -->\n\
- 📥 Watch https://github.com/btakita/agent-doc/issues for new issues.\n\
<!-- /agent:loop -->\n\n\
## Backlog <!-- agent:backlog priority queue -->\n<!-- /agent:backlog -->\n"
            );
            let (_dir, file) = project(&content);
            let report =
                schedule_status(&file, &content).unwrap_or_else(|err| panic!("ids={ids}: {err:#}"));
            let json = serde_json::to_value(&report).unwrap();
            assert_eq!(json["scheduled"], "true", "{json}");
            assert_eq!(json["queues"].as_array().unwrap().len(), 1, "{json}");
            assert_eq!(json["queues"][0]["queue_id"], loop_id, "{json}");
        }
    }
}
