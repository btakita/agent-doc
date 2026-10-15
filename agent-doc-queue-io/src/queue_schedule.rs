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
    if !queue_schedule::content_is_scheduled(content) {
        return Vec::new();
    }
    match agent_doc_controller_io::queue_scheduler::load_schedule_state(file) {
        Ok(state) => queue_schedule::held_heads(content, state.as_ref()),
        Err(err) => {
            eprintln!(
                "[queue-schedule] WARNING: could not read the queue schedule for {}; holding its heads: {err:#}",
                file.display()
            );
            queue_schedule::live_heads(content)
        }
    }
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
    // `#sqedit-race`: one queue writer at a time.
    let _queue_edit_guard = crate::queue_edit_owner::QueueEditGuard::acquire(file);
    let current = effects.current_document_content(file, "queue_schedule_loop_restore")?;
    let Some(updated) = queue_schedule::plan_loop_restore(&current, roster)? else {
        return Ok(0);
    };
    let before = queue_schedule::live_heads(&current).len();
    effects.converge_document_or_disk(file, &updated, &current, "queue_schedule_loop_restore")?;
    let restored = queue_schedule::live_heads(&updated)
        .len()
        .saturating_sub(before);
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "queue_schedule_loop_restore_write roster={} restored={restored}",
            roster.len()
        ),
    );
    Ok(restored)
}

/// The document's schedule, as `agent-doc queue schedule` reports it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "scheduled", rename_all = "snake_case")]
pub enum ScheduleReport {
    #[serde(rename = "false")]
    Unscheduled,
    #[serde(rename = "true")]
    Scheduled {
        #[serde(flatten)]
        status: queue_schedule::QueueScheduleStatus,
        held_heads: usize,
    },
}

pub fn schedule_status(file: &Path, content: &str) -> Result<ScheduleReport> {
    let Some(spec) = queue_schedule::queue_schedule_spec(content) else {
        return Ok(ScheduleReport::Unscheduled);
    };
    let state = agent_doc_controller_io::queue_scheduler::load_schedule_state(file)?;
    Ok(ScheduleReport::Scheduled {
        status: queue_schedule::status(
            &spec,
            state.as_ref(),
            &agent_doc_controller_io::queue_scheduler::SystemLocalOffset,
        ),
        held_heads: queue_schedule::held_heads(content, state.as_ref()).len(),
    })
}

/// Render [`ScheduleReport`] for a terminal.
pub fn render_schedule_report(file: &Path, report: &ScheduleReport) -> String {
    let ScheduleReport::Scheduled { status, held_heads } = report else {
        return format!(
            "{}: agent:queue is not scheduled (no interval/time/loop attribute)",
            file.display()
        );
    };
    let when = |secs: Option<u64>| {
        secs.map_or_else(|| "-".to_string(), agent_doc_log_time::format_log_timestamp)
    };
    let cadence = match (status.interval_secs, status.times.is_empty()) {
        (Some(interval), true) => format!("every {interval}s"),
        (Some(interval), false) => format!("every {interval}s and at {}", status.times.join(",")),
        (None, false) => format!("at {}", status.times.join(",")),
        (None, true) => "after each pass".to_string(),
    };
    let passes = match status.loop_limit {
        queue_schedule::LoopLimit::Off => format!("{} passes run", status.passes_completed),
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
        None => format!("idle, {held_heads} head(s) held"),
    };
    let mut lines = vec![
        format!("{}: scheduled queue ({cadence})", file.display()),
        format!("  state: {window}; {passes}"),
        format!(
            "  next run: {}; last tick: {}",
            when(status.next_run_secs),
            when(status.last_fired_at_secs)
        ),
    ];
    if status.failures > 0 {
        lines.push(format!(
            "  backing off after {} failure(s) until {}",
            status.failures,
            when(status.backoff_until_secs)
        ));
    }
    if status.restore_pending {
        lines.push("  loop items waiting to be restored".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_cmd::QueueCommandConsumeOutcome;
    use std::path::PathBuf;

    struct DiskEffects;

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
    fn restore_writes_the_items_back_once() {
        let content = doc("loop", "- ~~a~~\n");
        let (_dir, file) = project(&content);
        let roster = vec!["a".to_string(), "b".to_string()];
        assert_eq!(
            restore_loop_items_with_effects(&DiskEffects, &file, &roster).unwrap(),
            2
        );
        assert_eq!(
            std::fs::read_to_string(&file).unwrap(),
            doc("loop", "- a\n- b\n")
        );
        assert_eq!(
            restore_loop_items_with_effects(&DiskEffects, &file, &roster).unwrap(),
            0
        );
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
        let unscheduled = schedule_status(&file, &doc("go", "- a\n")).unwrap();
        assert_eq!(
            serde_json::to_value(&unscheduled).unwrap()["scheduled"],
            "false"
        );
    }
}
