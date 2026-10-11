//! GH #235 source-shape guard: no stale-supervisor warning reaches the operator
//! without passing through the recycle-status-aware formatter.
//!
//! One `agent-doc session-check` run printed the CLI entry's
//! `#stalewarnscheduled` line ("do not run `admin recycle`") and then
//! session-check's own line ending in the raw manual remedy ("run `admin
//! recycle`"). The raw remedy reached the report because one path built the
//! message without consulting the recycle request. These checks fail if such a
//! path comes back.

use std::fs;
use std::path::{Path, PathBuf};

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn rust_sources(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|name| name == "target") {
                continue;
            }
            rust_sources(&path, out);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            out.push(path);
        }
    }
}

/// Every non-test Rust source in the workspace: `src/` plus each member
/// crate's `src/`.
fn workspace_sources() -> Vec<(PathBuf, String)> {
    let root = workspace_root();
    let mut files = Vec::new();
    rust_sources(&root.join("src"), &mut files);
    for entry in fs::read_dir(&root).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir()
            && path
                .file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("agent-doc"))
        {
            rust_sources(&path.join("src"), &mut files);
        }
    }
    assert!(files.len() > 50, "workspace scan found too few sources");
    files
        .into_iter()
        .map(|path| {
            let text = fs::read_to_string(&path).unwrap();
            (path.strip_prefix(&root).unwrap().to_path_buf(), text)
        })
        .collect()
}

/// Split a source into top-level items, keyed by the item's first line.
fn top_level_items(source: &str) -> Vec<(String, String)> {
    let starts = [
        "pub fn ",
        "fn ",
        "pub(crate) fn ",
        "mod ",
        "impl",
        "#[cfg(test)]",
    ];
    let mut items: Vec<(String, String)> = Vec::new();
    for line in source.lines() {
        if starts.iter().any(|start| line.starts_with(start)) || items.is_empty() {
            items.push((line.to_string(), String::new()));
        }
        let body = &mut items.last_mut().unwrap().1;
        body.push_str(line);
        body.push('\n');
    }
    items
}

const REMEDY_TEXT: &str = "Refresh it without discarding the live turn";

#[test]
fn manual_remedy_is_owned_only_by_the_controller_status_module() {
    for (path, text) in workspace_sources() {
        if path == Path::new("agent-doc-controller/src/status.rs") {
            continue;
        }
        for owned in [
            REMEDY_TEXT,
            "HOST_SUPERVISOR_STALE_REMEDY",
            "CONTROLLER_STALE_REMEDY",
        ] {
            assert!(
                !text.contains(owned),
                "{} must not own the stale-supervisor remedy `{owned}`; build the warning \
                 through agent_doc_controller::status and show it through \
                 stale_supervisor_warning_for_display (GH #235)",
                path.display()
            );
        }
    }
}

#[test]
fn raw_stale_supervisor_builders_stay_inside_the_controller_crates() {
    for (path, text) in workspace_sources() {
        if path.starts_with("agent-doc-controller/") || path.starts_with("agent-doc-controller-io/")
        {
            continue;
        }
        for raw in [
            "host_supervisor_stale_warning_message(",
            "supervisor_stale_warning_message(",
            "stale_supervisor_warning_with_recycle_status(",
            "host_supervisor_stale_warning_for_doc(",
            "stale_supervisor_condition_for_doc(",
        ] {
            assert!(
                !text.contains(raw),
                "{} builds a raw stale-supervisor warning via `{raw}`; use \
                 stale_supervisor_warning_for_doc / _for_report or a recycle_stale_supervisor_* \
                 helper, which format it against the recycle request (GH #235)",
                path.display()
            );
        }
    }
}

#[test]
fn session_check_report_uses_the_recycle_aware_deduping_reporter() {
    let command =
        fs::read_to_string(workspace_root().join("agent-doc-session-check-io/src/command.rs"))
            .unwrap();
    assert!(
        !command.contains("stale_supervisor_warning_for_doc("),
        "session-check's report must use stale_supervisor_warning_for_report, which reads the \
         outstanding recycle request and skips a supervisor this process already reported \
         (GH #235)"
    );
    assert!(command.contains("stale_supervisor_warning_for_report(file)"));
    // Each turn-stage line session-check prints is recorded, so the report that
    // follows in the same process stays silent for that supervisor.
    let printed = command
        .matches("eprintln!(\"[session-check] WARNING: {message}\");\n        agent_doc_controller_io::project_controller::note_stale_supervisor_warning_shown(&message);")
        .count();
    let turn_stage_calls = command
        .matches("recycle_stale_supervisor_for_turn_stage(")
        .count();
    assert_eq!(
        printed, turn_stage_calls,
        "every printed turn-stage stale-supervisor warning must be noted as shown"
    );
    let main = fs::read_to_string(workspace_root().join("src/main.rs")).unwrap();
    assert!(
        main.contains("note_stale_supervisor_warning_shown(&message)"),
        "the CLI-entry stale-supervisor line must be noted as shown (GH #235)"
    );
}

#[test]
fn every_rpc_path_that_shows_a_raw_warning_formats_it_first() {
    let rpc = fs::read_to_string(
        workspace_root().join("agent-doc-controller-io/src/project_controller/rpc.rs"),
    )
    .unwrap();
    assert!(
        rpc.contains("\nfn stale_supervisor_condition_for_doc("),
        "the raw condition builder must stay private"
    );
    assert!(rpc.contains("\npub(crate) fn host_supervisor_stale_warning_for_doc("));
    let mut checked = 0;
    for (head, body) in top_level_items(&rpc) {
        if head.starts_with("fn stale_supervisor_condition_for_doc(")
            || head.starts_with("pub(crate) fn host_supervisor_stale_warning_for_doc(")
            || head.starts_with("#[cfg(test)]")
            || head.starts_with("mod ")
        {
            continue;
        }
        for raw in [
            "status::host_supervisor_stale_warning_message(",
            "stale_supervisor_condition_for_doc(",
        ] {
            if body.contains(raw) {
                checked += 1;
                assert!(
                    body.contains("status::stale_supervisor_warning_for_display("),
                    "`{head}` uses `{raw}` without stale_supervisor_warning_for_display (GH #235)"
                );
            }
        }
        // A host-supervisor observation used for its TEXT must be formatted;
        // presence checks (`.is_some()` / `.is_none()`) are fine.
        for line in body.lines() {
            if line.contains("host_supervisor_stale_warning_for_doc(")
                && !line.contains(".is_some()")
                && !line.contains(".is_none()")
                && !line.trim_start().starts_with("//")
            {
                checked += 1;
                assert!(
                    body.contains("status::stale_supervisor_warning_for_display("),
                    "`{head}` shows host_supervisor_stale_warning_for_doc text unformatted \
                     (GH #235): {line}"
                );
            }
        }
    }
    assert!(checked >= 4, "guard matched too few call sites ({checked})");
}
