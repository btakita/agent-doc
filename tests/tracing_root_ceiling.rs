//! `#tracingrootceiling`: the `AGENT_DOC_LOG` debug-log location is resolved
//! through the ceiling-aware project-root walk, so a process bounded by
//! `AGENT_DOC_ROOT_CEILING_DIRECTORIES` never writes `debug.log` (or anything
//! else) into an ancestor project above the ceiling. Before the fix the tracing
//! setup climbed from the cwd with no limit, which is how `~/.agent-doc/logs`
//! collected a `debug.log.<date>` from processes that should never reach it.

use std::path::{Path, PathBuf};
use std::process::Output;

/// `<tmp>/ancestor/.agent-doc/logs/` is a project above the ceiling;
/// `<tmp>/ancestor/ceiling/work/` is the process working directory.
struct Layout {
    _tmp: tempfile::TempDir,
    home: PathBuf,
    ancestor: PathBuf,
    ceiling: PathBuf,
    work: PathBuf,
}

fn layout() -> Layout {
    let tmp = tempfile::tempdir().unwrap();
    let home = tmp.path().join("home");
    let ancestor = tmp.path().join("ancestor");
    let ceiling = ancestor.join("ceiling");
    let work = ceiling.join("work");
    std::fs::create_dir_all(&home).unwrap();
    std::fs::create_dir_all(ancestor.join(".agent-doc/logs")).unwrap();
    std::fs::create_dir_all(&work).unwrap();
    Layout {
        _tmp: tmp,
        home,
        ancestor,
        ceiling,
        work,
    }
}

fn run_with_log(layout: &Layout, ceiling: &std::ffi::OsStr) -> Output {
    std::process::Command::new(assert_cmd::cargo::cargo_bin!("agent-doc"))
        .args(["version", "--json"])
        .env("HOME", &layout.home)
        .env("AGENT_DOC_LOG", "debug")
        .env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", ceiling)
        .current_dir(&layout.work)
        .output()
        .unwrap()
}

/// Every file under `dir`, relative to it.
fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let mut stack = vec![dir.to_path_buf()];
    while let Some(next) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&next) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else {
                out.push(path.strip_prefix(dir).unwrap().to_path_buf());
            }
        }
    }
    out.sort();
    out
}

fn debug_logs(logs: &Path) -> Vec<PathBuf> {
    files_under(logs)
        .into_iter()
        .filter(|path| path.to_string_lossy().starts_with("debug.log"))
        .collect()
}

#[test]
fn debug_log_never_lands_in_an_ancestor_project_above_the_root_ceiling() {
    let layout = layout();
    let output = run_with_log(&layout, layout.ceiling.as_os_str());
    assert!(output.status.success(), "{output:?}");

    let ancestor_state = layout.ancestor.join(".agent-doc");
    assert_eq!(
        files_under(&ancestor_state),
        Vec::<PathBuf>::new(),
        "the ancestor project above the ceiling must receive no debug.log, ops.log or other write"
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("logging disabled"),
        "with no project root inside the ceiling the file log is disabled, stderr={stderr}"
    );
}

/// Control: the same layout without a ceiling *does* log into the ancestor, so
/// the bounded case above is refusing a write that would otherwise happen.
#[test]
fn without_a_ceiling_the_ancestor_project_owns_the_debug_log() {
    let layout = layout();
    let output = run_with_log(&layout, std::ffi::OsStr::new(""));
    assert!(output.status.success(), "{output:?}");
    assert!(
        !debug_logs(&layout.ancestor.join(".agent-doc/logs")).is_empty(),
        "unbounded discovery adopts the ancestor project; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
}

/// The nearest project root owns the log even before its `logs/` exists. The
/// old walk skipped a root without `logs/` and climbed to an outer project.
#[test]
fn nearest_project_root_owns_the_debug_log_even_without_a_logs_dir() {
    let layout = layout();
    std::fs::create_dir_all(layout.work.join(".agent-doc")).unwrap();
    let output = run_with_log(&layout, std::ffi::OsStr::new(""));
    assert!(output.status.success(), "{output:?}");

    assert!(
        !debug_logs(&layout.work.join(".agent-doc/logs")).is_empty(),
        "the nearest root's logs/ is created and holds the debug log; stderr={}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        files_under(&layout.ancestor.join(".agent-doc")),
        Vec::<PathBuf>::new(),
        "an outer project must not receive the nested project's debug log"
    );
}
