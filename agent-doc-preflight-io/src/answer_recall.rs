//! tsift-backed recall for the answer ledger (`#decisiontsiftrecall`).
//!
//! [`TsiftRecall`] implements [`AnswerRecall`] by running the external
//! `tsift --envelope search` CLI. It is optional end to end: there is no build
//! dependency on tsift, the source is used only when the project already has
//! a tsift index (`.tsift/index.db`) and a `tsift` executable is on `PATH`
//! (or named by `AGENT_DOC_TSIFT_BIN`), and every query shares one bounded
//! deadline after which the processes are killed. Any failure yields no hits,
//! so the ledger degrades to its in-process tiers with no semantic or
//! cross-document notes. Hits are annotations only; nothing here closes a
//! copy or refuses a closeout.
//!
//! Environment:
//! - `AGENT_DOC_ANSWER_RECALL=off` (or `0`, `false`, `no`) disables recall.
//! - `AGENT_DOC_TSIFT_BIN` names the tsift executable (default `tsift`).
//! - `AGENT_DOC_ANSWER_RECALL_TIMEOUT_MS` is the deadline for one pass
//!   (default 2500, minimum 100).

use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use agent_doc_answer_ledger::recall::{AnswerRecall, RecallHit};
use serde_json::Value;
use wait_timeout::ChildExt;

const DEFAULT_TIMEOUT_MS: u64 = 2500;
const MAX_REPORT_BYTES: u64 = 1024 * 1024;

/// The process environment recall reads, injectable for tests.
#[derive(Debug, Clone, Default)]
pub struct RecallEnv {
    pub disabled: bool,
    pub tsift_bin: Option<String>,
    pub path: Option<OsString>,
    pub timeout_ms: Option<u64>,
}

impl RecallEnv {
    pub fn from_process() -> Self {
        Self {
            disabled: std::env::var("AGENT_DOC_ANSWER_RECALL").is_ok_and(|value| {
                matches!(
                    value.trim().to_ascii_lowercase().as_str(),
                    "0" | "off" | "false" | "no"
                )
            }),
            tsift_bin: std::env::var("AGENT_DOC_TSIFT_BIN")
                .ok()
                .filter(|value| !value.trim().is_empty()),
            path: std::env::var_os("PATH"),
            timeout_ms: std::env::var("AGENT_DOC_ANSWER_RECALL_TIMEOUT_MS")
                .ok()
                .and_then(|value| value.trim().parse().ok()),
        }
    }
}

/// `tsift --envelope search` as an [`AnswerRecall`] source.
#[derive(Debug, Clone)]
pub struct TsiftRecall {
    bin: PathBuf,
    root: PathBuf,
    deadline: Duration,
}

fn is_executable(path: &Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

fn resolve_bin(name: &str, path: Option<&OsString>) -> Option<PathBuf> {
    if name.contains(std::path::MAIN_SEPARATOR) || name.contains('/') {
        let candidate = PathBuf::from(name);
        return is_executable(&candidate).then_some(candidate);
    }
    std::env::split_paths(path?)
        .map(|dir| dir.join(name))
        .find(|candidate| is_executable(candidate))
}

impl TsiftRecall {
    /// The source for `file`'s project, or `None` when recall is disabled,
    /// the project has no tsift index, or no tsift executable is found. No
    /// process is started to find out.
    pub fn discover(file: &Path, env: &RecallEnv) -> Option<Self> {
        if env.disabled {
            return None;
        }
        let root = agent_doc_project_root_io::project_root_containing(file)?;
        if !root.join(".tsift/index.db").is_file() {
            return None;
        }
        let bin = resolve_bin(
            env.tsift_bin.as_deref().unwrap_or("tsift"),
            env.path.as_ref(),
        )?;
        Some(Self {
            bin,
            root: std::fs::canonicalize(&root).unwrap_or(root),
            deadline: Duration::from_millis(env.timeout_ms.unwrap_or(DEFAULT_TIMEOUT_MS).max(100)),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    fn spawn(&self, query: &str, limit: usize) -> std::io::Result<Child> {
        let mut attempt = 0;
        loop {
            let mut command = Command::new(&self.bin);
            // Own process group, so a deadline kill reaches its descendants.
            #[cfg(unix)]
            std::os::unix::process::CommandExt::process_group(&mut command, 0);
            let spawned = command
                .args(["--envelope", "search"])
                .arg(query)
                .args(["--strategy", "hybrid", "--absolute", "--budget", "normal"])
                .arg("--limit")
                .arg(limit.to_string())
                .current_dir(&self.root)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn();
            // ETXTBSY: the executable is being replaced (an install) or was
            // just written; it clears within milliseconds.
            match spawned {
                Err(err) if err.kind() == std::io::ErrorKind::ExecutableFileBusy && attempt < 5 => {
                    attempt += 1;
                    std::thread::sleep(Duration::from_millis(20));
                }
                other => return other,
            }
        }
    }

    /// Canonical absolute form of a hit path (relative paths are taken
    /// against the project root).
    fn hit_path(&self, raw: &str) -> PathBuf {
        let path = Path::new(raw);
        let joined = if path.is_absolute() {
            path.to_path_buf()
        } else {
            self.root.join(path)
        };
        std::fs::canonicalize(&joined).unwrap_or(joined)
    }
}

/// Kill a timed-out search and everything in its process group.
fn kill_tree(child: &mut Child) {
    #[cfg(unix)]
    if let Ok(pid) = libc::pid_t::try_from(child.id()) {
        // SAFETY: signalling the process group this child leads; no memory
        // is shared with the callee.
        unsafe {
            libc::kill(-pid, libc::SIGKILL);
        }
    }
    let _ = child.kill();
    let _ = child.wait();
}

/// Session-document hits only: Markdown outside agent-doc / tsift state and
/// build output.
fn eligible(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "md")
        && !path.components().any(|part| {
            matches!(
                part.as_os_str().to_str(),
                Some(".agent-doc" | ".tsift" | ".git" | "target" | "node_modules")
            )
        })
}

/// Hits from one `tsift --envelope search` JSON report. `ranked` carries the
/// backend per row; `hits` is the fallback shape.
pub fn parse_report(report: &Value) -> Vec<(String, usize, f64, bool)> {
    let body = report.get("report").unwrap_or(report);
    let rows = body
        .get("ranked")
        .and_then(Value::as_array)
        .filter(|rows| !rows.is_empty())
        .or_else(|| body.get("hits").and_then(Value::as_array));
    let Some(rows) = rows else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            let path = row.get("path")?.as_str()?.to_string();
            let line = usize::try_from(row.get("line")?.as_u64()?).ok()?;
            let score = row.get("score").and_then(Value::as_f64).unwrap_or(0.0);
            let semantic = row
                .get("source")
                .and_then(Value::as_str)
                .is_some_and(|source| {
                    ["vector", "semantic", "embed"]
                        .iter()
                        .any(|kind| source.contains(kind))
                });
            Some((path, line, score, semantic))
        })
        .collect()
}

impl AnswerRecall for TsiftRecall {
    fn recall(&self, queries: &[String], limit: usize) -> Vec<Vec<RecallHit>> {
        let started = Instant::now();
        // All queries run at once against one shared deadline.
        // The reader hands its bytes over a channel so a descendant that keeps
        // the pipe open can never hold the pass past its deadline.
        let mut running: Vec<Option<(Child, std::sync::mpsc::Receiver<Vec<u8>>)>> = queries
            .iter()
            .map(|query| {
                let mut child = self.spawn(query, limit).ok()?;
                let mut stdout = child.stdout.take()?;
                let (sender, receiver) = std::sync::mpsc::channel();
                std::thread::spawn(move || {
                    let mut bytes = Vec::new();
                    let _ = (&mut stdout)
                        .take(MAX_REPORT_BYTES + 1)
                        .read_to_end(&mut bytes);
                    let _ = sender.send(bytes);
                });
                Some((child, receiver))
            })
            .collect();
        let mut results = Vec::with_capacity(queries.len());
        let mut timed_out = 0usize;
        for slot in &mut running {
            let Some((mut child, reader)) = slot.take() else {
                results.push(Vec::new());
                continue;
            };
            let remaining = self.deadline.saturating_sub(started.elapsed());
            let status = match child.wait_timeout(remaining) {
                Ok(Some(status)) => Some(status),
                _ => {
                    kill_tree(&mut child);
                    timed_out += 1;
                    None
                }
            };
            let bytes = if status.is_some() {
                let remaining = self
                    .deadline
                    .saturating_sub(started.elapsed())
                    .max(Duration::from_millis(50));
                reader.recv_timeout(remaining).unwrap_or_default()
            } else {
                Vec::new()
            };
            let hits = match status {
                Some(status) if status.success() && (bytes.len() as u64) <= MAX_REPORT_BYTES => {
                    serde_json::from_slice::<Value>(&bytes)
                        .map(|report| parse_report(&report))
                        .unwrap_or_default()
                }
                _ => Vec::new(),
            };
            results.push(
                hits.into_iter()
                    .filter_map(|(path, line, score, semantic)| {
                        let path = self.hit_path(&path);
                        eligible(&path).then(|| RecallHit {
                            path: path.to_string_lossy().into_owned(),
                            line,
                            score,
                            semantic,
                        })
                    })
                    .collect(),
            );
        }
        if timed_out > 0 {
            eprintln!(
                "[preflight] answer ledger: tsift recall hit its {}ms deadline ({timed_out} of {} queries); continuing without them",
                self.deadline.as_millis(),
                queries.len()
            );
        }
        results
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn report_rows_parse_from_ranked_then_hits() {
        let report = serde_json::json!({
            "report": {
                "ranked": [
                    {"path": "/p/a.md", "line": 4, "score": 10.25, "source": "lexical_file"},
                    {"path": "/p/b.md", "line": 2, "score": 0.8, "source": "vector_chunk"},
                    {"path": "/p/c.md", "score": 1.0}
                ],
                "hits": [{"path": "/p/ignored.md", "line": 1, "score": 1.0}]
            }
        });
        assert_eq!(
            parse_report(&report),
            vec![
                ("/p/a.md".to_string(), 4, 10.25, false),
                ("/p/b.md".to_string(), 2, 0.8, true),
            ]
        );
        let hits_only = serde_json::json!({
            "report": {"ranked": [], "hits": [{"path": "x.md", "line": 3, "score": 5.0}]}
        });
        assert_eq!(
            parse_report(&hits_only),
            vec![("x.md".to_string(), 3, 5.0, false)]
        );
        assert!(parse_report(&serde_json::json!({"error": "stale"})).is_empty());
    }

    #[test]
    fn only_session_markdown_is_eligible() {
        assert!(eligible(Path::new("/p/tasks/a.md")));
        assert!(!eligible(Path::new("/p/src/lib.rs")));
        assert!(!eligible(Path::new("/p/.agent-doc/snapshots/a.md")));
        assert!(!eligible(Path::new("/p/target/doc/a.md")));
    }
}
