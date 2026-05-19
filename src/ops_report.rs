//! Operational log reports for `.agent-doc/logs/ops.log`.

use anyhow::{Context, Result};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const DEFAULT_LIMIT: usize = 1000;
const MAX_SAMPLES_PER_BUCKET: usize = 3;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpsSummaryReport {
    pub log_path: PathBuf,
    pub scanned_lines: usize,
    pub matched_events: usize,
    pub buckets: Vec<OpsSummaryBucket>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpsSummaryBucket {
    pub category: String,
    pub file: String,
    pub session: String,
    pub count: usize,
    pub latest_timestamp: Option<u64>,
    pub samples: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BucketKey {
    category: String,
    file: String,
    session: String,
}

#[derive(Debug, Clone)]
struct ClassifiedEvent {
    timestamp: Option<u64>,
    category: &'static str,
    file: Option<String>,
    session: Option<String>,
    detail: String,
}

pub fn run_summary(project_root: Option<&Path>, limit: usize, json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root = match project_root {
        Some(root) => root.to_path_buf(),
        None => crate::snapshot::find_project_root(&cwd).unwrap_or(cwd),
    };
    let log_path = root.join(".agent-doc/logs/ops.log");
    let contents = std::fs::read_to_string(&log_path)
        .with_context(|| format!("failed to read {}", log_path.display()))?;
    let report = summarize_ops_log(&contents, &root, limit, log_path);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_summary(&report);
    }
    Ok(())
}

pub fn summarize_ops_log(
    contents: &str,
    project_root: &Path,
    limit: usize,
    log_path: PathBuf,
) -> OpsSummaryReport {
    let all_lines: Vec<&str> = contents.lines().collect();
    let start = if limit == 0 || limit >= all_lines.len() {
        0
    } else {
        all_lines.len() - limit
    };
    let lines = &all_lines[start..];

    let mut buckets: BTreeMap<BucketKey, OpsSummaryBucket> = BTreeMap::new();
    let mut matched_events = 0;

    for line in lines {
        let Some(event) = classify_line(line, project_root) else {
            continue;
        };
        matched_events += 1;

        let key = BucketKey {
            category: event.category.to_string(),
            file: event.file.unwrap_or_else(|| "<global>".to_string()),
            session: event.session.unwrap_or_else(|| "-".to_string()),
        };
        let bucket = buckets
            .entry(key.clone())
            .or_insert_with(|| OpsSummaryBucket {
                category: key.category,
                file: key.file,
                session: key.session,
                count: 0,
                latest_timestamp: None,
                samples: Vec::new(),
            });

        bucket.count += 1;
        if event.timestamp > bucket.latest_timestamp {
            bucket.latest_timestamp = event.timestamp;
        }
        if bucket.samples.len() == MAX_SAMPLES_PER_BUCKET {
            bucket.samples.remove(0);
        }
        bucket.samples.push(event.detail);
    }

    let mut buckets: Vec<OpsSummaryBucket> = buckets.into_values().collect();
    buckets.sort_by(|a, b| {
        b.latest_timestamp
            .cmp(&a.latest_timestamp)
            .then_with(|| a.category.cmp(&b.category))
            .then_with(|| a.file.cmp(&b.file))
            .then_with(|| a.session.cmp(&b.session))
    });

    OpsSummaryReport {
        log_path,
        scanned_lines: lines.len(),
        matched_events,
        buckets,
    }
}

fn print_human_summary(report: &OpsSummaryReport) {
    println!(
        "ops summary: {} (matched {} events across {} scanned lines)",
        report.log_path.display(),
        report.matched_events,
        report.scanned_lines
    );
    if report.buckets.is_empty() {
        println!("no tracked events found");
        return;
    }

    let mut current_category = "";
    for bucket in &report.buckets {
        if bucket.category != current_category {
            current_category = &bucket.category;
            println!("\n{}", current_category);
        }
        let latest = bucket
            .latest_timestamp
            .map(|ts| ts.to_string())
            .unwrap_or_else(|| "-".to_string());
        println!(
            "  {} session={} count={} latest={}",
            bucket.file, bucket.session, bucket.count, latest
        );
        for sample in &bucket.samples {
            println!("    - {}", sample);
        }
    }
}

fn classify_line(line: &str, project_root: &Path) -> Option<ClassifiedEvent> {
    let (timestamp, message) = parse_log_line(line);
    let event_name = message.split_whitespace().next()?;
    let fields = parse_fields(message);

    let category = match event_name {
        "flow_event" => classify_flow_event(&fields),
        "ipc_write_consumed" => "write ipc consumed",
        "commit_success" => "commit success",
        "commit_noop" if field_eq(&fields, "drift_kind", "user_follow_up") => {
            "expected user follow-up noop"
        }
        "commit_noop" if field_eq(&fields, "drift_kind", "working_tree_edits") => {
            "anomalous drift noop"
        }
        "commit_noop" if field_eq(&fields, "drift_kind", "none") => "expected already-current noop",
        "commit_noop" => "commit noop",
        "route_dispatch_start_proven" => "route dispatch proven",
        "post_commit_user_follow_up" => "expected user follow-up",
        "post_commit_local_drift" if field_eq(&fields, "kind", "user_follow_up") => {
            "expected user follow-up"
        }
        "post_commit_local_drift" => "anomalous post-commit drift",
        "session_clear_active_pane_allowed" => "active clear allowed",
        "session_clear_protected_input_guard_refused" => "expected protected-input clear refusal",
        "session_clear_live_busy_guard_bypassed" => "busy clear bypassed",
        "session_clear_live_busy_guard_refused" => "busy clear refused",
        "route_authoritative_actor_starting_not_ready" => "starting actor not ready",
        "route_dispatch_start_unproven_but_accepted" => "accepted-only route proof",
        "route_dispatch_only_sent" if field_eq(&fields, "proof_scope", "accepted_only") => {
            "accepted-only route proof"
        }
        "route_dispatch_only_submit_unproven" => "dispatch-only not proven",
        "sync_latency" if field_eq(&fields, "status", "over_budget") => "sync over budget",
        _ => return None,
    };

    Some(ClassifiedEvent {
        timestamp,
        category,
        file: fields
            .get("file")
            .map(|file| normalize_file_for_report(file, project_root)),
        session: fields.get("session").cloned(),
        detail: render_detail(event_name, &fields),
    })
}

fn classify_flow_event(fields: &BTreeMap<String, String>) -> &'static str {
    match (
        fields.get("flow").map(String::as_str),
        fields.get("stage").map(String::as_str),
        fields.get("outcome").map(String::as_str),
    ) {
        (Some("routed_reopen"), Some("prompt_ready_barrier"), Some("failed_closed")) => {
            "flow routed reopen prompt-ready failures"
        }
        (Some("routed_reopen"), Some("dispatch_submit"), Some("failed_closed")) => {
            "flow routed reopen dispatch failures"
        }
        (Some("document_mutation"), Some("patchback_parse"), Some("failed_closed")) => {
            "flow document mutation parse failures"
        }
        (Some("document_mutation"), Some("patchback_parse"), Some("completed")) => {
            "flow document mutation parsed"
        }
        (Some("closeout"), Some("pre_write_guard"), Some("blocked" | "failed_closed")) => {
            "flow closeout pre-write guard blocked"
        }
        (Some("closeout"), Some("pre_commit_guard"), Some("blocked" | "failed_closed")) => {
            "flow closeout pre-commit guard blocked"
        }
        (Some("closeout"), Some("terminal_guard"), Some("blocked" | "failed_closed")) => {
            "flow closeout terminal guard blocked"
        }
        (Some("closeout"), Some("session_check"), Some("blocked" | "failed_closed")) => {
            "flow closeout session-check failures"
        }
        (Some("closeout"), Some("commit"), Some("completed")) => "flow closeout commit completed",
        (Some("closeout"), Some("commit"), Some("blocked" | "failed_closed")) => {
            "flow closeout commit failures"
        }
        (
            Some("orchestration_batch"),
            Some("child_closeout"),
            Some("blocked" | "failed_closed"),
        ) => "flow orchestration child closeout failures",
        (Some("operator_clear"), Some("operator_guard"), Some("blocked" | "failed_closed")) => {
            "flow operator clear guard failures"
        }
        (Some("session_cycle"), _, Some("blocked" | "failed_closed")) => {
            "flow session cycle failures"
        }
        (Some("routed_reopen"), _, Some("blocked" | "failed_closed")) => {
            "flow routed reopen failures"
        }
        (Some("closeout"), _, Some("blocked" | "failed_closed")) => "flow closeout failures",
        _ => "flow events",
    }
}

fn parse_log_line(line: &str) -> (Option<u64>, &str) {
    if let Some(rest) = line.strip_prefix('[')
        && let Some((ts, message)) = rest.split_once("] ")
    {
        return (ts.parse::<u64>().ok(), message);
    }
    (None, line)
}

fn parse_fields(message: &str) -> BTreeMap<String, String> {
    let mut fields = BTreeMap::new();
    for token in message.split_whitespace().skip(1) {
        let Some((key, value)) = token.split_once('=') else {
            continue;
        };
        fields.insert(key.to_string(), trim_field_value(value).to_string());
    }
    fields
}

fn trim_field_value(value: &str) -> &str {
    value.trim_matches(',').trim_matches('"').trim_matches('\'')
}

fn field_eq(fields: &BTreeMap<String, String>, key: &str, expected: &str) -> bool {
    fields.get(key).is_some_and(|value| value == expected)
}

fn normalize_file_for_report(file: &str, project_root: &Path) -> String {
    let path = Path::new(file);
    if path.is_absolute()
        && let Ok(relative) = path.strip_prefix(project_root)
    {
        return relative.to_string_lossy().to_string();
    }
    file.to_string()
}

fn render_detail(event_name: &str, fields: &BTreeMap<String, String>) -> String {
    let mut parts = vec![event_name.to_string()];
    for key in [
        "kind",
        "reason",
        "drift_kind",
        "basis",
        "pane",
        "source",
        "current_command",
        "harness",
        "proof",
        "proof_scope",
        "phase",
        "mode",
        "elapsed_ms",
        "budget_ms",
        "patches",
        "generation",
        "actor_state",
        "flow",
        "stage",
        "outcome",
    ] {
        if let Some(value) = fields.get(key) {
            parts.push(format!("{key}={value}"));
        }
    }
    parts.join(" ")
}

pub const fn default_summary_limit() -> usize {
    DEFAULT_LIMIT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ops_summary_groups_tracked_events_by_file_and_session() {
        let root = Path::new("/repo");
        let log = "\
[100] ipc_write_consumed file=/repo/tasks/a.md patches=1
[101] commit_success file=/repo/tasks/a.md
[102] route_dispatch_start_proven file=tasks/a.md pane=%1 harness=codex proof=consumed timeout_secs=10
[103] route_dispatch_only_sent file=tasks/b.md pane=%2 harness=opencode proof=accepted proof_scope=accepted_only
[104] route_dispatch_only_submit_unproven file=tasks/b.md pane=%2 harness=opencode delivery=direct_pane_submit submit_mode=tmux_literal_enter_delayed proof=accepted proof_scope=accepted_only timeout_secs=10
[105] post_commit_local_drift file=/repo/tasks/a.md kind=user_follow_up basis=head
[106] post_commit_user_follow_up file=/repo/tasks/a.md basis=head
[107] post_commit_local_drift file=/repo/tasks/a.md kind=working_tree_edits basis=head
[108] commit_noop file=/repo/tasks/a.md reason=already_current drift_kind=user_follow_up basis=head
[109] commit_noop file=/repo/tasks/a.md reason=already_current drift_kind=working_tree_edits basis=head
[110] commit_noop file=/repo/tasks/a.md reason=already_current drift_kind=none basis=head
[111] session_clear_active_pane_allowed file=/repo/tasks/a.md pane=%1 source=authoritative_actor current_command=agent-doc
[112] session_clear_protected_input_guard_refused file=/repo/tasks/a.md pane=%1 source=authoritative_actor reason=drafted_prompt_input current_command=agent-doc
[113] session_clear_live_busy_guard_bypassed file=/repo/tasks/a.md pane=%1 source=authoritative_actor current_command=agent-doc
[114] session_clear_live_busy_guard_refused file=/repo/tasks/a.md pane=%1 source=authoritative_actor current_command=agent-doc
[115] route_authoritative_actor_starting_not_ready file=tasks/c.md pane=%3 harness=codex generation=9 actor_state=starting
[116] sync_latency phase=prune_stash_panes elapsed_ms=309 budget_ms=250 status=over_budget mode=full
[117] flow_event file=/repo/tasks/a.md flow=closeout stage=commit outcome=completed reason=already_current
[118] flow_event file=/repo/tasks/c.md flow=routed_reopen stage=prompt_ready_barrier outcome=failed_closed reason=starting_actor_not_ready
[119] flow_event file=/repo/tasks/a.md flow=document_mutation stage=patchback_parse outcome=failed_closed reason=malformed_patchback
[120] flow_event file=/repo/tasks/a.md flow=closeout stage=pre_write_guard outcome=blocked reason=pending_capture_recommendations
[121] flow_event file=/repo/tasks/a.md flow=closeout stage=terminal_guard outcome=blocked reason=already_committed
[122] flow_event file=/repo/tasks/a.md flow=document_mutation stage=patchback_parse outcome=completed reason=valid_patch
[123] controller_supervisor_heartbeat session=s1 pane=%1 generation=3 state=ready
";

        let report =
            summarize_ops_log(log, root, 0, PathBuf::from("/repo/.agent-doc/logs/ops.log"));

        assert_eq!(report.scanned_lines, 24);
        assert_eq!(report.matched_events, 23);
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "write ipc consumed"
                    && bucket.file == "tasks/a.md"
                    && bucket.count == 1
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "accepted-only route proof"
                    && bucket.file == "tasks/b.md"
                    && bucket.samples[0].contains("proof_scope=accepted_only")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "dispatch-only not proven"
                    && bucket.file == "tasks/b.md"
                    && bucket.samples[0].contains("route_dispatch_only_submit_unproven")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "expected user follow-up"
                    && bucket.file == "tasks/a.md"
                    && bucket.count == 2
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("kind=user_follow_up"))
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "anomalous post-commit drift"
                    && bucket.file == "tasks/a.md"
                    && bucket.samples[0].contains("kind=working_tree_edits")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "expected user follow-up noop"
                    && bucket.file == "tasks/a.md"
                    && bucket.samples[0].contains("drift_kind=user_follow_up")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "anomalous drift noop"
                    && bucket.file == "tasks/a.md"
                    && bucket.samples[0].contains("drift_kind=working_tree_edits")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "expected already-current noop"
                    && bucket.file == "tasks/a.md"
                    && bucket.samples[0].contains("drift_kind=none")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "expected protected-input clear refusal"
                    && bucket.file == "tasks/a.md"
                    && bucket.samples[0].contains("current_command=agent-doc")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "sync over budget"
                    && bucket.file == "<global>"
                    && bucket.samples[0].contains("phase=prune_stash_panes")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow routed reopen prompt-ready failures"
                    && bucket.file == "tasks/c.md"
                    && bucket.samples[0].contains("stage=prompt_ready_barrier")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow document mutation parse failures"
                    && bucket.file == "tasks/a.md"
                    && bucket.samples[0].contains("flow=document_mutation")
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow closeout pre-write guard blocked"
                    && bucket.file == "tasks/a.md"
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("pending_capture_recommendations"))
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow closeout terminal guard blocked"
                    && bucket.file == "tasks/a.md"
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("already_committed"))
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow document mutation parsed"
                    && bucket.file == "tasks/a.md"
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("valid_patch"))
            }),
            "{report:#?}"
        );
    }

    #[test]
    fn ops_summary_limit_scans_tail_only() {
        let root = Path::new("/repo");
        let log = "\
[100] commit_success file=old.md
[101] commit_success file=new.md
";

        let report = summarize_ops_log(log, root, 1, PathBuf::from("ops.log"));

        assert_eq!(report.scanned_lines, 1);
        assert_eq!(report.matched_events, 1);
        assert_eq!(report.buckets[0].file, "new.md");
    }
}
