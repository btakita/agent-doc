use std::cell::RefCell;
use std::path::{Path, PathBuf};

use agent_doc_hash::short_content_hash;
use agent_doc_tmux_commands::input_diag::{
    EDITOR_ROUTE_ATTEMPT_ID_ENV, RoutePaneSnapshotFacts, RoutePaneSnapshotFailedLogFacts,
    RoutePaneSnapshotHintFacts, RoutePaneSnapshotLogFacts, format_route_pane_snapshot_failed_log,
    format_route_pane_snapshot_filename, format_route_pane_snapshot_hint,
    format_route_pane_snapshot_log, sanitize_route_snapshot_field,
};
use anyhow::{Context, Result};

thread_local! {
    static EDITOR_ROUTE_ATTEMPT_ID_OVERRIDE: RefCell<Option<String>> = const { RefCell::new(None) };
}

pub struct EditorRouteAttemptIdGuard {
    previous: Option<String>,
}

impl EditorRouteAttemptIdGuard {
    pub fn set(value: Option<&str>) -> Self {
        let sanitized = value
            .map(sanitize_route_snapshot_field)
            .filter(|value| !value.is_empty());
        let previous = EDITOR_ROUTE_ATTEMPT_ID_OVERRIDE.with(|cell| cell.replace(sanitized));
        Self { previous }
    }
}

impl Drop for EditorRouteAttemptIdGuard {
    fn drop(&mut self) {
        let previous = self.previous.take();
        EDITOR_ROUTE_ATTEMPT_ID_OVERRIDE.with(|cell| {
            cell.replace(previous);
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePaneSnapshot {
    pub len: usize,
    pub hash: String,
    pub path: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoutePaneSnapshotPreserveOutcome {
    pub snapshot: RoutePaneSnapshot,
    pub warning: Option<String>,
}

pub fn editor_route_attempt_id() -> Option<String> {
    if let Some(value) = EDITOR_ROUTE_ATTEMPT_ID_OVERRIDE.with(|cell| cell.borrow().clone()) {
        return Some(value);
    }
    std::env::var(EDITOR_ROUTE_ATTEMPT_ID_ENV)
        .ok()
        .map(|value| sanitize_route_snapshot_field(&value))
        .filter(|value| !value.is_empty())
}

fn route_snapshot_timestamp_millis() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0)
}

pub fn preserve_route_pane_snapshot(
    file: &Path,
    pane: &str,
    harness_binary: &str,
    phase: &str,
    content: &str,
    mut ops_logger: impl FnMut(&Path, &str),
) -> RoutePaneSnapshotPreserveOutcome {
    let redacted = agent_doc_secret_redact::redact(content);
    let snapshot = RoutePaneSnapshot {
        len: redacted.len(),
        hash: short_content_hash(&redacted),
        path: None,
    };

    let path = preserve_snapshot_file(file, pane, harness_binary, phase, &snapshot, redacted);

    match path {
        Ok(path) => {
            let file_display = file.display().to_string();
            let snapshot_path = path.display().to_string();
            let editor_attempt_id = editor_route_attempt_id();
            let message = format_route_pane_snapshot_log(RoutePaneSnapshotLogFacts {
                snapshot: RoutePaneSnapshotFacts {
                    file_display: &file_display,
                    pane,
                    harness_binary,
                    phase,
                    capture_len: snapshot.len,
                    capture_hash: &snapshot.hash,
                    editor_attempt_id: editor_attempt_id.as_deref(),
                },
                snapshot_path: &snapshot_path,
            });
            ops_logger(file, &message);
            RoutePaneSnapshotPreserveOutcome {
                snapshot: RoutePaneSnapshot {
                    path: Some(path),
                    ..snapshot
                },
                warning: None,
            }
        }
        Err(err) => {
            let file_display = file.display().to_string();
            let error = err.to_string();
            let editor_attempt_id = editor_route_attempt_id();
            let message = format_route_pane_snapshot_failed_log(RoutePaneSnapshotFailedLogFacts {
                snapshot: RoutePaneSnapshotFacts {
                    file_display: &file_display,
                    pane,
                    harness_binary,
                    phase,
                    capture_len: snapshot.len,
                    capture_hash: &snapshot.hash,
                    editor_attempt_id: editor_attempt_id.as_deref(),
                },
                error: &error,
            });
            ops_logger(file, &message);
            RoutePaneSnapshotPreserveOutcome {
                snapshot,
                warning: Some(error),
            }
        }
    }
}

fn preserve_snapshot_file(
    file: &Path,
    pane: &str,
    harness_binary: &str,
    phase: &str,
    snapshot: &RoutePaneSnapshot,
    redacted: String,
) -> Result<PathBuf> {
    let canonical = file
        .canonicalize()
        .with_context(|| format!("failed to canonicalize {}", file.display()))?;
    let root = agent_doc_fs::find_project_root(&canonical)
        .with_context(|| format!("could not find .agent-doc root for {}", file.display()))?;
    let dir = root.join(".agent-doc/logs/route-submit");
    std::fs::create_dir_all(&dir).with_context(|| format!("failed to create {}", dir.display()))?;
    let name = format_route_pane_snapshot_filename(
        route_snapshot_timestamp_millis(),
        phase,
        harness_binary,
        pane,
        snapshot.hash.as_str(),
    );
    let path = dir.join(name);
    std::fs::write(&path, redacted)
        .with_context(|| format!("failed to write {}", path.display()))?;
    Ok(path)
}

pub fn route_pane_snapshot_hint(
    file: &Path,
    pane: &str,
    harness_binary: &str,
    phase: &str,
    snapshot: &RoutePaneSnapshot,
) -> String {
    let file_display = file.display().to_string();
    let snapshot_path = snapshot
        .path
        .as_ref()
        .map(|path| path.display().to_string());
    let editor_attempt_id = editor_route_attempt_id();
    format_route_pane_snapshot_hint(RoutePaneSnapshotHintFacts {
        snapshot: RoutePaneSnapshotFacts {
            file_display: &file_display,
            pane,
            harness_binary,
            phase,
            capture_len: snapshot.len,
            capture_hash: &snapshot.hash,
            editor_attempt_id: editor_attempt_id.as_deref(),
        },
        snapshot_path: snapshot_path.as_deref(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_pane_snapshot_preserves_redacted_terminal_capture() {
        let tmp = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join(".agent-doc")).unwrap();
        let file = tmp.path().join("session.md");
        std::fs::write(&file, "session").unwrap();
        let content = "\
> agent-doc tasks/agent-doc/agent-doc-bugs2.md
OPENAI_API_KEY=sk-proj-aaaaaaaaaaaaaaaaaaaaaaaa
";
        let mut messages = Vec::new();

        let outcome = preserve_route_pane_snapshot(
            &file,
            "%7",
            "codex",
            "direct_pane_acceptance",
            content,
            |path, message| messages.push((path.to_path_buf(), message.to_string())),
        );

        let path = outcome
            .snapshot
            .path
            .expect("snapshot path should be preserved");
        assert!(path.starts_with(tmp.path().join(".agent-doc/logs/route-submit")));
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(
            saved.contains("OPENAI_API_KEY=[REDACTED]"),
            "snapshot should redact named API keys: {saved}"
        );
        assert!(
            !saved.contains("sk-proj-aaaaaaaa"),
            "raw token must not be preserved in snapshot: {saved}"
        );

        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].0, file);
        let message = &messages[0].1;
        assert!(message.contains("route_pane_snapshot"), "{message}");
        assert!(
            message.contains("phase=direct_pane_acceptance"),
            "{message}"
        );
        assert!(message.contains("capture_hash="), "{message}");
        assert!(message.contains("snapshot_path="), "{message}");
    }

    #[test]
    fn route_pane_snapshot_hint_includes_snapshot_path() {
        let file = PathBuf::from("/tmp/session.md");
        let snapshot = RoutePaneSnapshot {
            len: 10,
            hash: "abc123".to_string(),
            path: Some(PathBuf::from("/tmp/.agent-doc/logs/route-submit/snap.txt")),
        };

        let hint = route_pane_snapshot_hint(&file, "%7", "codex", "phase", &snapshot);

        assert!(
            hint.contains("preserved dispatch-start proof snapshot"),
            "{hint}"
        );
        assert!(
            hint.contains("snapshot_path=/tmp/.agent-doc/logs/route-submit/snap.txt"),
            "{hint}"
        );
    }
}
