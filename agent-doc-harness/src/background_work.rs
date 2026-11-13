//! A worker that stopped its turn while its own background work still runs
//! (`#waitingfreezelive`).
//!
//! Claude Code ends a subagent's turn when the model stops calling tools, even
//! while a command it backgrounded (`run_in_background`, an auto-backgrounded
//! long Bash call, a `Monitor`, an async `Agent`) is still in flight. The
//! harness then fires `SubagentStop` and tells the parent
//! `Agent "..." is waiting on its own background work`: the worker resumes
//! when that work reports. That stop is proof of life, not a worker end, so
//! the subagent-end hook must not freeze the attempt.
//!
//! The `SubagentStop` payload carries `background_tasks`: every in-flight task
//! in the whole session, with no owner. The worker's own transcript names the
//! task ids it launched, so the intersection is that worker's live work. When
//! a harness omits `background_tasks`, the transcript alone decides: a launched
//! id with no terminal `<task-notification>` is still running.

use std::collections::HashSet;

use serde_json::Value;

/// One in-flight task from a hook payload's `background_tasks`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessBackgroundTask {
    pub id: String,
    /// Harness label: `shell`, `subagent`, `monitor`, `workflow`, ...
    pub kind: Option<String>,
    pub status: Option<String>,
}

impl HarnessBackgroundTask {
    /// The harness lists in-flight work; a terminal status is defensive.
    pub fn in_flight(&self) -> bool {
        !self.status.as_deref().is_some_and(status_is_terminal)
    }
}

/// `background_tasks` / `backgroundTasks`, or `None` when the payload does not
/// carry the field. `Some(vec![])` means the harness says nothing is in flight.
pub fn parse_background_tasks(value: &Value) -> Option<Vec<HarnessBackgroundTask>> {
    let list = ["background_tasks", "backgroundTasks"]
        .iter()
        .find_map(|key| value.get(*key)?.as_array())?;
    Some(
        list.iter()
            .filter_map(|entry| {
                let id = entry.get("id")?.as_str()?.trim();
                (!id.is_empty()).then(|| HarnessBackgroundTask {
                    id: id.to_string(),
                    kind: text(entry, "type"),
                    status: text(entry, "status"),
                })
            })
            .collect(),
    )
}

/// One `<task-notification>` block as Claude Code delivers it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskNotification {
    pub task_id: String,
    pub status: Option<String>,
    pub summary: Option<String>,
    pub note: Option<String>,
}

impl TaskNotification {
    /// `Agent "..." is waiting on its own background work`: the agent stopped
    /// its turn with its own background work still running and may resume.
    /// The harness still says `<status>completed</status>`, so the status
    /// alone must not be read as a terminal end.
    pub fn waiting_on_own_background_work(&self) -> bool {
        self.summary
            .as_deref()
            .is_some_and(|summary| summary.contains("is waiting on its own background work"))
            || self.note.as_deref().is_some_and(|note| {
                note.contains("stopped with background work of its own still running")
            })
    }

    /// The task reached an end: it will not report again.
    pub fn is_terminal(&self) -> bool {
        self.status.as_deref().is_some_and(status_is_terminal)
            && !self.waiting_on_own_background_work()
    }
}

/// Every `<task-notification>` block in `text`, in order.
pub fn parse_task_notifications(text: &str) -> Vec<TaskNotification> {
    const OPEN: &str = "<task-notification>";
    const CLOSE: &str = "</task-notification>";
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find(OPEN) {
        let body_start = start + OPEN.len();
        let body_len = rest[body_start..].find(CLOSE);
        let body = match body_len {
            Some(len) => &rest[body_start..body_start + len],
            None => &rest[body_start..],
        };
        if let Some(task_id) = tag(body, "task-id") {
            out.push(TaskNotification {
                task_id,
                status: tag(body, "status"),
                summary: tag(body, "summary"),
                note: tag(body, "note"),
            });
        }
        rest = match body_len {
            Some(len) => &rest[body_start + len + CLOSE.len()..],
            None => "",
        };
    }
    out
}

/// Background task ids a worker launched, and those that already ended,
/// read from its own JSONL transcript.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorkerBackgroundLaunches {
    /// Launch order, without duplicates.
    pub launched: Vec<String>,
    pub ended: HashSet<String>,
}

impl WorkerBackgroundLaunches {
    /// Whether a JSONL line can matter. Lets the IO side skip parsing the
    /// rest of a multi-megabyte transcript.
    pub fn line_may_matter(line: &str) -> bool {
        line.contains("backgroundTaskId")
            || line.contains("\"taskId\"")
            || line.contains("async_launched")
            || line.contains("<task-notification>")
    }

    /// Fold one transcript line in.
    pub fn observe_line(&mut self, line: &str) {
        if !Self::line_may_matter(line) {
            return;
        }
        let Ok(record) = serde_json::from_str::<Value>(line) else {
            return;
        };
        if let Some(result) = record.get("toolUseResult") {
            for id in launched_ids(result) {
                if !self.launched.contains(&id) {
                    self.launched.push(id);
                }
            }
        }
        if line.contains("<task-notification>") {
            let mut texts = Vec::new();
            collect_strings(record.get("message").unwrap_or(&record), &mut texts);
            for text in texts {
                for notice in parse_task_notifications(text) {
                    if notice.is_terminal() {
                        self.ended.insert(notice.task_id);
                    }
                }
            }
        }
    }

    pub fn from_transcript(jsonl: &str) -> Self {
        let mut launches = Self::default();
        for line in jsonl.lines() {
            launches.observe_line(line);
        }
        launches
    }
}

/// Why this worker's stop is proof of life.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OwnBackgroundWork {
    /// The harness lists these ids in flight, and the worker launched them.
    HarnessInFlight(Vec<String>),
    /// The payload carries no `background_tasks`; the worker launched these
    /// ids and no terminal notification for them reached its transcript.
    TranscriptUnsettled(Vec<String>),
}

impl OwnBackgroundWork {
    pub fn task_ids(&self) -> &[String] {
        match self {
            Self::HarnessInFlight(ids) | Self::TranscriptUnsettled(ids) => ids,
        }
    }
}

/// The worker's own background work still in flight at this stop, if any.
///
/// `harness_tasks` is the payload's `background_tasks` (`None` when absent).
/// `launches` is `None` when the worker transcript could not be read: the
/// session-wide list cannot be attributed then, so the answer is `None`.
pub fn own_background_work(
    harness_tasks: Option<&[HarnessBackgroundTask]>,
    launches: Option<&WorkerBackgroundLaunches>,
) -> Option<OwnBackgroundWork> {
    let launches = launches?;
    let ids = match harness_tasks {
        Some(tasks) => {
            let in_flight = tasks
                .iter()
                .filter(|task| task.in_flight())
                .map(|task| task.id.as_str())
                .collect::<HashSet<_>>();
            let owned = launches
                .launched
                .iter()
                .filter(|id| in_flight.contains(id.as_str()))
                .cloned()
                .collect::<Vec<_>>();
            return (!owned.is_empty()).then_some(OwnBackgroundWork::HarnessInFlight(owned));
        }
        None => launches
            .launched
            .iter()
            .filter(|id| !launches.ended.contains(*id))
            .cloned()
            .collect::<Vec<_>>(),
    };
    (!ids.is_empty()).then_some(OwnBackgroundWork::TranscriptUnsettled(ids))
}

fn launched_ids(result: &Value) -> Vec<String> {
    let mut ids = Vec::new();
    // Bash `run_in_background`, or a long Bash call the harness backgrounded.
    if let Some(id) = text(result, "backgroundTaskId") {
        ids.push(id);
    }
    // `Monitor`.
    if let Some(id) = text(result, "taskId") {
        ids.push(id);
    }
    // An async `Agent`: its task id is its agent id.
    if text(result, "status").as_deref() == Some("async_launched")
        && let Some(id) = text(result, "agentId")
    {
        ids.push(id);
    }
    ids
}

fn status_is_terminal(status: &str) -> bool {
    matches!(
        status.trim().to_ascii_lowercase().as_str(),
        "completed" | "failed" | "killed" | "stopped" | "cancelled" | "canceled" | "error"
    )
}

fn text(value: &Value, key: &str) -> Option<String> {
    let text = value.get(key)?.as_str()?.trim();
    (!text.is_empty()).then(|| text.to_string())
}

fn tag(body: &str, name: &str) -> Option<String> {
    let open = format!("<{name}>");
    let close = format!("</{name}>");
    let start = body.find(&open)? + open.len();
    let len = body[start..].find(&close)?;
    let value = body[start..start + len].trim();
    (!value.is_empty()).then(|| value.to_string())
}

fn collect_strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        Value::Object(map) => map.values().for_each(|item| collect_strings(item, out)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    /// Verbatim notification the coordinator received on 2026-11-13 for the
    /// #gh410 worker that agent-doc froze while `make tmux-ci` ran.
    const WAITING_NOTICE: &str = "<task-notification>\n<task-id>a5cfda4a2bfde1278</task-id>\n<tool-use-id>toolu_01LJ4fniCL7aY2fMYh9uuefK</tool-use-id>\n<output-file>/tmp/claude-1000/-home-brian-work-btakita-agent-loop/680c36c0-3d4c-4ee0-b292-eea1130f79bf/tasks/a5cfda4a2bfde1278.output</output-file>\n<status>completed</status>\n<summary>Agent \"Fix agent-doc GH issue 410\" is waiting on its own background work</summary>\n<note>This agent stopped with background work of its own still running. It may resume on its own when that work completes or reports, and the same task-id notifies again if it does; the result below may be interim.</note>\n<result>`make check` is still running in the background. I'll continue when it finishes.</result>\n<usage><subagent_tokens>237967</subagent_tokens><tool_uses>106</tool_uses><duration_ms>751924</duration_ms></usage>\n</task-notification>";

    const FINISHED_NOTICE: &str = "<task-notification>\n<task-id>b3rik1w2m</task-id>\n<tool-use-id>toolu_01XUSGotTNf2YYQWHiu2YCpW</tool-use-id>\n<output-file>/tmp/x/tasks/b3rik1w2m.output</output-file>\n<status>completed</status>\n<summary>Background command \"Run make tmux-ci in worktree\" completed (exit code 0)</summary>\n</task-notification>";

    fn launch_line(id: &str) -> String {
        json!({
            "type": "user",
            "isSidechain": true,
            "message": {"role": "user", "content": [{
                "tool_use_id": "toolu_01XUSGotTNf2YYQWHiu2YCpW",
                "type": "tool_result",
                "content": format!("Command running in background with ID: {id}."),
            }]},
            "toolUseResult": {
                "stdout": "", "stderr": "", "interrupted": false,
                "backgroundTaskId": id,
            },
        })
        .to_string()
    }

    fn notice_line(notice: &str) -> String {
        json!({
            "type": "user",
            "isMeta": true,
            "message": {"role": "user", "content": format!(
                "[SYSTEM NOTIFICATION - NOT USER INPUT]\nThis is an automated background-task event.\n\n{notice}"
            )},
        })
        .to_string()
    }

    #[test]
    fn the_waiting_notice_is_proof_of_life_not_a_terminal_completion() {
        let notices = parse_task_notifications(WAITING_NOTICE);
        assert_eq!(notices.len(), 1);
        let notice = &notices[0];
        assert_eq!(notice.task_id, "a5cfda4a2bfde1278");
        assert_eq!(notice.status.as_deref(), Some("completed"));
        assert!(notice.waiting_on_own_background_work());
        assert!(!notice.is_terminal());

        let finished = &parse_task_notifications(FINISHED_NOTICE)[0];
        assert!(!finished.waiting_on_own_background_work());
        assert!(finished.is_terminal());
    }

    #[test]
    fn the_session_wide_list_counts_only_ids_this_worker_launched() {
        let transcript = launch_line("b3rik1w2m");
        let launches = WorkerBackgroundLaunches::from_transcript(&transcript);
        assert_eq!(launches.launched, vec!["b3rik1w2m".to_string()]);
        let payload = json!({
            "hook_event_name": "SubagentStop",
            "agent_id": "a5cfda4a2bfde1278",
            "last_assistant_message": "The commit is in. Waiting on `make tmux-ci` before I push.",
            "background_tasks": [
                {"id": "b3rik1w2m", "type": "shell", "status": "running",
                 "description": "Run make tmux-ci in worktree", "command": "make tmux-ci"},
                {"id": "a01da8a28a9c64248", "type": "subagent", "status": "running",
                 "description": "a sibling worker", "agent_type": "general-purpose"},
            ],
        });
        let tasks = parse_background_tasks(&payload).expect("field present");
        assert_eq!(
            own_background_work(Some(&tasks), Some(&launches)),
            Some(OwnBackgroundWork::HarnessInFlight(vec![
                "b3rik1w2m".to_string()
            ]))
        );

        // A sibling's or the coordinator's work is not this worker's.
        let others = &tasks[1..];
        assert_eq!(own_background_work(Some(others), Some(&launches)), None);
        // An empty list is the harness saying nothing is in flight.
        assert_eq!(own_background_work(Some(&[]), Some(&launches)), None);
        // Without the worker transcript the list cannot be attributed.
        assert_eq!(own_background_work(Some(&tasks), None), None);
    }

    #[test]
    fn without_the_field_an_unsettled_launch_in_the_transcript_counts() {
        let payload = json!({"hook_event_name": "SubagentStop", "agent_id": "w"});
        assert_eq!(parse_background_tasks(&payload), None);

        let running = launch_line("b3rik1w2m");
        let launches = WorkerBackgroundLaunches::from_transcript(&running);
        assert_eq!(
            own_background_work(None, Some(&launches)),
            Some(OwnBackgroundWork::TranscriptUnsettled(vec![
                "b3rik1w2m".to_string()
            ]))
        );

        let settled = format!("{running}\n{}", notice_line(FINISHED_NOTICE));
        let launches = WorkerBackgroundLaunches::from_transcript(&settled);
        assert!(launches.ended.contains("b3rik1w2m"));
        assert_eq!(own_background_work(None, Some(&launches)), None);
    }

    #[test]
    fn a_nested_agent_waiting_on_its_own_work_has_not_ended() {
        let launch = json!({
            "type": "user",
            "toolUseResult": {"isAsync": true, "status": "async_launched",
                              "agentId": "a5cfda4a2bfde1278"},
        })
        .to_string();
        let transcript = format!("{launch}\n{}", notice_line(WAITING_NOTICE));
        let launches = WorkerBackgroundLaunches::from_transcript(&transcript);
        assert_eq!(launches.launched, vec!["a5cfda4a2bfde1278".to_string()]);
        assert!(launches.ended.is_empty());
        assert!(own_background_work(None, Some(&launches)).is_some());
    }

    #[test]
    fn monitor_launches_are_background_work_and_unrelated_lines_are_skipped() {
        let monitor = json!({
            "type": "user",
            "toolUseResult": {"taskId": "brpzhsse1", "timeoutMs": 1800000, "persistent": false},
        })
        .to_string();
        let plain = json!({"type": "user", "toolUseResult": {"stdout": "ok"}}).to_string();
        let launches = WorkerBackgroundLaunches::from_transcript(&format!("{plain}\n{monitor}"));
        assert_eq!(launches.launched, vec!["brpzhsse1".to_string()]);
        assert!(!WorkerBackgroundLaunches::line_may_matter(&plain));
    }
}
