//! # Module: fleet_board_cmd
//!
//! ## Spec
//! - `agent-doc board [ROOT] [--json] [--dag] [--all] [--no-submodules]`
//!   renders one severity-ordered view of the queue + backlog of every session
//!   document across a superproject and its submodules.
//! - Root discovery climbs to the superproject with
//!   `git rev-parse --show-superproject-working-tree`, then takes every
//!   `.gitmodules` path that exists and carries its own `.agent-doc/`. Each of
//!   those roots becomes its own section, so `src/haiven-dev`,
//!   `src/boost-client`, and `src/equityfundingsource-dev` group separately.
//! - Document discovery per root is the durable session registry plus a bounded
//!   filesystem scan that stops at nested submodule roots, so a document is
//!   attributed to exactly one project.
//! - `CLEAR` rows are hidden unless `--all` is passed.
//!
//! ## Agentic Contracts
//! - Read-only. This command never writes a document, claims a pane, or touches
//!   controller state; a wedged project must still render.
//! - Per-root failures degrade to a diagnostic row rather than aborting the
//!   board: one unreadable submodule must not hide the other forty.
//! - Classification, ordering, and rendering are pure and live in
//!   `agent_doc_work_graph::fleet_board`. This module owns only discovery,
//!   parsing, and output.
//!
//! ## Evals
//! - `board_groups_documents_by_submodule_root`
//! - `board_scan_stops_at_nested_submodule_roots`
//! - `board_marks_a_queued_document_with_no_actor_as_stalled`

use anyhow::{Context, Result};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use agent_doc_work_graph::fleet_board::{
    DocumentFacts, FleetBoard, LaneCounts, build_board, render_board_dag, render_board_text,
    without_clear_rows,
};

/// Directories that never hold a live session document worth scanning.
const IGNORED_SCAN_DIRS: [&str; 6] = ["node_modules", "target", "dist", "build", "vendor", "tmp"];

/// True for a directory the scan must not descend into. Hidden directories cover
/// `.git` / `.agent-doc` / `.venv` / `.tsift`; a leading underscore is agent-doc's
/// own quarantine-and-aside convention (`_stale-aside`, `_quarantine-<id>`), whose
/// documents are recovery residue rather than live sessions.
fn is_ignored_scan_dir(name: &str) -> bool {
    name.starts_with('.') || name.starts_with('_') || IGNORED_SCAN_DIRS.contains(&name)
}

/// Options for `agent-doc board`.
pub(crate) struct BoardOptions {
    pub root: Option<PathBuf>,
    pub json: bool,
    pub dag: bool,
    pub all: bool,
    pub no_submodules: bool,
}

pub(crate) fn run_command(options: BoardOptions) -> Result<()> {
    let base = resolve_base_root(options.root.as_deref())?;
    let board = board_for_root(&base, options.all, options.no_submodules);
    if options.json {
        println!("{}", serde_json::to_string_pretty(&board)?);
        return Ok(());
    }
    print!("{}", render_board_text(&board));
    if options.dag {
        println!();
        print!("{}", render_board_dag(&board));
    }
    Ok(())
}

/// Build the whole board for one base root. Shared by `agent-doc board` and the
/// `serve` HTTP view so both render exactly the same model.
pub(crate) fn board_for_root(base: &Path, include_clear: bool, no_submodules: bool) -> FleetBoard {
    board_for_projects(&project_roots(base, no_submodules), include_clear)
}

/// The project sections a board over `base` covers: just `base`, or the whole
/// superproject fan-out. Shared with `agent-doc dashboard` so its controller
/// liveness section covers exactly the projects its work board does.
pub(crate) fn project_roots(base: &Path, no_submodules: bool) -> Vec<ProjectRoot> {
    if no_submodules {
        vec![ProjectRoot {
            label: project_label(base, base),
            root: base.to_path_buf(),
        }]
    } else {
        discover_project_roots(base)
    }
}

/// Build the board over an already-discovered set of project sections.
pub(crate) fn board_for_projects(roots: &[ProjectRoot], include_clear: bool) -> FleetBoard {
    let owned: BTreeSet<PathBuf> = roots.iter().map(|project| project.root.clone()).collect();
    let facts: Vec<_> = roots
        .iter()
        .flat_map(|project| collect_project_facts(project, &owned))
        .collect();
    let board = build_board(facts);
    if include_clear {
        board
    } else {
        without_clear_rows(board)
    }
}

/// One project section of the board.
pub(crate) struct ProjectRoot {
    pub(crate) label: String,
    pub(crate) root: PathBuf,
}

fn resolve_base_root(root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = root {
        return Ok(root.canonicalize().unwrap_or_else(|_| root.to_path_buf()));
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    Ok(agent_doc_fs::find_project_root(&cwd).unwrap_or(cwd))
}

/// Climb to the outermost superproject, then fan back out over its submodules.
fn discover_project_roots(base: &Path) -> Vec<ProjectRoot> {
    let superproject = superproject_root(base);
    let mut roots = Vec::new();
    let mut seen = BTreeSet::new();

    let push = |root: PathBuf, roots: &mut Vec<ProjectRoot>, seen: &mut BTreeSet<PathBuf>| {
        if !root.join(".agent-doc").is_dir() || !seen.insert(root.clone()) {
            return;
        }
        roots.push(ProjectRoot {
            label: project_label(&superproject, &root),
            root,
        });
    };

    push(superproject.clone(), &mut roots, &mut seen);
    for path in submodule_paths(&superproject) {
        push(superproject.join(path), &mut roots, &mut seen);
    }
    // The base root may sit outside the superproject's `.gitmodules` (a plain
    // nested checkout). Never drop the root the operator actually asked about.
    push(base.to_path_buf(), &mut roots, &mut seen);
    roots
}

/// Walk `git rev-parse --show-superproject-working-tree` up to the outermost
/// working tree. Falls back to `start` when git is unavailable.
fn superproject_root(start: &Path) -> PathBuf {
    let mut current = start.to_path_buf();
    for _ in 0..8 {
        let Some(parent) = git_superproject_of(&current) else {
            break;
        };
        current = parent;
    }
    current
}

fn git_superproject_of(dir: &Path) -> Option<PathBuf> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["rev-parse", "--show-superproject-working-tree"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let path = String::from_utf8(output.stdout).ok()?;
    let path = path.trim();
    if path.is_empty() {
        return None;
    }
    let path = PathBuf::from(path);
    path.is_dir().then_some(path)
}

/// Parse `path = <dir>` entries out of a superproject's `.gitmodules`.
pub(crate) fn submodule_paths(root: &Path) -> Vec<String> {
    let Ok(text) = std::fs::read_to_string(root.join(".gitmodules")) else {
        return Vec::new();
    };
    parse_submodule_paths(&text)
}

fn parse_submodule_paths(gitmodules: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in gitmodules.lines() {
        let line = line.trim();
        let Some(value) = line.strip_prefix("path") else {
            continue;
        };
        let Some(value) = value.trim_start().strip_prefix('=') else {
            continue;
        };
        let value = value.trim();
        if !value.is_empty() {
            paths.push(value.to_string());
        }
    }
    paths
}

fn project_label(superproject: &Path, root: &Path) -> String {
    match root.strip_prefix(superproject) {
        Ok(relative) if !relative.as_os_str().is_empty() => {
            relative.to_string_lossy().replace('\\', "/")
        }
        _ => root
            .file_name()
            .map(|name| name.to_string_lossy().to_string())
            .unwrap_or_else(|| root.display().to_string()),
    }
}

/// Collect one project's rows. `owned` is every project root on the board, so a
/// superproject never absorbs a document that belongs to a submodule section.
fn collect_project_facts(project: &ProjectRoot, owned: &BTreeSet<PathBuf>) -> Vec<DocumentFacts> {
    let actors = load_actor_index(&project.root);
    let mut facts = Vec::new();
    for document in discover_documents(&project.root, owned) {
        let Some(mut row) = cached_document_facts(&document) else {
            continue;
        };
        let relative = document
            .strip_prefix(&project.root)
            .map(|path| path.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| document.display().to_string());
        row.project = project.label.clone();
        row.project_root = project.root.display().to_string();
        row.path = relative;
        if let Some(actor) = actors.get(&document) {
            row.actor_pane = Some(actor.pane.clone());
            row.actor_harness = (!actor.harness.is_empty()).then(|| actor.harness.clone());
            row.actor_alive = actor.alive;
        }
        facts.push(row);
    }
    facts
}

/// Everything the board needs that can be read out of document text alone.
pub(crate) fn document_facts(content: &str) -> DocumentFacts {
    use agent_doc_queue::queue_continuation as continuation;
    use agent_doc_queue::queue_heads;

    let frontmatter_active = agent_doc_frontmatter::frontmatter::parse(content)
        .ok()
        .and_then(|(frontmatter, _)| frontmatter.queue_active);
    DocumentFacts {
        queue_active: frontmatter_active == Some(true)
            || queue_heads::active_queue_prompt(content).is_some(),
        queue_stopped: queue_heads::queue_is_explicitly_stopped(content),
        drainable_heads: continuation::drainable_head_count(content),
        deferred_heads: continuation::deferred_head_count(content),
        backlog_open: open_backlog_item_count(content),
        review_open: continuation::open_review_item_count(content),
        review_gated: continuation::gated_review_ids(content).len(),
        lanes: agent_doc_work_graph::analyze_document(content)
            .map(|dag| LaneCounts::from_dag(&dag))
            .unwrap_or_default(),
        ..DocumentFacts::default()
    }
}

fn open_backlog_item_count(content: &str) -> usize {
    use agent_doc_element_backlog::backlog::{self, PendingState};

    let Ok(components) = agent_doc_element::element::parse(content) else {
        return 0;
    };
    components
        .iter()
        .filter(|component| component.name == "backlog")
        .map(|component| {
            let (_, items, _) = backlog::parse_items(component.content(content));
            items
                .iter()
                .filter(|item| !matches!(item.state, PendingState::Done))
                .count()
        })
        .sum()
}

struct ActorBinding {
    pane: String,
    harness: String,
    alive: bool,
}

/// Index this project's controller actors by the document path they own.
fn load_actor_index(root: &Path) -> BTreeMap<PathBuf, ActorBinding> {
    let Ok(actors) = agent_doc_controller_io::project_controller::load_actor_store(root) else {
        return BTreeMap::new();
    };
    let tmux = agent_doc_tmux_io::configured_tmux();
    let mut index = BTreeMap::new();
    for record in actors.values() {
        let document = PathBuf::from(&record.document_id);
        let document = document.canonicalize().unwrap_or(document);
        index.insert(
            document,
            ActorBinding {
                pane: record.pane_id.clone(),
                harness: record.harness.clone(),
                alive: !record.pane_id.is_empty() && tmux.pane_alive(&record.pane_id),
            },
        );
    }
    index
}

/// Registry entries plus a bounded scan, de-duplicated by canonical path.
fn discover_documents(root: &Path, owned: &BTreeSet<PathBuf>) -> Vec<PathBuf> {
    let mut found: BTreeSet<PathBuf> = BTreeSet::new();
    if let Ok(registry) = agent_doc_session_registry_io::load_in(root) {
        for entry in registry.values() {
            if entry.file.is_empty() {
                continue;
            }
            let path = Path::new(&entry.file);
            let path = if path.is_absolute() {
                path.to_path_buf()
            } else {
                root.join(path)
            };
            if path.is_file() && path.starts_with(root) {
                found.insert(path.canonicalize().unwrap_or(path));
            }
        }
    }
    let mut nested = nested_submodule_roots(root);
    nested.extend(owned.iter().filter(|other| *other != root).cloned());
    scan_session_documents(root, &nested, &mut found);
    found.into_iter().collect()
}

/// Submodule directories of `root`, which are scanned as their own projects.
fn nested_submodule_roots(root: &Path) -> BTreeSet<PathBuf> {
    submodule_paths(root)
        .into_iter()
        .map(|path| root.join(path))
        .map(|path| path.canonicalize().unwrap_or(path))
        .collect()
}

fn scan_session_documents(dir: &Path, nested: &BTreeSet<PathBuf>, out: &mut BTreeSet<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
            continue;
        };
        if path.is_dir() {
            if is_ignored_scan_dir(name) {
                continue;
            }
            let canonical = path.canonicalize().unwrap_or_else(|_| path.clone());
            if nested.contains(&canonical) {
                continue;
            }
            scan_session_documents(&path, nested, out);
        } else if is_session_document(&path, name) {
            out.insert(path.canonicalize().unwrap_or(path));
        }
    }
}

/// True for a markdown file that carries agent-doc session structure. Archive
/// siblings (`*.done.md`) are reaped history, not live sessions.
fn is_session_document(path: &Path, name: &str) -> bool {
    if !name.ends_with(".md") || name.ends_with(".done.md") {
        return false;
    }
    cached_is_session_document(path)
}

/// Per-file memo keyed by `(mtime, len)`, so a long-lived renderer (the
/// controller-owned dashboard projection, `gvqv`) re-reads and re-parses only
/// the documents that changed since its last refresh. A one-shot `board` run
/// sees an empty cache and behaves exactly as before.
#[derive(Default)]
struct CachedDocument {
    stamp: Option<(std::time::SystemTime, u64)>,
    session: Option<bool>,
    facts: Option<DocumentFacts>,
}

fn document_cache() -> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, CachedDocument>>
{
    static CACHE: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, CachedDocument>>,
    > = std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

fn file_stamp(path: &Path) -> Option<(std::time::SystemTime, u64)> {
    let metadata = std::fs::metadata(path).ok()?;
    Some((metadata.modified().ok()?, metadata.len()))
}

/// Run `fill` against the cache entry for `path`, resetting it first when the
/// file changed on disk. Returns `None` when the file cannot be stat'ed.
fn with_cached_document<T>(path: &Path, fill: impl FnOnce(&mut CachedDocument) -> T) -> Option<T> {
    let stamp = file_stamp(path)?;
    let mut cache = document_cache()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = cache.entry(path.to_path_buf()).or_default();
    if entry.stamp != Some(stamp) {
        *entry = CachedDocument {
            stamp: Some(stamp),
            ..CachedDocument::default()
        };
    }
    Some(fill(entry))
}

fn cached_is_session_document(path: &Path) -> bool {
    with_cached_document(path, |entry| {
        if let Some(session) = entry.session {
            return session;
        }
        let session = std::fs::read_to_string(path).is_ok_and(|content| {
            has_session_structure(&content)
                && !agent_doc_frontmatter::dashboard_projection::is_dashboard_projection(&content)
        });
        entry.session = Some(session);
        session
    })
    .unwrap_or(false)
}

fn cached_document_facts(path: &Path) -> Option<DocumentFacts> {
    with_cached_document(path, |entry| {
        if entry.facts.is_none() {
            let content = std::fs::read_to_string(path).ok()?;
            entry.facts = Some(document_facts(&content));
        }
        entry.facts.clone()
    })
    .flatten()
}

pub(crate) fn has_session_structure(content: &str) -> bool {
    content.contains("<!-- agent:exchange")
        || content.contains("<!-- agent:backlog")
        || content.contains("<!-- agent:queue")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_work_graph::fleet_board::DocState;
    use tempfile::TempDir;

    const QUEUED_DOC: &str = concat!(
        "---\nqueue_active: true\n---\n\n",
        "<!-- agent:queue go -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#alpha] accept a leading bare id token in the parser\n",
        "- [ ] [#beta] extend the parser fixture set\n",
        "<!-- /agent:backlog -->\n",
    );

    fn write(path: &Path, content: &str) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, content).unwrap();
    }

    #[test]
    fn parse_submodule_paths_reads_every_path_entry() {
        let paths = parse_submodule_paths(concat!(
            "[submodule \"src/haiven-dev\"]\n",
            "\tpath = src/haiven-dev\n",
            "\turl = git@github.com:btakita/haiven-dev.git\n",
            "[submodule \"src/boost-client\"]\n",
            "\tpath=src/boost-client\n",
            "\turl = git@github.com:btakita/boost-client.git\n",
            "# path = commented/out\n",
        ));
        assert_eq!(paths, vec!["src/haiven-dev", "src/boost-client"]);
    }

    #[test]
    fn project_label_is_the_submodule_path_and_the_dir_name_at_the_root() {
        let superproject = Path::new("/w/agent-loop");
        assert_eq!(
            project_label(superproject, Path::new("/w/agent-loop/src/haiven-dev")),
            "src/haiven-dev"
        );
        assert_eq!(project_label(superproject, superproject), "agent-loop");
    }

    #[test]
    fn board_marks_a_queued_document_with_no_actor_as_stalled() {
        let facts = document_facts(QUEUED_DOC);
        assert!(facts.queue_active);
        assert_eq!(facts.drainable_heads, 1);
        assert_eq!(facts.backlog_open, 2);
        let board = build_board([facts]);
        assert_eq!(board.attention_count, 1);
        assert_eq!(board.groups[0].rows[0].state, DocState::Stalled);
    }

    #[test]
    fn document_facts_reads_a_parked_queue_as_stopped() {
        let content = QUEUED_DOC.replace("queue_active: true", "queue: pause");
        let facts = document_facts(&content);
        assert!(facts.queue_stopped);
        assert!(!facts.queue_active);
    }

    #[test]
    fn board_scan_stops_at_nested_submodule_roots() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        write(&root.join("tasks/own.md"), QUEUED_DOC);
        write(&root.join("sub/child.md"), QUEUED_DOC);
        write(
            &root.join(".gitmodules"),
            "[submodule \"sub\"]\n\tpath = sub\n\turl = git@example.com:sub.git\n",
        );

        let documents = discover_documents(root, &BTreeSet::new());
        assert_eq!(documents.len(), 1, "found {documents:?}");
        assert!(documents[0].ends_with("tasks/own.md"));
    }

    #[test]
    fn scan_skips_archives_ignored_dirs_and_plain_markdown() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        write(&root.join("tasks/live.md"), QUEUED_DOC);
        write(&root.join("tasks/live.done.md"), QUEUED_DOC);
        write(&root.join("README.md"), "# just a readme\n");
        write(&root.join("target/generated.md"), QUEUED_DOC);
        write(&root.join(".agent-doc/snapshots/snap.md"), QUEUED_DOC);

        let documents = discover_documents(root, &BTreeSet::new());
        assert_eq!(documents.len(), 1, "found {documents:?}");
        assert!(documents[0].ends_with("tasks/live.md"));
    }

    #[test]
    fn board_groups_documents_by_submodule_root() {
        let dir = TempDir::new().unwrap();
        let superproject = dir.path();
        let submodule = superproject.join("src/haiven-dev");
        std::fs::create_dir_all(superproject.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(submodule.join(".agent-doc")).unwrap();
        write(&superproject.join("tasks/parent.md"), QUEUED_DOC);
        write(&submodule.join("tasks/child.md"), QUEUED_DOC);

        let projects = [
            ProjectRoot {
                label: project_label(superproject, superproject),
                root: superproject.to_path_buf(),
            },
            ProjectRoot {
                label: project_label(superproject, &submodule),
                root: submodule.clone(),
            },
        ];
        let owned: BTreeSet<PathBuf> = projects
            .iter()
            .map(|project| project.root.clone())
            .collect();
        let facts: Vec<_> = projects
            .iter()
            .flat_map(|project| collect_project_facts(project, &owned))
            .collect();
        let board = build_board(facts);

        assert_eq!(board.document_count, 2);
        let labels: Vec<_> = board
            .groups
            .iter()
            .map(|group| group.project.as_str())
            .collect();
        assert!(labels.contains(&"src/haiven-dev"), "got {labels:?}");
        for group in &board.groups {
            assert_eq!(
                group.rows.len(),
                1,
                "{} leaked a sibling row",
                group.project
            );
        }
    }

    #[test]
    fn a_root_without_agent_doc_state_is_not_a_project_section() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        write(&root.join("tasks/live.md"), QUEUED_DOC);
        let roots = discover_project_roots(root);
        assert!(
            roots.is_empty(),
            "a directory with no .agent-doc/ is not an agent-doc project"
        );
    }

    #[test]
    fn board_scan_never_picks_up_a_dashboard_projection() {
        let dir = TempDir::new().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        write(&root.join("tasks/live.md"), QUEUED_DOC);
        // Even a projection written outside `.agent-doc/` whose rendered rows
        // somehow carried a component marker is not a session document.
        write(
            &root.join("docs/dashboard.md"),
            "<!-- agent-doc-dashboard v1 scope=project all=false -->\n<!-- agent:queue -->\n",
        );
        write(
            &root.join(".agent-doc/dashboard.md"),
            "<!-- agent-doc-dashboard v1 scope=project all=false -->\n",
        );
        let documents = discover_documents(root, &BTreeSet::new());
        assert_eq!(documents.len(), 1, "found {documents:?}");
        assert!(documents[0].ends_with("tasks/live.md"));
    }

    #[test]
    fn cached_document_facts_follow_file_changes() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("tasks/live.md");
        write(&path, QUEUED_DOC);
        assert_eq!(cached_document_facts(&path).unwrap().backlog_open, 2);
        assert!(cached_is_session_document(&path));
        // A different length invalidates the entry even within one mtime tick.
        let fewer = QUEUED_DOC.replace("- [ ] [#beta] extend the parser fixture set\n", "");
        write(&path, &fewer);
        assert_eq!(cached_document_facts(&path).unwrap().backlog_open, 1);
        write(&path, "# plain notes now\n");
        assert!(!cached_is_session_document(&path));
    }

    #[test]
    fn has_session_structure_requires_an_agent_component() {
        assert!(has_session_structure("<!-- agent:queue go -->\n"));
        assert!(has_session_structure("<!-- agent:backlog -->\n"));
        assert!(!has_session_structure("# plain markdown\n"));
    }
}
