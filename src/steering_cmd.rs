//! `agent-doc steering <FILE>` — poll or follow mid-turn operator steering
//! (`#midturn-steering`).
//!
//! The in-turn delivery path is the `PostToolUse` hook
//! (`agent-doc hook steering-post-tool-use`). This command is the same core
//! for harnesses without a post-tool hook (OpenCode) and for monitors: a
//! one-shot poll prints the settled steering since this consumer's last
//! poll; `--follow` watches the document (filesystem events, per
//! `#reactive-boundary-ingress`) and prints one JSON line per settled batch.
//! Each mode keeps its own watermark, so polling never steals steering from
//! the hook. Polls keep reporting unsurfaced changes after the cycle closes,
//! until the next preflight re-seeds (`#closeout-steering`).

use std::path::Path;
use std::time::Duration;

use anyhow::{Context, Result};

use agent_doc_session_check_io::midturn_steering::{
    self as steering, CONSUMER_CLI, CONSUMER_FOLLOW, SteeringReport,
};

fn print_report(report: Option<SteeringReport>, json: bool) -> Result<()> {
    if json {
        let value = match report {
            Some(report) => serde_json::to_value(&report)?,
            None => serde_json::json!({ "items": [], "pending": 0 }),
        };
        println!("{}", serde_json::to_string_pretty(&value)?);
        return Ok(());
    }
    match report.as_ref().and_then(SteeringReport::render) {
        Some(context) => println!("{context}"),
        None => {
            let pending = report.map(|report| report.pending).unwrap_or(0);
            if pending > 0 {
                println!(
                    "[agent-doc] no settled steering yet; {pending} item(s) still being typed"
                );
            } else {
                println!("[agent-doc] no new operator steering");
            }
        }
    }
    Ok(())
}

pub fn run(file: &Path, json: bool, peek: bool, follow: bool) -> Result<()> {
    anyhow::ensure!(file.is_file(), "document not found: {}", file.display());
    if !follow {
        let report = steering::observe(file, CONSUMER_CLI, !peek)?;
        return print_report(report, json);
    }
    follow_document(file)
}

/// Stream settled steering as JSON lines until interrupted.
///
/// Filesystem events wake the loop; the bounded timeout exists only so an
/// item held by the debounce is re-evaluated once its quiet period elapses
/// (no further event will arrive for a settled document).
fn follow_document(file: &Path) -> Result<()> {
    use ::notify::{RecursiveMode, Watcher};

    let canonical = file
        .canonicalize()
        .with_context(|| format!("canonicalize {}", file.display()))?;
    let parent = canonical
        .parent()
        .context("document has no parent directory")?
        .to_path_buf();
    let (tx, rx) = std::sync::mpsc::channel::<()>();
    let target = canonical.clone();
    let mut watcher =
        ::notify::recommended_watcher(move |res: ::notify::Result<::notify::Event>| match res {
            Ok(event) if event.paths.iter().any(|path| path == &target) => {
                if tx.send(()).is_err() {
                    eprintln!("[agent-doc] steering follow: receiver closed");
                }
            }
            Ok(_) => {}
            Err(err) => eprintln!("[agent-doc] steering follow watch error: {err}"),
        })?;
    watcher.watch(&parent, RecursiveMode::NonRecursive)?;
    let mut pending = 0usize;
    loop {
        if let Some(report) = steering::observe(&canonical, CONSUMER_FOLLOW, true)? {
            pending = report.pending;
            if !report.items.is_empty() {
                println!("{}", serde_json::to_string(&report)?);
            }
        }
        let wait = if pending > 0 {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(30)
        };
        match rx.recv_timeout(wait) {
            Ok(()) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                anyhow::bail!("steering follow: file watcher stopped")
            }
        }
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// The project a dataset/explain read targets: `file`'s, else the current
/// directory's.
fn gate_log_project(file: Option<&Path>) -> Result<std::path::PathBuf> {
    let anchor = match file {
        Some(file) => {
            anyhow::ensure!(file.is_file(), "document not found: {}", file.display());
            file.to_path_buf()
        }
        None => std::env::current_dir().context("current directory")?,
    };
    agent_doc_fs::find_project_root(&anchor).with_context(|| {
        format!(
            "no agent-doc project (.agent-doc/) above {}",
            anchor.display()
        )
    })
}

/// `agent-doc steering dataset [--json] [--file <FILE>] [--limit N]`
/// (`#steergatelog`): read-only export of the completion-gate decision log.
pub fn run_dataset(file: Option<&Path>, json: bool, limit: Option<usize>) -> Result<()> {
    use agent_doc_session_check_io::steering_gate_log::{self as gate_log, GateLabel};
    let root = gate_log_project(file)?;
    let document = file.map(|file| gate_log::document_key(&root, file));
    let window = match file {
        Some(file) => gate_log::label_window_ms_for(file),
        None => agent_doc_project_config_io::load_project_for_doc(&root.join("."))
            .agent_doc_steering_label_window_ms
            .unwrap_or(gate_log::DEFAULT_LABEL_WINDOW_MS),
    };
    let mut rows = gate_log::export_dataset(&root, document.as_deref(), now_ms(), window)?;
    if let Some(limit) = limit {
        let skip = rows.len().saturating_sub(limit);
        rows.drain(..skip);
    }
    if json {
        println!("{}", serde_json::to_string_pretty(&rows)?);
        return Ok(());
    }
    let count = |label: Option<GateLabel>| rows.iter().filter(|r| r.row.label == label).count();
    println!(
        "[agent-doc] steering gate dataset: {} row(s) in {} — premature={} late={} on_time={} open={}",
        rows.len(),
        root.display(),
        count(Some(GateLabel::Premature)),
        count(Some(GateLabel::Late)),
        count(Some(GateLabel::OnTime)),
        count(None),
    );
    println!("[agent-doc] export with `agent-doc steering dataset --json`");
    Ok(())
}
