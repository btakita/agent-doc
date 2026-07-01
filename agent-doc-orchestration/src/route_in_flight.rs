//! Short-lived route-submit marker.
//!
//! A JetBrains `Run Agent Doc` route command and the supervisor idle-queue watch
//! are separate processes/threads that can both observe an idle owned pane. The
//! route path owns the pane while it is submitting and waiting for dispatch proof;
//! this sidecar lets the idle watcher back off instead of sending a context reset
//! or queue drain into the same prompt window.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use agent_doc_supervisor::route_submit_inflight::{
    ROUTE_BLOCKED_DIR, ROUTE_BLOCKED_TTL_SECS, ROUTE_DISPATCH_SUBMIT_REASON, ROUTE_IN_FLIGHT_DIR,
    RouteSubmitBlocked, RouteSubmitInFlight, RouteSubmitMarkerJson,
    parse_route_submit_blocked_marker_json, parse_route_submit_inflight_marker_json,
    route_submit_blocked_marker, route_submit_blocked_marker_json, route_submit_inflight_marker,
    route_submit_inflight_marker_json,
};
use anyhow::{Context, Result};

pub struct RouteSubmitInFlightGuard {
    path: Option<PathBuf>,
    marker: Option<RouteSubmitInFlight>,
}

impl Drop for RouteSubmitInFlightGuard {
    fn drop(&mut self) {
        let Some(path) = self.path.as_ref() else {
            return;
        };
        match std::fs::remove_file(path) {
            Ok(()) => {
                if let Some(marker) = self.marker.as_ref() {
                    crate::ops_log::log_op(
                        Path::new(&marker.file),
                        &format!(
                            "route_submit_in_flight_marker_cleared file={} pane={} harness={} reason={}",
                            marker.file,
                            marker.pane,
                            marker.harness,
                            marker.reason_label()
                        ),
                    );
                }
            }
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                eprintln!(
                    "[route] warning: failed to clear route-submit marker {}: {err}",
                    path.display()
                );
                if let Some(marker) = self.marker.as_ref() {
                    crate::ops_log::log_op(
                        Path::new(&marker.file),
                        &format!(
                            "route_submit_in_flight_marker_clear_failed file={} pane={} harness={} reason={} error={:?}",
                            marker.file,
                            marker.pane,
                            marker.harness,
                            marker.reason_label(),
                            err.to_string()
                        ),
                    );
                }
            }
        }
    }
}

fn now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

fn marker_path(file: &Path) -> Result<Option<PathBuf>> {
    marker_path_in(file, ROUTE_IN_FLIGHT_DIR)
}

fn blocked_marker_path(file: &Path) -> Result<Option<PathBuf>> {
    marker_path_in(file, ROUTE_BLOCKED_DIR)
}

fn marker_path_in(file: &Path, dir: &str) -> Result<Option<PathBuf>> {
    let Some(root) = agent_doc_fs::find_project_root(file) else {
        return Ok(None);
    };
    let hash = crate::snapshot::doc_hash(file)?;
    Ok(Some(root.join(dir).join(format!("{hash}.json"))))
}

pub fn begin_route_submit(
    file: &Path,
    pane: &str,
    harness: &str,
) -> Result<RouteSubmitInFlightGuard> {
    begin_route_submit_with_reason(file, pane, harness, ROUTE_DISPATCH_SUBMIT_REASON)
}

pub fn begin_route_submit_with_reason(
    file: &Path,
    pane: &str,
    harness: &str,
    reason: &str,
) -> Result<RouteSubmitInFlightGuard> {
    let Some(path) = marker_path(file)? else {
        return Ok(RouteSubmitInFlightGuard {
            path: None,
            marker: None,
        });
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let marker = route_submit_inflight_marker(
        file.to_string_lossy().into_owned(),
        pane,
        harness,
        reason,
        now_secs(),
    );
    let json =
        route_submit_inflight_marker_json(&marker).context("serialize route-submit marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    crate::ops_log::log_op(
        file,
        &format!(
            "route_submit_in_flight_marker_set file={} pane={} harness={} reason={} ttl_secs={}",
            file.display(),
            pane,
            harness,
            marker.reason_label(),
            marker.ttl_secs()
        ),
    );
    Ok(RouteSubmitInFlightGuard {
        path: Some(path),
        marker: Some(marker),
    })
}

pub fn route_submit_in_flight(file: &Path) -> Result<bool> {
    let Some(path) = marker_path(file)? else {
        return Ok(false);
    };
    if active_in_flight_marker_at(&path)? {
        return Ok(true);
    }
    route_submit_blocked(file).map(|marker| marker.is_some())
}

fn active_in_flight_marker_at(path: &Path) -> Result<bool> {
    let content = match std::fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(err) => return Err(err).with_context(|| format!("read {}", path.display())),
    };
    match parse_route_submit_inflight_marker_json(&content, now_secs()) {
        RouteSubmitMarkerJson::Fresh(_) => Ok(true),
        RouteSubmitMarkerJson::Malformed => {
            remove_marker_file(path, "malformed_route_submit_marker");
            Ok(false)
        }
        RouteSubmitMarkerJson::Stale => {
            remove_marker_file(path, "stale_route_submit_marker");
            Ok(false)
        }
    }
}

fn remove_marker_file(path: &Path, reason: &str) {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => {
            eprintln!(
                "[route] warning: failed to remove {reason} {}: {err}",
                path.display()
            );
        }
    }
}

pub fn mark_route_submit_blocked(
    file: &Path,
    pane: &str,
    harness: &str,
    reason: &str,
) -> Result<()> {
    let Some(path) = blocked_marker_path(file)? else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).with_context(|| format!("create {}", parent.display()))?;
    }
    let marker = route_submit_blocked_marker(
        file.to_string_lossy().into_owned(),
        pane,
        harness,
        reason,
        now_secs(),
    );
    let json = route_submit_blocked_marker_json(&marker)
        .context("serialize route-submit blocked marker")?;
    std::fs::write(&path, json).with_context(|| format!("write {}", path.display()))?;
    crate::ops_log::log_op(
        file,
        &format!(
            "route_submit_blocked_marker_set file={} pane={} harness={} reason={} ttl_secs={}",
            file.display(),
            pane,
            harness,
            reason,
            ROUTE_BLOCKED_TTL_SECS
        ),
    );
    Ok(())
}

pub fn route_submit_blocked(file: &Path) -> Result<Option<RouteSubmitBlocked>> {
    let Some(path) = blocked_marker_path(file)? else {
        return Ok(None);
    };
    let content = match std::fs::read_to_string(&path) {
        Ok(content) => content,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).with_context(|| format!("read {}", path.display())),
    };
    match parse_route_submit_blocked_marker_json(&content, now_secs()) {
        RouteSubmitMarkerJson::Fresh(marker) => Ok(Some(marker)),
        RouteSubmitMarkerJson::Malformed => {
            remove_marker_file(&path, "malformed_route_submit_blocked_marker");
            Ok(None)
        }
        RouteSubmitMarkerJson::Stale => {
            remove_marker_file(&path, "stale_route_submit_blocked_marker");
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_supervisor::route_submit_inflight::{
        ROUTE_DISPATCH_ONLY_READY_PROBE_REASON, ROUTE_IN_FLIGHT_TTL_SECS,
        ROUTE_READY_PROBE_TTL_SECS,
    };

    #[test]
    fn route_submit_marker_is_active_until_guard_drops() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();

        let guard = begin_route_submit(&doc, "%1", "codex").unwrap();
        assert!(route_submit_in_flight(&doc).unwrap());
        let content = std::fs::read_to_string(marker_path(&doc).unwrap().unwrap()).unwrap();
        let marker = match parse_route_submit_inflight_marker_json(&content, now_secs()) {
            RouteSubmitMarkerJson::Fresh(marker) => marker,
            state => panic!("expected fresh marker, got {state:?}"),
        };
        assert_eq!(marker.reason, "dispatch_submit");
        drop(guard);
        assert!(!route_submit_in_flight(&doc).unwrap());
    }

    #[test]
    fn route_submit_marker_records_ready_probe_reason_until_guard_drops() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();

        let guard = begin_route_submit_with_reason(
            &doc,
            "%42",
            "codex",
            ROUTE_DISPATCH_ONLY_READY_PROBE_REASON,
        )
        .unwrap();
        assert!(route_submit_in_flight(&doc).unwrap());
        let path = marker_path(&doc).unwrap().unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        let marker = match parse_route_submit_inflight_marker_json(&content, now_secs()) {
            RouteSubmitMarkerJson::Fresh(marker) => marker,
            state => panic!("expected fresh marker, got {state:?}"),
        };
        assert_eq!(marker.pane, "%42");
        assert_eq!(marker.harness, "codex");
        assert_eq!(marker.reason, ROUTE_DISPATCH_ONLY_READY_PROBE_REASON);

        drop(guard);
        assert!(!path.exists());
        assert!(!route_submit_in_flight(&doc).unwrap());
    }

    #[test]
    fn ready_probe_route_submit_marker_survives_editor_wait_budget() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();
        let path = marker_path(&doc).unwrap().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let marker = RouteSubmitInFlight {
            file: doc.display().to_string(),
            pane: "%1".to_string(),
            harness: "codex".to_string(),
            reason: ROUTE_DISPATCH_ONLY_READY_PROBE_REASON.to_string(),
            written_at: now_secs().saturating_sub(ROUTE_IN_FLIGHT_TTL_SECS + 1),
        };
        std::fs::write(&path, route_submit_inflight_marker_json(&marker).unwrap()).unwrap();

        assert!(route_submit_in_flight(&doc).unwrap());

        let stale = RouteSubmitInFlight {
            written_at: now_secs().saturating_sub(ROUTE_READY_PROBE_TTL_SECS + 1),
            ..marker
        };
        std::fs::write(&path, route_submit_inflight_marker_json(&stale).unwrap()).unwrap();
        assert!(!route_submit_in_flight(&doc).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn route_submit_marker_ignores_stale_payloads() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();
        let path = marker_path(&doc).unwrap().unwrap();
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        let marker = RouteSubmitInFlight {
            file: doc.display().to_string(),
            pane: "%1".to_string(),
            harness: "codex".to_string(),
            reason: "dispatch_submit".to_string(),
            written_at: now_secs().saturating_sub(ROUTE_IN_FLIGHT_TTL_SECS + 1),
        };
        std::fs::write(&path, route_submit_inflight_marker_json(&marker).unwrap()).unwrap();

        assert!(!route_submit_in_flight(&doc).unwrap());
        assert!(!path.exists());
    }

    #[test]
    fn route_submit_blocked_marker_blocks_idle_drain_until_stale() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("plan.md");
        std::fs::write(&doc, "body").unwrap();

        mark_route_submit_blocked(&doc, "%1", "codex", "accepted_without_dispatch_start_proof")
            .unwrap();
        assert!(route_submit_in_flight(&doc).unwrap());
        let marker = route_submit_blocked(&doc).unwrap().unwrap();
        assert_eq!(marker.reason, "accepted_without_dispatch_start_proof");

        let path = blocked_marker_path(&doc).unwrap().unwrap();
        let stale = RouteSubmitBlocked {
            file: doc.display().to_string(),
            pane: "%1".to_string(),
            harness: "codex".to_string(),
            reason: "accepted_without_dispatch_start_proof".to_string(),
            written_at: now_secs().saturating_sub(ROUTE_BLOCKED_TTL_SECS + 1),
        };
        std::fs::write(&path, route_submit_blocked_marker_json(&stale).unwrap()).unwrap();
        assert!(!route_submit_in_flight(&doc).unwrap());
        assert!(!path.exists());
    }
}
