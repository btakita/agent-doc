//! Operational log reports for `.agent-doc/logs/ops.log`.

use agent_doc_turn::op_log::OpsLogEvent;
use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

const DEFAULT_LIMIT: usize = 1000;
const MAX_SAMPLES_PER_BUCKET: usize = 3;

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpsSummaryReport {
    pub log_path: PathBuf,
    pub scanned_lines: usize,
    pub matched_events: usize,
    pub buckets: Vec<OpsSummaryBucket>,
    pub bug_clusters: Vec<OpsBugCluster>,
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

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct OpsBugCluster {
    pub rank: usize,
    pub family: String,
    pub severity: String,
    pub count: usize,
    pub latest_timestamp: Option<u64>,
    pub files: Vec<String>,
    pub sessions: Vec<String>,
    pub cycles: Vec<String>,
    pub threads: Vec<String>,
    pub examples: Vec<String>,
    pub recommendation: String,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CycleDiagnosisReport {
    pub project_root: PathBuf,
    pub query: CycleDiagnosisQuery,
    pub sources: Vec<CycleDiagnosisSource>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CycleDiagnosisQuery {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cycle_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub patch_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub session_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CycleDiagnosisSource {
    pub name: String,
    pub path: PathBuf,
    pub status: String,
    pub scanned_files: usize,
    pub scanned_lines: usize,
    pub matched: usize,
    pub matches: Vec<CycleDiagnosisMatch>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CycleDiagnosisMatch {
    pub path: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line: Option<usize>,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub json: Option<serde_json::Value>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct BucketKey {
    category: String,
    file: String,
    session: String,
}

#[derive(Debug, Clone)]
struct ClassifiedEvent {
    event_kind: Option<OpsLogEvent>,
    timestamp: Option<u64>,
    category: String,
    file: Option<String>,
    session: Option<String>,
    cycle: Option<String>,
    thread: Option<String>,
    detail: String,
    fields: BTreeMap<String, String>,
}

#[derive(Debug, Clone)]
struct ClusterSeed {
    family: &'static str,
    severity: &'static str,
    recommendation: &'static str,
}

pub fn run_summary(project_root: Option<&Path>, limit: usize, json: bool) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root = match project_root {
        Some(root) => root.to_path_buf(),
        None => agent_doc_fs::find_project_root(&cwd).unwrap_or(cwd),
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

pub fn run_diagnose(
    project_root: Option<&Path>,
    file: Option<&Path>,
    cycle_id: Option<&str>,
    patch_id: Option<&str>,
    session_id: Option<&str>,
    limit: usize,
    json: bool,
) -> Result<()> {
    let report = diagnose_cycle(project_root, file, cycle_id, patch_id, session_id, limit)?;
    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human_diagnosis(&report);
    }
    Ok(())
}

pub fn diagnose_cycle(
    project_root: Option<&Path>,
    file: Option<&Path>,
    cycle_id: Option<&str>,
    patch_id: Option<&str>,
    session_id: Option<&str>,
    limit: usize,
) -> Result<CycleDiagnosisReport> {
    let root = resolve_diagnosis_root(project_root, file)?;
    let file_query = file.map(|file| normalize_file_query(file, &root));
    let query = CycleDiagnosisQuery {
        cycle_id: cycle_id.map(ToOwned::to_owned),
        patch_id: patch_id.map(ToOwned::to_owned),
        session_id: session_id.map(ToOwned::to_owned),
        file: file_query.clone(),
    };
    let terms = diagnosis_terms(&query);
    if terms.is_empty() {
        bail!("provide at least one of --cycle-id, --patch-id, --session-id, or --file");
    }

    let agent_doc = root.join(".agent-doc");
    let logs = agent_doc.join("logs");

    let session_log_source = if let Some(session_id) = session_id {
        scan_text_source(
            "harness session log",
            logs.join(format!("{session_id}.log")),
            &terms,
            limit,
        )
    } else {
        scan_text_tree_source("harness session logs", &logs, &terms, limit, |path| {
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                return false;
            };
            name.ends_with(".log") && name != "ops.log" && !name.starts_with("debug.log")
        })
    };

    let sources = std::thread::scope(|s| {
        let ops_log = s.spawn(|| scan_text_source("ops log", logs.join("ops.log"), &terms, limit));
        let cycle_jsonl =
            s.spawn(|| scan_text_source("cycle jsonl", logs.join("cycles.jsonl"), &terms, limit));
        let editor_debug = s.spawn(|| {
            scan_text_tree_source("editor plugin/debug logs", &logs, &terms, limit, |path| {
                let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                    return false;
                };
                name.starts_with("debug.log") || name.contains("plugin") || name.contains("jb")
            })
        });
        let codex_hooks = s.spawn(|| {
            scan_json_tree_source(
                "codex hook sessions",
                &agent_doc.join("codex-hooks"),
                &terms,
            )
        });
        let hook_payloads =
            s.spawn(|| scan_json_tree_source("hook payloads", &agent_doc.join("hooks"), &terms));
        let state = s.spawn(|| scan_state_db_source(&root, &terms));

        vec![
            ops_log.join().unwrap(),
            cycle_jsonl.join().unwrap(),
            session_log_source,
            editor_debug.join().unwrap(),
            codex_hooks.join().unwrap(),
            hook_payloads.join().unwrap(),
            state.join().unwrap(),
        ]
    });

    Ok(CycleDiagnosisReport {
        project_root: root,
        query,
        sources,
    })
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
    let mut events = Vec::new();
    let mut matched_events = 0;

    for line in lines {
        let Some(event) = classify_line(line, project_root) else {
            continue;
        };
        matched_events += 1;

        let key = BucketKey {
            category: event.category.clone(),
            file: event.file.clone().unwrap_or_else(|| "<global>".to_string()),
            session: event.session.clone().unwrap_or_else(|| "-".to_string()),
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
        bucket.samples.push(event.detail.clone());
        events.push(event);
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
        bug_clusters: build_bug_clusters(&events),
    }
}

fn print_human_diagnosis(report: &CycleDiagnosisReport) {
    println!("cycle diagnosis: {}", report.project_root.display());
    let mut query = Vec::new();
    if let Some(cycle_id) = &report.query.cycle_id {
        query.push(format!("cycle_id={cycle_id}"));
    }
    if let Some(patch_id) = &report.query.patch_id {
        query.push(format!("patch_id={patch_id}"));
    }
    if let Some(session_id) = &report.query.session_id {
        query.push(format!("session_id={session_id}"));
    }
    if let Some(file) = &report.query.file {
        query.push(format!("file={file}"));
    }
    println!("query: {}", query.join(" "));

    for source in &report.sources {
        println!(
            "\n{}: {} (status={}, files={}, lines={}, matches={})",
            source.name,
            source.path.display(),
            source.status,
            source.scanned_files,
            source.scanned_lines,
            source.matched
        );
        for matched in &source.matches {
            let line = matched
                .line
                .map(|line| format!(":{line}"))
                .unwrap_or_default();
            println!("  - {}{} [{}]", matched.path.display(), line, matched.kind);
            if let Some(text) = &matched.text {
                println!("    {}", text);
            }
            if let Some(json) = &matched.json {
                println!("    {}", json);
            }
        }
    }
}

fn resolve_diagnosis_root(project_root: Option<&Path>, file: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = project_root {
        return Ok(root.to_path_buf());
    }
    if let Some(file) = file
        && let Some(root) = agent_doc_fs::find_project_root(file)
    {
        return Ok(root);
    }
    let cwd = std::env::current_dir()?;
    Ok(agent_doc_fs::find_project_root(&cwd).unwrap_or(cwd))
}

fn normalize_file_query(file: &Path, root: &Path) -> String {
    let path = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    path.strip_prefix(root)
        .unwrap_or(&path)
        .to_string_lossy()
        .to_string()
}

fn diagnosis_terms(query: &CycleDiagnosisQuery) -> Vec<String> {
    let mut terms = Vec::new();
    for value in [
        query.cycle_id.as_deref(),
        query.patch_id.as_deref(),
        query.session_id.as_deref(),
        query.file.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        if !value.trim().is_empty() && !terms.iter().any(|term| term == value) {
            terms.push(value.to_string());
        }
    }
    terms
}

fn scan_text_source(
    name: impl Into<String>,
    path: PathBuf,
    terms: &[String],
    limit: usize,
) -> CycleDiagnosisSource {
    scan_text_files_source(name, path.clone(), vec![path], terms, limit)
}

fn scan_text_tree_source(
    name: impl Into<String>,
    root: &Path,
    terms: &[String],
    limit: usize,
    include: impl Fn(&Path) -> bool,
) -> CycleDiagnosisSource {
    let files = collect_files(root, |path| include(path));
    scan_text_files_source(name, root.to_path_buf(), files, terms, limit)
}

fn scan_text_files_source(
    name: impl Into<String>,
    source_path: PathBuf,
    files: Vec<PathBuf>,
    terms: &[String],
    limit: usize,
) -> CycleDiagnosisSource {
    let mut source = empty_source(
        name,
        source_path.clone(),
        if source_path.exists() {
            "scanned"
        } else {
            "missing"
        },
    );
    for file in files {
        let Ok(contents) = std::fs::read_to_string(&file) else {
            continue;
        };
        source.scanned_files += 1;
        let lines: Vec<&str> = contents.lines().collect();
        let start = if limit == 0 || limit >= lines.len() {
            0
        } else {
            lines.len() - limit
        };
        source.scanned_lines += lines.len() - start;
        let path_matches = path_matches_terms(&file, terms);
        for (idx, line) in lines.iter().enumerate().skip(start) {
            if path_matches || matches_terms(line, terms) {
                source.matches.push(CycleDiagnosisMatch {
                    path: file.clone(),
                    line: Some(idx + 1),
                    kind: "text".to_string(),
                    text: Some(truncate_text(&agent_doc_secret_redact::redact(line), 700)),
                    json: None,
                });
            }
        }
    }
    finalize_source(source)
}

fn scan_json_tree_source(
    name: impl Into<String>,
    root: &Path,
    terms: &[String],
) -> CycleDiagnosisSource {
    let files = collect_files(root, |path| {
        path.extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext == "json" || ext == "jsonl")
    });
    scan_json_files_source(name, root, &files, terms)
}

fn scan_json_files_source(
    name: impl Into<String>,
    source_path: &Path,
    files: &[PathBuf],
    terms: &[String],
) -> CycleDiagnosisSource {
    let mut source = empty_source(
        name,
        source_path.to_path_buf(),
        if source_path.exists() {
            "scanned"
        } else {
            "missing"
        },
    );
    for file in files {
        let Ok(contents) = std::fs::read_to_string(file) else {
            continue;
        };
        source.scanned_files += 1;
        source.scanned_lines += contents.lines().count();
        if !path_matches_terms(file, terms) && !matches_terms(&contents, terms) {
            continue;
        }
        source.matches.push(CycleDiagnosisMatch {
            path: file.clone(),
            line: None,
            kind: "json".to_string(),
            text: None,
            json: Some(json_summary(&contents)),
        });
    }
    finalize_source(source)
}

fn scan_state_db_source(project_root: &Path, terms: &[String]) -> CycleDiagnosisSource {
    let path = agent_doc_sqlite::state_store::state_db_path(project_root);
    if !path.exists() {
        return empty_source("state db", path, "missing");
    }
    let ledger =
        match agent_doc_controller_io::project_controller::load_state_event_ledger(project_root) {
            Ok(ledger) => ledger,
            Err(error) => {
                let mut source = empty_source("state db", path.clone(), "error");
                source.matches.push(CycleDiagnosisMatch {
                    path,
                    line: None,
                    kind: "error".to_string(),
                    text: Some(error.to_string()),
                    json: None,
                });
                return finalize_source(source);
            }
        };
    let mut source = empty_source("state db", path.clone(), "scanned");
    source.scanned_files = 1;
    source.scanned_lines = ledger.events().len();
    for event in ledger.events() {
        let Ok(serialized) = serde_json::to_string(event) else {
            continue;
        };
        if !matches_terms(&serialized, terms) {
            continue;
        }
        source.matches.push(CycleDiagnosisMatch {
            path: path.clone(),
            line: None,
            kind: "state_event".to_string(),
            text: None,
            json: Some(json_summary(&serialized)),
        });
    }
    finalize_source(source)
}

fn empty_source(
    name: impl Into<String>,
    path: PathBuf,
    status: impl Into<String>,
) -> CycleDiagnosisSource {
    CycleDiagnosisSource {
        name: name.into(),
        path,
        status: status.into(),
        scanned_files: 0,
        scanned_lines: 0,
        matched: 0,
        matches: Vec::new(),
    }
}

fn finalize_source(mut source: CycleDiagnosisSource) -> CycleDiagnosisSource {
    source.matched = source.matches.len();
    source
}

fn collect_files(root: &Path, include: impl Fn(&Path) -> bool) -> Vec<PathBuf> {
    let mut files = Vec::new();
    collect_files_inner(root, &include, &mut files);
    files.sort();
    files
}

fn collect_files_inner(root: &Path, include: &impl Fn(&Path) -> bool, files: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        if file_type.is_dir() {
            collect_files_inner(&path, include, files);
        } else if file_type.is_file() && include(&path) {
            files.push(path);
        }
    }
}

fn path_matches_terms(path: &Path, terms: &[String]) -> bool {
    matches_terms(&path.to_string_lossy(), terms)
}

fn matches_terms(text: &str, terms: &[String]) -> bool {
    terms.iter().any(|term| text.contains(term))
}

fn json_summary(contents: &str) -> serde_json::Value {
    let redacted = agent_doc_secret_redact::redact(contents);
    match serde_json::from_str::<serde_json::Value>(&redacted) {
        Ok(value) => summarize_json_value(&value),
        Err(_) => serde_json::json!({
            "unparsed": truncate_text(&redacted, 1000),
        }),
    }
}

fn summarize_json_value(value: &serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut out = serde_json::Map::new();
            for (key, value) in map {
                if is_large_payload_key(key) {
                    out.insert(
                        key.clone(),
                        serde_json::Value::String(format!(
                            "<{} bytes omitted from diagnosis summary>",
                            value.to_string().len()
                        )),
                    );
                } else {
                    out.insert(key.clone(), summarize_json_value(value));
                }
            }
            serde_json::Value::Object(out)
        }
        serde_json::Value::Array(items) => serde_json::Value::Array(
            items
                .iter()
                .take(10)
                .map(summarize_json_value)
                .collect::<Vec<_>>(),
        ),
        serde_json::Value::String(text) => serde_json::Value::String(truncate_text(text, 700)),
        other => other.clone(),
    }
}

fn is_large_payload_key(key: &str) -> bool {
    matches!(
        key,
        "response_body"
            | "content"
            | "full_content"
            | "payload"
            | "stdout"
            | "stderr"
            | "transcript"
            | "text"
    )
}

fn truncate_text(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...<truncated>", &text[..end])
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

    if !report.bug_clusters.is_empty() {
        println!("\nbug clusters");
        for cluster in &report.bug_clusters {
            let latest = cluster
                .latest_timestamp
                .map(|ts| ts.to_string())
                .unwrap_or_else(|| "-".to_string());
            println!(
                "  #{} {} severity={} count={} latest={}",
                cluster.rank, cluster.family, cluster.severity, cluster.count, latest
            );
            if !cluster.files.is_empty() {
                println!("    files: {}", cluster.files.join(", "));
            }
            if !cluster.sessions.is_empty() {
                println!("    sessions: {}", cluster.sessions.join(", "));
            }
            if !cluster.cycles.is_empty() {
                println!("    cycles: {}", cluster.cycles.join(", "));
            }
            if !cluster.threads.is_empty() {
                println!("    threads: {}", cluster.threads.join(", "));
            }
            println!("    next: {}", cluster.recommendation);
            for sample in &cluster.examples {
                println!("    - {}", sample);
            }
        }
    }
}

fn classify_line(line: &str, project_root: &Path) -> Option<ClassifiedEvent> {
    let (timestamp, message) = parse_log_line(line);
    let event_name = message.split_whitespace().next()?;
    let event_kind = OpsLogEvent::from_token(event_name);
    let fields = parse_fields(message);

    let category = match event_kind {
        Some(OpsLogEvent::FlowEvent) => classify_flow_event(&fields),
        Some(OpsLogEvent::InterruptedCycleDetected) => "closeout interrupted cycle".to_string(),
        Some(OpsLogEvent::LateFallbackPatchRejected) => {
            "closeout late fallback rejected".to_string()
        }
        Some(OpsLogEvent::StaleSnapshotResetDriftBlocked) => {
            "closeout stale snapshot reset blocked".to_string()
        }
        Some(OpsLogEvent::CommitBlockedMissingCapturedResponse) => {
            "closeout missing captured response".to_string()
        }
        Some(OpsLogEvent::SessionCheckCommitBoundaryRecovered) => {
            "closeout commit boundary recovered".to_string()
        }
        Some(OpsLogEvent::IpcWriteConsumed) => "write ipc consumed".to_string(),
        Some(OpsLogEvent::IpcProofInsufficient) => "write ipc proof insufficient".to_string(),
        Some(OpsLogEvent::CommitSuccess) => "commit success".to_string(),
        Some(OpsLogEvent::CommitNoop) if field_eq(&fields, "drift_kind", "user_follow_up") => {
            "expected user follow-up noop".to_string()
        }
        Some(OpsLogEvent::CommitNoop) if field_eq(&fields, "drift_kind", "working_tree_edits") => {
            "anomalous drift noop".to_string()
        }
        Some(OpsLogEvent::CommitNoop) if field_eq(&fields, "drift_kind", "none") => {
            "expected already-current noop".to_string()
        }
        Some(OpsLogEvent::CommitNoop) => "commit noop".to_string(),
        Some(OpsLogEvent::RouteDispatchStartProven) => "route dispatch proven".to_string(),
        Some(OpsLogEvent::RouteSubmitIssue)
            if field_eq(&fields, "issue", "prompt_not_submitted") =>
        {
            "route submit prompt not submitted".to_string()
        }
        Some(OpsLogEvent::RouteSubmitIssue)
            if field_eq(&fields, "issue", "accepted_without_dispatch_start_proof") =>
        {
            "route submit dispatch proof missing".to_string()
        }
        Some(OpsLogEvent::RouteSubmitIssue) => "route submit issue".to_string(),
        Some(OpsLogEvent::PostCommitUserFollowUp) => "expected user follow-up".to_string(),
        Some(OpsLogEvent::PostCommitLocalDrift) if field_eq(&fields, "kind", "user_follow_up") => {
            "expected user follow-up".to_string()
        }
        Some(OpsLogEvent::PostCommitLocalDrift) => "anomalous post-commit drift".to_string(),
        Some(OpsLogEvent::SessionClearActivePaneAllowed) => "active clear allowed".to_string(),
        Some(OpsLogEvent::SessionClearProtectedInputGuardRefused) => {
            "expected protected-input clear refusal".to_string()
        }
        Some(OpsLogEvent::SessionClearLiveBusyGuardBypassed) => "busy clear bypassed".to_string(),
        Some(OpsLogEvent::SessionClearLiveBusyGuardRefused) => "busy clear refused".to_string(),
        Some(OpsLogEvent::SessionClearLiveBusyGuardBlocked) => "busy clear blocked".to_string(),
        Some(OpsLogEvent::RouteAuthoritativeActorStartingNotReady) => {
            "starting actor not ready".to_string()
        }
        Some(OpsLogEvent::RouteStartingActorTimeoutCoalesced) => {
            "starting actor timeout coalesced".to_string()
        }
        Some(
            OpsLogEvent::RouteCycleStartMissing
            | OpsLogEvent::RouteCycleStartMissingAfterFreshRestartOptimistic
            | OpsLogEvent::RouteCycleStartMissingOptimistic,
        ) => "route cycle start missing".to_string(),
        Some(OpsLogEvent::RouteDispatchStartUnprovenButAccepted) => {
            "accepted-only route proof".to_string()
        }
        Some(OpsLogEvent::RouteDispatchOnlySent)
            if field_eq(&fields, "proof_scope", "accepted_only") =>
        {
            "accepted-only route proof".to_string()
        }
        Some(OpsLogEvent::RouteDispatchOnlySubmitUnproven) => {
            "dispatch-only not proven".to_string()
        }
        Some(OpsLogEvent::RunPreflightTimeout) => "run preflight timeout".to_string(),
        Some(OpsLogEvent::SyncLatency) if field_eq(&fields, "status", "over_budget") => {
            "sync over budget".to_string()
        }
        Some(OpsLogEvent::SqliteLogCounts | OpsLogEvent::SqliteLogCount) => {
            "sqlite log counts".to_string()
        }
        Some(OpsLogEvent::SessionReviewGuard) => "session-review guardrail".to_string(),
        Some(
            OpsLogEvent::CodexThreadStarted
            | OpsLogEvent::ClaudeJsonlHookMarker
            | OpsLogEvent::AgentDocCycleMarker,
        ) => "cross-harness correlation marker".to_string(),
        Some(OpsLogEvent::ConvergenceGateBlocked) => "convergence gate blocked".to_string(),
        Some(OpsLogEvent::RecursiveDirectInvocationBlocked) => {
            "recursive direct invocation blocked".to_string()
        }
        Some(OpsLogEvent::EditorOpRecorded) => "editor op recorded".to_string(),
        Some(OpsLogEvent::EditorOpsForBase) => "editor ops consumed".to_string(),
        Some(OpsLogEvent::EditorOpRecordFailed) => "editor op record failed".to_string(),
        Some(OpsLogEvent::SupervisorBinaryStale) => "stale supervisor".to_string(),
        Some(OpsLogEvent::RetryOnCurrentGeneration | OpsLogEvent::SupervisorRestartRedirect) => {
            "supervisor generation recovery".to_string()
        }
        Some(OpsLogEvent::StaleGeneration) => "stale generation block".to_string(),
        Some(
            OpsLogEvent::EditorConvergenceAckMismatch
            | OpsLogEvent::EditorConvergenceNoAck
            | OpsLogEvent::LivePromptDriftAfterPreflight,
        ) => "editor convergence failure".to_string(),
        None if is_codex_manifest_warning(message) => "codex manifest warning storm".to_string(),
        None if session_review_family_for_message(message).is_some() => {
            "session-review guardrail".to_string()
        }
        None if let Some(site) = crdt_convergence_wait_site(event_name) => {
            format!("crdt convergence wait {site}")
        }
        _ => return None,
    };

    let detail = if category == "codex manifest warning storm" {
        message.to_string()
    } else {
        render_detail(event_name, &fields)
    };

    Some(ClassifiedEvent {
        event_kind,
        timestamp,
        category,
        file: fields
            .get("file")
            .map(|file| normalize_file_for_report(file, project_root)),
        session: extract_session(&fields),
        cycle: extract_cycle(&fields),
        thread: extract_thread(&fields),
        detail,
        fields,
    })
}

fn classify_flow_event(fields: &BTreeMap<String, String>) -> String {
    match (
        fields.get("flow").map(String::as_str),
        fields.get("stage").map(String::as_str),
        fields.get("outcome").map(String::as_str),
    ) {
        (Some("routed_reopen"), Some("prompt_ready_barrier"), Some("failed_closed")) => {
            "flow routed reopen prompt-ready failures".to_string()
        }
        (
            Some("routed_reopen"),
            Some("dispatch_submit" | "dispatch_proof"),
            Some("failed_closed"),
        ) => "flow routed reopen dispatch failures".to_string(),
        (Some("document_mutation"), Some("patchback_parse"), Some("failed_closed")) => {
            "flow document mutation parse failures".to_string()
        }
        (Some("document_mutation"), Some("patchback_parse"), Some("completed")) => {
            "flow document mutation parsed".to_string()
        }
        (Some("document_mutation"), Some("pre_write_guard"), Some("blocked" | "failed_closed")) => {
            "flow document mutation pre-write guard blocked".to_string()
        }
        (Some("closeout"), Some("pre_write_guard"), Some("blocked" | "failed_closed")) => {
            "flow closeout pre-write guard blocked".to_string()
        }
        (Some("closeout"), Some("pre_commit_guard"), Some("blocked" | "failed_closed")) => {
            "flow closeout pre-commit guard blocked".to_string()
        }
        (Some("closeout"), Some("terminal_guard"), Some("blocked" | "failed_closed")) => {
            "flow closeout terminal guard blocked".to_string()
        }
        (Some("closeout"), Some("session_check"), Some("blocked" | "failed_closed")) => {
            "flow closeout session-check failures".to_string()
        }
        (Some("closeout"), Some("commit"), Some("completed")) => {
            "flow closeout commit completed".to_string()
        }
        (Some("closeout"), Some("commit"), Some("blocked" | "failed_closed")) => {
            "flow closeout commit failures".to_string()
        }
        (
            Some("orchestration_batch"),
            Some("child_closeout"),
            Some("blocked" | "failed_closed"),
        ) => "flow orchestration child closeout failures".to_string(),
        (Some("orchestration_batch"), Some("child_closeout"), Some("completed")) => {
            "flow orchestration child closeout completed".to_string()
        }
        (Some("orchestration_batch"), Some("queue_freeze"), Some("blocked" | "failed_closed")) => {
            "flow orchestration batch freeze blocked".to_string()
        }
        (Some("orchestration_batch"), Some("queue_freeze"), Some("completed")) => {
            "flow orchestration batch freeze completed".to_string()
        }
        (Some("operator_clear"), Some("operator_guard"), Some("blocked" | "failed_closed")) => {
            "flow operator clear guard failures".to_string()
        }
        (Some("operator_clear"), Some("operator_guard"), Some("completed")) => {
            "flow operator clear guard completed".to_string()
        }
        (Some("session_cycle"), _, Some("blocked" | "failed_closed")) => {
            "flow session cycle failures".to_string()
        }
        (Some("routed_reopen"), _, Some("blocked" | "failed_closed")) => {
            "flow routed reopen failures".to_string()
        }
        (Some("closeout"), _, Some("blocked" | "failed_closed")) => {
            "flow closeout failures".to_string()
        }
        (Some(flow), Some(stage), Some(outcome)) => format!(
            "flow {} {} {}",
            flow.replace('_', " "),
            stage.replace('_', " "),
            outcome.replace('_', " ")
        ),
        _ => "flow events".to_string(),
    }
}

fn build_bug_clusters(events: &[ClassifiedEvent]) -> Vec<OpsBugCluster> {
    #[derive(Debug, Default)]
    struct Accumulator {
        count: usize,
        latest_timestamp: Option<u64>,
        files: BTreeSet<String>,
        sessions: BTreeSet<String>,
        cycles: BTreeSet<String>,
        threads: BTreeSet<String>,
        examples: Vec<String>,
        severity: &'static str,
        recommendation: &'static str,
    }

    let mut clusters: BTreeMap<&'static str, Accumulator> = BTreeMap::new();
    for event in events {
        let Some(seed) = cluster_seed(event) else {
            continue;
        };
        let entry = clusters.entry(seed.family).or_insert_with(|| Accumulator {
            severity: seed.severity,
            recommendation: seed.recommendation,
            ..Accumulator::default()
        });
        entry.count += 1;
        if event.timestamp > entry.latest_timestamp {
            entry.latest_timestamp = event.timestamp;
        }
        if let Some(file) = &event.file {
            entry.files.insert(file.clone());
        }
        if let Some(session) = &event.session {
            entry.sessions.insert(session.clone());
        }
        if let Some(cycle) = &event.cycle {
            entry.cycles.insert(cycle.clone());
        }
        if let Some(thread) = &event.thread {
            entry.threads.insert(thread.clone());
        }
        if entry.examples.len() == MAX_SAMPLES_PER_BUCKET {
            entry.examples.remove(0);
        }
        entry.examples.push(event.detail.clone());
    }

    let mut ranked = clusters
        .into_iter()
        .map(|(family, cluster)| {
            let severity_rank = severity_rank(cluster.severity);
            (
                severity_rank,
                cluster.latest_timestamp,
                OpsBugCluster {
                    rank: 0,
                    family: family.to_string(),
                    severity: cluster.severity.to_string(),
                    count: cluster.count,
                    latest_timestamp: cluster.latest_timestamp,
                    files: cluster.files.into_iter().collect(),
                    sessions: cluster.sessions.into_iter().collect(),
                    cycles: cluster.cycles.into_iter().collect(),
                    threads: cluster.threads.into_iter().collect(),
                    examples: cluster.examples,
                    recommendation: cluster.recommendation.to_string(),
                },
            )
        })
        .collect::<Vec<_>>();
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| b.2.count.cmp(&a.2.count))
            .then_with(|| b.1.cmp(&a.1))
            .then_with(|| a.2.family.cmp(&b.2.family))
    });
    ranked
        .into_iter()
        .enumerate()
        .map(|(idx, (_, _, mut cluster))| {
            cluster.rank = idx + 1;
            cluster
        })
        .collect()
}

fn cluster_seed(event: &ClassifiedEvent) -> Option<ClusterSeed> {
    let flow = event.fields.get("flow").map(String::as_str);
    let reason = event.fields.get("reason").map(String::as_str);
    let category = event.category.as_str();

    if matches!(
        event.event_kind,
        Some(
            OpsLogEvent::InterruptedCycleDetected
                | OpsLogEvent::LateFallbackPatchRejected
                | OpsLogEvent::StaleSnapshotResetDriftBlocked
                | OpsLogEvent::CommitBlockedMissingCapturedResponse
                | OpsLogEvent::SessionCheckCommitBoundaryRecovered
        )
    ) || (flow == Some("closeout")
        && matches!(
            reason,
            Some(
                "already_committed"
                    | "session_check_interrupted"
                    | "commit_boundary_recovered"
                    | "snapshot_differs_from_head"
            )
        ))
    {
        return Some(ClusterSeed {
            family: "closeout captured-response drift",
            severity: "high",
            recommendation: "replay through `agent-doc write --commit` or `finalize` and keep capture/snapshot/HEAD proof on the strict closeout path",
        });
    }

    if matches!(
        event.event_kind,
        Some(
            OpsLogEvent::RouteStartingActorTimeoutCoalesced
                | OpsLogEvent::RouteCycleStartMissing
                | OpsLogEvent::RouteCycleStartMissingAfterFreshRestartOptimistic
                | OpsLogEvent::RouteCycleStartMissingOptimistic
                | OpsLogEvent::RunPreflightTimeout
                | OpsLogEvent::RouteSubmitIssue
                | OpsLogEvent::RouteDispatchOnlySubmitUnproven
        )
    ) || flow == Some("routed_reopen")
        || category == "accepted-only route proof"
        || category == "starting actor not ready"
    {
        return Some(ClusterSeed {
            family: "route/start replay gap",
            severity: "high",
            recommendation: "replay the route with dispatch-start proof, starting-actor readiness evidence, and timeout diagnostics before accepting pane input as delivery",
        });
    }

    if is_codex_manifest_warning(&event.detail) || category == "codex manifest warning storm" {
        return Some(ClusterSeed {
            family: "codex warning storm",
            severity: "medium",
            recommendation: "deduplicate external Codex plugin/skill-loader manifest noise and keep local manifest warnings visible",
        });
    }

    if category == "sqlite log counts" {
        return Some(ClusterSeed {
            family: "sqlite correlation counts",
            severity: "medium",
            recommendation: "compare controller SQLite row counts with session-log and ops-log markers for missing projection or correlation drift",
        });
    }

    if category == "cross-harness correlation marker" {
        return Some(ClusterSeed {
            family: "cross-harness correlation",
            severity: "medium",
            recommendation: "join Claude hook markers, Codex thread ids, agent-doc cycle ids, and ops timestamps before ranking bug clusters",
        });
    }

    if category == "session-review guardrail"
        || session_review_family_for_message(&event.detail).is_some()
    {
        return Some(ClusterSeed {
            family: "session-review guardrail",
            severity: "medium",
            recommendation: "apply the guard action before scheduling: compact for budget/cache churn, restart or repair for loops, and fix fixtures for repeated noop closeouts",
        });
    }

    if category == "anomalous drift noop" || category == "anomalous post-commit drift" {
        return Some(ClusterSeed {
            family: "working-tree drift after closeout",
            severity: "medium",
            recommendation: "separate benign user follow-up prompts from dirty working-tree drift before marking closeout complete",
        });
    }

    None
}

fn severity_rank(severity: &str) -> usize {
    match severity {
        "high" => 3,
        "medium" => 2,
        "low" => 1,
        _ => 0,
    }
}

fn extract_session(fields: &BTreeMap<String, String>) -> Option<String> {
    fields
        .get("session")
        .or_else(|| fields.get("session_id"))
        .or_else(|| fields.get("agent_doc_session"))
        .cloned()
}

fn extract_cycle(fields: &BTreeMap<String, String>) -> Option<String> {
    fields
        .get("cycle_id")
        .or_else(|| fields.get("cycle"))
        .or_else(|| fields.get("capture_id"))
        .cloned()
}

fn extract_thread(fields: &BTreeMap<String, String>) -> Option<String> {
    fields
        .get("thread_id")
        .or_else(|| fields.get("codex_thread_id"))
        .or_else(|| fields.get("claude_session_id"))
        .cloned()
}

fn is_codex_manifest_warning(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    (lower.contains("codex_core_plugins::manifest:")
        || lower.contains("codex_core::plugins::manifest:")
        || lower.contains("codex_core_skills::loader:"))
        && (lower.contains("interface.defaultprompt")
            || lower.contains("interface.icon_small")
            || lower.contains("interface.icon_large")
            || lower.contains("invalid icon")
            || lower.contains("defaultprompt"))
}

fn session_review_family_for_message(message: &str) -> Option<&'static str> {
    let lower = message.to_ascii_lowercase();
    if lower.contains("prompt_budget")
        || lower.contains("prompt budget")
        || lower.contains("context budget")
        || lower.contains("prompt too large")
        || lower.contains("maximum context")
        || lower.contains("token budget exceeded")
    {
        return Some("prompt_budget");
    }
    if lower.contains("cache_resend")
        || lower.contains("cache resend")
        || lower.contains("cache-resend")
        || lower.contains("resend full context")
    {
        return Some("cache_resend");
    }
    if lower.contains("restart_loop")
        || lower.contains("restart loop")
        || lower.contains("startup_miss")
        || lower.contains("fresh_restart_retry")
    {
        return Some("restart_loop");
    }
    if lower.contains("noop_closeout")
        || lower.contains("noop closeout")
        || lower.contains("commit_noop")
        || lower.contains("already_current")
    {
        return Some("noop_closeout");
    }
    None
}

/// Write site of a `<site>_crdt_convergence_wait` event, if that is what this is.
///
/// `#caswriteracechurn`: these events carry the settle wait that dominates
/// closeout latency, and `ops summary` did not surface them at all — the only
/// way to see how often a closeout took the compare-and-swap branch, and which
/// writer it contended with, was to grep the raw log by hand. Bucketing by the
/// event's own site prefix answers "what is the contending writer" directly,
/// because a site names one write path.
///
/// The event name is dynamic (`pending_write_...`, `serialized_atomic_write_...`,
/// `start_resume_id_capture_...`), so it cannot be a fixed token in
/// `OpsLogEvent`.
fn crdt_convergence_wait_site(event_name: &str) -> Option<&str> {
    let site = event_name.strip_suffix("_crdt_convergence_wait")?;
    (!site.is_empty()).then_some(site)
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
        "issue",
        "result",
        "pane",
        "source",
        "current_command",
        "harness",
        "proof",
        "proof_scope",
        "phase",
        "mode",
        "trigger_visible",
        "elapsed_ms",
        "budget_ms",
        "capture_len",
        "capture_hash",
        "patches",
        "generation",
        "actor_state",
        "cycle_id",
        "cycle",
        "capture_id",
        "thread_id",
        "session",
        "session_id",
        "sqlite_documents",
        "sqlite_actor_transitions",
        "sqlite_cycles",
        "count",
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
[104] route_dispatch_only_submit_unproven file=tasks/b.md pane=%2 harness=opencode delivery=direct_pane_submit submit_mode=tmux_text_enter proof=accepted proof_scope=accepted_only timeout_secs=10
[105] route_submit_issue file=tasks/b.md pane=%2 harness=codex phase=direct_pane_acceptance issue=prompt_not_submitted result=trigger_still_visible elapsed_ms=5010 trigger_visible=true capture_len=2048 capture_hash=abc123def456
[106] post_commit_local_drift file=/repo/tasks/a.md kind=user_follow_up basis=head
[107] post_commit_user_follow_up file=/repo/tasks/a.md basis=head
[108] post_commit_local_drift file=/repo/tasks/a.md kind=working_tree_edits basis=head
[109] commit_noop file=/repo/tasks/a.md reason=already_current drift_kind=user_follow_up basis=head
[110] commit_noop file=/repo/tasks/a.md reason=already_current drift_kind=working_tree_edits basis=head
[111] commit_noop file=/repo/tasks/a.md reason=already_current drift_kind=none basis=head
[112] session_clear_active_pane_allowed file=/repo/tasks/a.md pane=%1 source=authoritative_actor current_command=agent-doc
[113] session_clear_protected_input_guard_refused file=/repo/tasks/a.md pane=%1 source=authoritative_actor reason=drafted_prompt_input current_command=agent-doc
[114] session_clear_live_busy_guard_bypassed file=/repo/tasks/a.md pane=%1 source=authoritative_actor current_command=agent-doc
[115] session_clear_live_busy_guard_refused file=/repo/tasks/a.md pane=%1 source=authoritative_actor current_command=agent-doc
[116] route_authoritative_actor_starting_not_ready file=tasks/c.md pane=%3 harness=codex generation=9 actor_state=starting
[117] sync_latency phase=prune_stash_panes elapsed_ms=309 budget_ms=250 status=over_budget mode=full
[118] flow_event file=/repo/tasks/a.md flow=closeout stage=commit outcome=completed reason=already_current
[119] flow_event file=/repo/tasks/c.md flow=routed_reopen stage=prompt_ready_barrier outcome=failed_closed reason=starting_actor_not_ready
[120] flow_event file=/repo/tasks/a.md flow=document_mutation stage=patchback_parse outcome=failed_closed reason=malformed_patchback
[121] flow_event file=/repo/tasks/a.md flow=closeout stage=pre_write_guard outcome=blocked reason=pending_capture_recommendations
[122] flow_event file=/repo/tasks/a.md flow=closeout stage=terminal_guard outcome=blocked reason=already_committed
[123] flow_event file=/repo/tasks/a.md flow=document_mutation stage=patchback_parse outcome=completed reason=valid_patch
[124] flow_event file=/repo/tasks/a.md flow=document_mutation stage=pre_write_guard outcome=blocked reason=visible_write_typing_defer_active_typing:socket_ipc
[125] flow_event file=/repo/tasks/a.md flow=routed_reopen stage=dispatch_proof outcome=failed_closed reason=accepted_only_dispatch_start_proof
[126] flow_event file=/repo/tasks/a.md flow=orchestration_batch stage=child_closeout outcome=completed reason=child_patchback:wrapped_plain_response
[127] flow_event file=/repo/tasks/a.md flow=session_cycle stage=plan outcome=completed reason=normal
[128] controller_supervisor_heartbeat session=s1 pane=%1 generation=3 state=ready
";

        let report =
            summarize_ops_log(log, root, 0, PathBuf::from("/repo/.agent-doc/logs/ops.log"));

        assert_eq!(report.scanned_lines, 29);
        assert_eq!(report.matched_events, 28);
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
                bucket.category == "route submit prompt not submitted"
                    && bucket.file == "tasks/b.md"
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("issue=prompt_not_submitted"))
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
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow document mutation pre-write guard blocked"
                    && bucket.file == "tasks/a.md"
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("visible_write_typing_defer_active_typing"))
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow routed reopen dispatch failures"
                    && bucket.file == "tasks/a.md"
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("accepted_only_dispatch_start_proof"))
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow orchestration child closeout completed"
                    && bucket.file == "tasks/a.md"
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("child_patchback:wrapped_plain_response"))
            }),
            "{report:#?}"
        );
        assert!(
            report.buckets.iter().any(|bucket| {
                bucket.category == "flow session cycle plan completed"
                    && bucket.file == "tasks/a.md"
                    && bucket
                        .samples
                        .iter()
                        .any(|sample| sample.contains("reason=normal"))
            }),
            "{report:#?}"
        );
    }

    #[test]
    fn ops_summary_emits_ranked_bug_clusters_with_correlation_keys() {
        let root = Path::new("/repo");
        let log = "\
[200] interrupted_cycle_detected file=/repo/tasks/a.md session=s1 cycle_id=cycle-a phase=response_captured
[201] commit_blocked_missing_captured_response file=/repo/tasks/a.md session=s1 capture_id=cycle-a response_sha256=abc basis=head_current
[202] stale_snapshot_reset_drift_blocked file=/repo/tasks/a.md session=s1 cycle_id=cycle-a phase=stream_write
[203] route_starting_actor_timeout_coalesced file=/repo/tasks/b.md session=s2 pane=%2 generation=4 actor_state=starting
[204] route_cycle_start_missing file=/repo/tasks/b.md session=s2 pane=%2 harness=codex marker=run timeout_secs=10
[205] run_preflight_timeout file=/repo/tasks/b.md session=s2 event=direct_invocation_timeout diagnostic=preflight_started
[207] WARN codex_core_plugins::manifest: ignoring interface.defaultPrompt: prompt must be at most 128 characters path=/home/brian/.codex/.tmp/plugins/plugins/build-ios-apps/.codex-plugin/plugin.json
[208] sqlite_log_counts file=/repo/tasks/a.md session=s1 cycle_id=cycle-a sqlite_documents=3 sqlite_actor_transitions=9 sqlite_cycles=2
[209] session_review_guard file=/repo/tasks/c.md session=s3 family=prompt_budget count=2
[210] codex_thread_started file=/repo/tasks/a.md session=s1 cycle_id=cycle-a thread_id=thread-7
[211] route_submit_issue file=/repo/tasks/b.md session=s2 pane=%2 harness=codex phase=dispatch_start_proof issue=accepted_without_dispatch_start_proof result=accepted_without_dispatch_start_proof elapsed_ms=10000
";

        let report =
            summarize_ops_log(log, root, 0, PathBuf::from("/repo/.agent-doc/logs/ops.log"));

        assert_eq!(report.matched_events, 11);
        let closeout = report
            .bug_clusters
            .iter()
            .find(|cluster| cluster.family == "closeout captured-response drift")
            .expect("closeout cluster");
        assert_eq!(closeout.severity, "high");
        assert_eq!(closeout.count, 3);
        assert_eq!(closeout.files, vec!["tasks/a.md"]);
        assert_eq!(closeout.sessions, vec!["s1"]);
        assert!(closeout.cycles.contains(&"cycle-a".to_string()));

        let route = report
            .bug_clusters
            .iter()
            .find(|cluster| cluster.family == "route/start replay gap")
            .expect("route cluster");
        assert_eq!(route.count, 4);
        assert_eq!(route.sessions, vec!["s2"]);

        let codex = report
            .bug_clusters
            .iter()
            .find(|cluster| cluster.family == "codex warning storm")
            .expect("codex cluster");
        assert_eq!(codex.count, 1);
        assert!(
            codex
                .examples
                .iter()
                .any(|sample| sample.contains("codex_core_plugins::manifest"))
        );

        let sqlite = report
            .bug_clusters
            .iter()
            .find(|cluster| cluster.family == "sqlite correlation counts")
            .expect("sqlite cluster");
        assert!(
            sqlite
                .examples
                .iter()
                .any(|sample| sample.contains("sqlite_actor_transitions=9"))
        );

        let guard = report
            .bug_clusters
            .iter()
            .find(|cluster| cluster.family == "session-review guardrail")
            .expect("session-review cluster");
        assert_eq!(guard.files, vec!["tasks/c.md"]);

        let thread_link = report
            .bug_clusters
            .iter()
            .flat_map(|cluster| cluster.threads.iter())
            .any(|thread| thread == "thread-7");
        assert!(
            thread_link,
            "expected a bug cluster to retain Codex thread correlation keys: {:#?}",
            report.bug_clusters
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

/// (`#ptsubmitmetric`) Rollup of pass-through submit repairs, so the
/// `#passthroughsplitprofile` / `#claudesplitsubmit` decision is readable from
/// one command instead of hand-parsing `route_pass_through_submit_draft` lines.
///
/// The measured question is: how often does the initial single-call text+Enter
/// send fail to submit, requiring a bare submit key? That is
/// `resubmit_required=true` on a terminal repair line. Counting
/// `outcome=enter_resubmit` instead — the criterion this replaces — is
/// structurally always zero, because `EnterResubmit` is non-terminal and never
/// reaches ops.log.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SubmitProfileReport {
    pub log_path: PathBuf,
    pub scanned_lines: usize,
    /// Terminal repair lines that predate the `resubmit_required` field. They
    /// are reported separately rather than silently folded in: `enters_sent` is
    /// recoverable from them, but a reader must know the sample is mixed.
    pub legacy_lines_without_field: usize,
    pub since: Option<String>,
    pub harnesses: Vec<SubmitProfileHarness>,
}

#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SubmitProfileHarness {
    pub harness: String,
    pub repairs: usize,
    pub resubmit_required: usize,
    /// Percentage of repairs that needed at least one bare submit key.
    pub resubmit_rate_pct: u32,
    pub outcomes: BTreeMap<String, usize>,
    pub max_enters_sent: usize,
}

pub fn summarize_submit_profile(
    contents: &str,
    log_path: PathBuf,
    since: Option<&str>,
) -> SubmitProfileReport {
    let mut per_harness: BTreeMap<String, (usize, usize, BTreeMap<String, usize>, usize)> =
        BTreeMap::new();
    let mut scanned = 0usize;
    let mut legacy = 0usize;

    for line in contents.lines() {
        if !line.contains("route_pass_through_submit_draft ") {
            continue;
        }
        if let Some(since) = since
            && submit_profile_line_timestamp(line).is_some_and(|ts| ts.as_str() < since)
        {
            continue;
        }
        scanned += 1;
        let harness = submit_profile_field(line, "harness").unwrap_or_else(|| "unknown".to_string());
        let outcome = submit_profile_field(line, "outcome").unwrap_or_else(|| "unknown".to_string());
        let enters = submit_profile_field(line, "enters_sent")
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(0);
        let required = match submit_profile_field(line, "resubmit_required") {
            Some(value) => value == "true",
            None => {
                legacy += 1;
                enters > 0
            }
        };

        let entry = per_harness.entry(harness).or_insert_with(|| {
            (
                0,
                0,
                BTreeMap::new(),
                0,
            )
        });
        entry.0 += 1;
        if required {
            entry.1 += 1;
        }
        *entry.2.entry(outcome).or_insert(0) += 1;
        entry.3 = entry.3.max(enters);
    }

    let harnesses = per_harness
        .into_iter()
        .map(
            |(harness, (repairs, resubmit_required, outcomes, max_enters_sent))| {
                let resubmit_rate_pct = (resubmit_required * 100)
                    .checked_div(repairs)
                    .and_then(|pct| u32::try_from(pct).ok())
                    .unwrap_or(0);
                SubmitProfileHarness {
                    harness,
                    repairs,
                    resubmit_required,
                    resubmit_rate_pct,
                    outcomes,
                    max_enters_sent,
                }
            },
        )
        .collect();

    SubmitProfileReport {
        log_path,
        scanned_lines: scanned,
        legacy_lines_without_field: legacy,
        since: since.map(str::to_string),
        harnesses,
    }
}

fn submit_profile_field(line: &str, key: &str) -> Option<String> {
    let needle = format!(" {key}=");
    let start = line.find(&needle)? + needle.len();
    let rest = &line[start..];
    let end = rest.find(' ').unwrap_or(rest.len());
    Some(rest[..end].to_string())
}

/// ops.log lines are prefixed `[RFC3339] `, which sorts lexicographically, so a
/// `--since` prefix like `2026-08-08` filters correctly by string compare.
fn submit_profile_line_timestamp(line: &str) -> Option<String> {
    let rest = line.strip_prefix('[')?;
    let end = rest.find(']')?;
    Some(rest[..end].to_string())
}

pub fn run_submit_profile(
    project_root: Option<&Path>,
    since: Option<&str>,
    json: bool,
) -> Result<()> {
    let cwd = std::env::current_dir()?;
    let root = match project_root {
        Some(root) => root.to_path_buf(),
        None => agent_doc_fs::find_project_root(&cwd).unwrap_or(cwd),
    };
    let log_path = root.join(".agent-doc/logs/ops.log");
    let contents = std::fs::read_to_string(&log_path)
        .with_context(|| format!("failed to read {}", log_path.display()))?;
    let report = summarize_submit_profile(&contents, log_path, since);

    if json {
        println!("{}", serde_json::to_string_pretty(&report)?);
        return Ok(());
    }

    println!("pass-through submit repairs — {}", report.log_path.display());
    if let Some(since) = report.since.as_deref() {
        println!("  since: {since}");
    }
    if report.scanned_lines == 0 {
        println!("  no route_pass_through_submit_draft lines matched");
        return Ok(());
    }
    for harness in &report.harnesses {
        println!(
            "  {}: {} repair(s), {} needed a bare submit key ({}%), max enters_sent={}",
            harness.harness,
            harness.repairs,
            harness.resubmit_required,
            harness.resubmit_rate_pct,
            harness.max_enters_sent
        );
        for (outcome, count) in &harness.outcomes {
            println!("      outcome={outcome}: {count}");
        }
    }
    if report.legacy_lines_without_field > 0 {
        println!(
            "  note: {} line(s) predate `resubmit_required` and were counted from `enters_sent`",
            report.legacy_lines_without_field
        );
    }
    Ok(())
}

#[cfg(test)]
mod submit_profile_tests {
    use super::*;

    /// `#ptsubmitmetric`: the rollup must count the signal that actually exists
    /// in a production log — `resubmit_required` on a TERMINAL line — not the
    /// structurally-impossible `outcome=enter_resubmit`.
    #[test]
    fn submit_profile_counts_terminal_resubmits_per_harness() {
        let log = concat!(
            "[2026-08-07T10:00:00Z] route_pass_through_submit_draft file=a.md pane=%1 harness=claude outcome=cleared enters_sent=1 resubmit_required=true elapsed_ms=306 capture_failed=false\n",
            "[2026-08-08T10:00:00Z] route_pass_through_submit_draft file=a.md pane=%1 harness=claude outcome=cleared enters_sent=0 resubmit_required=false elapsed_ms=153 capture_failed=false\n",
            "[2026-08-08T10:01:00Z] route_pass_through_submit_draft file=a.md pane=%1 harness=claude outcome=exhausted_still_stranded enters_sent=3 resubmit_required=true elapsed_ms=1200 capture_failed=false\n",
            "[2026-08-08T10:02:00Z] route_pass_through_submit_draft file=b.md pane=%2 harness=codex outcome=cleared enters_sent=0 resubmit_required=false elapsed_ms=90 capture_failed=false\n",
            "[2026-08-08T10:03:00Z] some_other_event file=a.md\n",
        );

        let report = summarize_submit_profile(log, PathBuf::from("ops.log"), None);
        assert_eq!(report.scanned_lines, 4, "the non-repair line must not count");
        assert_eq!(report.legacy_lines_without_field, 0);

        let claude = report
            .harnesses
            .iter()
            .find(|h| h.harness == "claude")
            .expect("claude rollup");
        assert_eq!(claude.repairs, 3);
        assert_eq!(claude.resubmit_required, 2);
        assert_eq!(claude.resubmit_rate_pct, 66);
        assert_eq!(claude.max_enters_sent, 3);
        assert_eq!(claude.outcomes.get("cleared"), Some(&2));
        assert_eq!(claude.outcomes.get("exhausted_still_stranded"), Some(&1));

        let codex = report
            .harnesses
            .iter()
            .find(|h| h.harness == "codex")
            .expect("codex rollup");
        assert_eq!(codex.repairs, 1);
        assert_eq!(codex.resubmit_required, 0);

        // `--since` is a lexicographic RFC3339 prefix compare, which is why the
        // ops.log timestamp format can be filtered without date parsing.
        let recent = summarize_submit_profile(log, PathBuf::from("ops.log"), Some("2026-08-08"));
        let claude = recent
            .harnesses
            .iter()
            .find(|h| h.harness == "claude")
            .expect("claude rollup");
        assert_eq!(claude.repairs, 2, "the 08-07 line must be excluded");
        assert_eq!(claude.resubmit_required, 1);
    }

    /// Lines written before the field exists must still be countable, and the
    /// mixed sample must be declared rather than silently folded in.
    #[test]
    fn submit_profile_recovers_legacy_lines_from_enters_sent_and_says_so() {
        let log = concat!(
            "[2026-08-08T10:00:00Z] route_pass_through_submit_draft file=a.md pane=%1 harness=claude outcome=cleared enters_sent=2 elapsed_ms=306 capture_failed=false\n",
            "[2026-08-08T10:01:00Z] route_pass_through_submit_draft file=a.md pane=%1 harness=claude outcome=cleared enters_sent=0 elapsed_ms=100 capture_failed=false\n",
        );
        let report = summarize_submit_profile(log, PathBuf::from("ops.log"), None);
        assert_eq!(report.legacy_lines_without_field, 2);
        let claude = &report.harnesses[0];
        assert_eq!(claude.repairs, 2);
        assert_eq!(claude.resubmit_required, 1);
    }
}

#[cfg(test)]
mod crdt_convergence_wait_tests {
    use super::*;

    /// `#caswriteracechurn`: bucket by write site, because "which writer are we
    /// contending with" is the question the settle wait cannot answer on its own.
    #[test]
    fn a_convergence_wait_is_classified_by_its_write_site() {
        for (event, expected) in [
            ("pending_write_crdt_convergence_wait", "pending_write"),
            (
                "serialized_atomic_write_crdt_convergence_wait",
                "serialized_atomic_write",
            ),
            (
                "start_resume_id_capture_crdt_convergence_wait",
                "start_resume_id_capture",
            ),
        ] {
            assert_eq!(crdt_convergence_wait_site(event), Some(expected));
        }
    }

    /// A site-less or unrelated event must not be swept into the bucket.
    #[test]
    fn unrelated_events_are_not_convergence_waits() {
        for event in [
            "commit_success",
            "_crdt_convergence_wait",
            "crdt_convergence_wait_extra",
            "pending_write_crdt_write_coalesced_retry",
        ] {
            assert_eq!(crdt_convergence_wait_site(event), None, "{event}");
        }
    }

    /// End to end through the classifier, with the real line shape from ops.log.
    #[test]
    fn a_real_settle_wait_line_lands_in_the_bucket() {
        let line = "[1786327775] serialized_atomic_write_crdt_convergence_wait \
                    file=tasks/agent-doc/agent-doc-bugs2.md reason=compare_and_swap_raced \
                    elapsed_ms=3215 backoff_ms=25";
        let classified =
            classify_line(line, Path::new("/repo")).expect("a settle wait must classify");
        assert_eq!(
            classified.category,
            "crdt convergence wait serialized_atomic_write"
        );
    }
}
