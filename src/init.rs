//! # Module: init
//!
//! ## Spec
//! - `run(file, title, agent, mode, artifacts, config)`: dispatches on whether a file path was provided.
//!   - No file: calls `init_project()` — checks prerequisites, creates `.agent-doc/`, and installs
//!     `SKILL.md` for the detected harness (generic `.agent/skills/` when none is detected, in
//!     which case it prints the `skill install --harness` command to run), then prints next
//!     steps ending in the README "Getting started" link. Per-document state is stored in the typed ledger, not a filesystem sidecar.
//!     `--artifact` without a session file is an error and does not initialize a project.
//!   - With file: calls `init_file()` — lazily runs project init if `.agent-doc/` is absent, then
//!     scaffolds a new session document.  Fails if the file already exists.
//!   - With one or more `--artifact PLAN.md`: the session is a template control document whose
//!     `agent:run` names each path and no other work. Each plan is ordinary markdown (no `agent:*`
//!     marker, no `agent_doc_format`, no `## Draft`). A missing artifact is created. An existing
//!     ordinary markdown artifact is left unchanged. An artifact that is already an agent-doc
//!     session is refused. `--mode append` is refused.
//! - `bind_artifacts` / `unbind_artifacts` attach and detach those paths on an existing template
//!   session. Bind appends one `agent:run` row per new path and creates a missing markdown file.
//!   Unbind removes that row and its nested lines and leaves the file on disk. Neither commits.
//! - `init_file` scaffolds two document formats depending on `mode`:
//!   - `"template"` or `"stream"`: frontmatter with `agent_doc_format: template` and
//!     `agent_doc_write: crdt`; body contains a single `<!-- agent:exchange -->` component.
//!   - Any other value (default `"append"`): frontmatter with `agent_doc_session` + `agent` only;
//!     inline `## User` block format.
//! - A UUID v4 is generated for `agent_doc_session` on every scaffolded document.
//! - Parent directories are created with `create_dir_all` if absent.
//! - Agent defaults: `--agent` flag > `config.default_agent` > `"claude"`.
//!
//! ## Agentic Contracts
//! - Returns `Err` if the target file already exists.
//! - Project init is idempotent: re-running when `.agent-doc/` exists is a no-op for directory
//!   creation (via `create_dir_all`).
//! - No git operations are performed during init; the caller is responsible for committing.
//! - `SKILL.md` installation is delegated to `crate::skill::install_and_check_updated`.
//!
//! ## Evals
//! - project_init_next_steps_name_the_harness_install_when_none_was_detected: with no detected
//!   harness, init names `agent-doc skill install --harness <h>` instead of claiming a
//!   `.claude/skills` install; both variants point to the Getting started section.
//! - init_project_creates_dirs: `.agent-doc` is created without a snapshots sidecar directory.
//! - init_file_template_mode: `mode="template"` → scaffolded file contains `agent_doc_format: template` and `agent:exchange` component
//! - init_file_append_mode: `mode="append"` → scaffolded file contains `## User` block, no exchange component
//! - init_file_already_exists: target file exists → returns Err
//! - artifact_session_is_template_aimed_at_one_plain_markdown_file: `--artifact` writes a template
//!   session whose queue names only that file, and the artifact has no agent markers.
//! - artifact_session_keeps_an_existing_ordinary_markdown_file: an existing ordinary artifact is
//!   not rewritten, and a file that is already a session is refused.
//! - artifact_session_names_every_artifact_and_nothing_else: repeated `--artifact` writes one queue
//!   row per file.
//! - bind_appends_an_artifact_and_unbind_leaves_the_file: bind adds a row without dropping other
//!   work; unbind removes that row and does not delete the markdown file.

use anyhow::{Context, Result};
use std::path::{Component, Path, PathBuf};
use uuid::Uuid;

use agent_doc_config::Config;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionPreparation {
    Existing,
    Initialized,
}

pub fn title_from_path(file: &Path) -> String {
    let stem = file
        .file_stem()
        .and_then(|value| value.to_str())
        .unwrap_or("Untitled Session");
    let title = stem.replace(['-', '_'], " ");
    let mut chars = title.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => "Untitled Session".to_string(),
    }
}

pub fn prepare_editor_session(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    config: &Config,
) -> Result<SessionPreparation> {
    prepare_editor_session_with(file, title, agent, config, || {
        agent_doc_project_root_io::ensure_project_root_allowed(&std::env::current_dir()?)?;
        if !Path::new(".agent-doc").exists() {
            init_project()?;
        }
        Ok(())
    })
}

/// [`prepare_editor_session`] with the working-directory project init
/// injected. The init acts on the process CWD, which for a test is its crate
/// checkout, so tests supply their own (`#testisolationleaks`).
fn prepare_editor_session_with(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    config: &Config,
    ensure_project: impl FnOnce() -> Result<()>,
) -> Result<SessionPreparation> {
    let existing = std::fs::read_to_string(file)?;
    if agent_doc_frontmatter::session_id_from_content(&existing).is_some() {
        return Ok(SessionPreparation::Existing);
    }

    ensure_project()?;
    let title = title
        .map(str::to_owned)
        .unwrap_or_else(|| title_from_path(file));
    let agent = agent
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let session_id = Uuid::new_v4();
    let initial = existing.trim_end();
    let initial = if initial.is_empty() {
        String::new()
    } else {
        format!("{initial}\n")
    };
    let components =
        agent_doc_document::claim_scaffold::render_default_template_components(&initial);
    let content = format!(
        "---\nagent_doc_session: {session_id}\nagent: {agent}\nagent_doc_format: template\nagent_doc_write: crdt\nenable_tool_search: true\n---\n\n# {title}{components}"
    );
    std::fs::write(file, content)?;
    eprintln!("Initialized {}", file.display());
    Ok(SessionPreparation::Initialized)
}

/// Initialize a project (no file given): check prereqs, create .agent-doc/, install skill.
fn init_project() -> Result<()> {
    agent_doc_project_root_io::ensure_project_root_allowed(&std::env::current_dir()?)?;
    // Check prerequisites
    crate::install::check_prereqs();

    // Create .agent-doc/ directory structure
    let agent_doc_dir = Path::new(".agent-doc");
    // `#testisolationleaks`: project init writes the CWD project and installs
    // the skill into its root. A test's CWD is its checkout, never a fixture.
    agent_doc_fs::guard_test_runtime_target(agent_doc_dir, "a CWD project init");
    std::fs::create_dir_all(agent_doc_dir)?;
    eprintln!("[init] Created {}", agent_doc_dir.display());

    // Install SKILL.md. The installer prints the real harness and path.
    let generic = crate::skill::install_env_is_generic();
    crate::skill::install_and_check_updated()?;

    for line in project_init_next_steps(generic) {
        eprintln!("{line}");
    }

    Ok(())
}

/// The "what now" lines `agent-doc init` prints after project init.
///
/// `generic` is true when no harness was detected from the calling shell (the
/// usual case for a new user running `init` in a plain terminal), so the skill
/// went to `.agent/skills/`, which no harness reads on its own.
pub(crate) fn project_init_next_steps(generic: bool) -> Vec<String> {
    let mut lines = Vec::new();
    if generic {
        lines.push(
            "[init] No agent harness detected from this shell, so the skill went to .agent/skills/ (no harness reads it automatically).".to_string(),
        );
        lines.push(
            "[init]   Install it for your agent: agent-doc skill install --harness claude   # or: codex, grok, opencode, --all".to_string(),
        );
    }
    lines.push("[init] Project initialized. Next steps:".to_string());
    lines.push(
        "[init]   agent-doc init notes.md \"My Topic\"   # create a session document".to_string(),
    );
    lines.push(
        "[init]   write a request in it, then run a turn: your editor plugin, /agent-doc notes.md in Claude Code, or agent-doc run notes.md".to_string(),
    );
    lines.push("[init]   .agent-doc/ holds local runtime state; add it to .gitignore".to_string());
    lines.push(format!("[init]   Getting started: {GETTING_STARTED_URL}"));
    lines
}

/// Public entry point for a new user, printed by `init` and `--help`.
pub(crate) const GETTING_STARTED_URL: &str = "https://github.com/btakita/agent-doc#getting-started";

/// Scaffold a new session document, lazily initializing the project if needed.
fn init_file(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    mode: Option<&str>,
    artifacts: &[PathBuf],
    config: &Config,
) -> Result<()> {
    agent_doc_project_root_io::ensure_project_root_allowed(&std::env::current_dir()?)?;
    // Lazy project init: if .agent-doc/ doesn't exist, run project init first.
    if !Path::new(".agent-doc").exists() {
        eprintln!("[init] No .agent-doc/ found — running project init first.");
        init_project()?;
    }

    write_new_session(file, title, agent, mode, artifacts, config)
}

/// Write a new session, and when `artifacts` is non-empty, each plain markdown file it names.
///
/// Callers that already decided project init may call this directly. It does not
/// touch `.agent-doc/` or the process working directory.
fn write_new_session(
    file: &Path,
    title: Option<&str>,
    agent: Option<&str>,
    mode: Option<&str>,
    artifacts: &[PathBuf],
    config: &Config,
) -> Result<()> {
    if file.exists() {
        anyhow::bail!("file already exists: {}", file.display());
    }
    let labels = if artifacts.is_empty() {
        Vec::new()
    } else {
        require_artifact_template_mode(mode)?;
        validate_new_artifact_paths(file, artifacts)?
    };
    let title = match (title, artifacts) {
        (Some(title), _) => title.to_string(),
        (None, [only]) => title_from_path(only),
        (None, _) => "Untitled Session".to_string(),
    };
    let agent = agent
        .or(config.default_agent.as_deref())
        .unwrap_or("claude");
    let session_id = Uuid::new_v4();
    let mode = mode.unwrap_or(if artifacts.is_empty() {
        "append"
    } else {
        "template"
    });

    let content = if mode == "template" || mode == "stream" {
        let components = agent_doc_document::claim_scaffold::render_default_template_components("");
        let content = format!(
            "---\nagent_doc_session: {session_id}\nagent: {agent}\nagent_doc_format: template\nagent_doc_write: crdt\nenable_tool_search: true\n---\n\n# {title}{components}"
        );
        if labels.is_empty() {
            content
        } else {
            aim_queue_at_artifacts(&content, &labels)?
        }
    } else {
        format!(
            "---\nagent_doc_session: {session_id}\nagent: {agent}\n---\n\n# Session: {title}\n\n## User\n\n"
        )
    };

    let created = write_artifact_files(artifacts)?;
    if let Err(error) = write_session_file(file, &content) {
        remove_created_artifacts(&created);
        return Err(error);
    }
    for artifact in &created {
        eprintln!("Created artifact {}", artifact.display());
    }
    eprintln!("Created {}", file.display());
    Ok(())
}

fn ensure_existing_artifact_stays_ordinary(artifact: &Path) -> Result<()> {
    let text = std::fs::read_to_string(artifact)?;
    if text.contains("agent_doc_session:")
        || text.contains("agent_doc_format:")
        || text.contains("<!-- agent:")
    {
        anyhow::bail!(
            "artifact is already an agent-doc session: {}",
            artifact.display()
        );
    }
    Ok(())
}

fn require_artifact_template_mode(mode: Option<&str>) -> Result<()> {
    match mode {
        None | Some("template") | Some("stream") => Ok(()),
        Some(other) => anyhow::bail!(
            "an artifact session is a template session; --mode {other} cannot be combined with --artifact"
        ),
    }
}

fn artifact_queue_label(artifact: &Path) -> Result<String> {
    let name = artifact
        .file_name()
        .and_then(|value| value.to_str())
        .unwrap_or("");
    if !name.ends_with(".md") || name == ".md" {
        anyhow::bail!(
            "artifact file must be markdown (.md): {}",
            artifact.display()
        );
    }
    let raw = artifact.to_string_lossy().into_owned();
    if raw.contains('\n') || raw.contains('\r') || raw.contains("<!--") || raw.contains("-->") {
        anyhow::bail!(
            "artifact path must be a single line and must not contain a marker: {}",
            artifact.display()
        );
    }
    let label = raw.strip_prefix("./").map(str::to_owned).unwrap_or(raw);
    if label.is_empty() {
        anyhow::bail!("artifact path is empty");
    }
    Ok(label)
}

fn artifact_markdown(artifact: &Path) -> String {
    format!("# {}\n", title_from_path(artifact))
}

fn validate_new_artifact_paths(session: &Path, artifacts: &[PathBuf]) -> Result<Vec<String>> {
    let mut labels: Vec<String> = Vec::with_capacity(artifacts.len());
    for artifact in artifacts {
        if artifact.exists() {
            ensure_existing_artifact_stays_ordinary(artifact)?;
        }
        if paths_name_the_same_file(session, artifact) {
            anyhow::bail!(
                "artifact path must be a different file from the session: {}",
                artifact.display()
            );
        }
        let label = artifact_queue_label(artifact)?;
        if labels.iter().any(|existing| labels_match(existing, &label)) {
            anyhow::bail!("artifact path is already listed: {}", artifact.display());
        }
        labels.push(label);
    }
    Ok(labels)
}

fn aim_queue_at_artifacts(content: &str, labels: &[String]) -> Result<String> {
    let components = agent_doc_element::element::parse(content)?;
    let queue = components
        .iter()
        .find(|component| component.name == agent_doc_element::element::QUEUE_COMPONENT)
        .ok_or_else(|| anyhow::anyhow!("artifact session scaffold is missing agent:run"))?;
    // The scaffold opener already ends at the following newline, so the first
    // row starts on the next line without a blank line above it.
    Ok(queue.replace_content(content, &artifact_rows(labels)))
}

fn artifact_rows(labels: &[String]) -> String {
    labels
        .iter()
        .map(|label| format!("- [ ] {label}\n"))
        .collect()
}

fn write_artifact_files(artifacts: &[PathBuf]) -> Result<Vec<PathBuf>> {
    let mut created = Vec::new();
    for artifact in artifacts {
        if artifact.exists() {
            continue;
        }
        if let Err(error) = write_artifact_file(artifact) {
            remove_created_artifacts(&created);
            return Err(error);
        }
        created.push(artifact.clone());
    }
    Ok(created)
}

fn write_artifact_file(artifact: &Path) -> Result<()> {
    if let Some(parent) = artifact.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(artifact, artifact_markdown(artifact))?;
    Ok(())
}

fn remove_created_artifacts(created: &[PathBuf]) {
    for artifact in created {
        let _ = std::fs::remove_file(artifact);
    }
}

/// Read and write the control session. `write` receives the bytes that were
/// read and the replacement, so a live editor can compare-and-swap.
pub trait SessionRewrite {
    fn read(&self, session: &Path) -> Result<String>;
    fn write(&self, session: &Path, current: &str, target: &str) -> Result<()>;
}

/// Disk rewrite used by tests. Production bind and unbind go through the live editor path.
#[cfg(test)]
pub struct DiskSessionRewrite;

#[cfg(test)]
impl SessionRewrite for DiskSessionRewrite {
    fn read(&self, session: &Path) -> Result<String> {
        std::fs::read_to_string(session)
            .with_context(|| format!("failed to read {}", session.display()))
    }

    fn write(&self, session: &Path, _current: &str, target: &str) -> Result<()> {
        std::fs::write(session, target)
            .with_context(|| format!("failed to write {}", session.display()))
    }
}

/// Bind artifact paths onto an existing template control session.
///
/// Each path becomes one top-level `agent:run` row. A path that is already
/// bound is left as that one row. A missing markdown file is created. An
/// existing file is not rewritten. Nothing is committed.
pub fn bind_artifacts(
    session: &Path,
    artifacts: &[PathBuf],
    store: &dyn SessionRewrite,
) -> Result<()> {
    if artifacts.is_empty() {
        anyhow::bail!("artifact bind requires an artifact path");
    }
    let current = store.read(session)?;
    require_template_control(&current)?;
    let runs = run_queues(&current)?;
    let mut labels: Vec<String> = Vec::new();
    let mut create = Vec::new();
    for artifact in artifacts {
        if paths_name_the_same_file(session, artifact) {
            anyhow::bail!(
                "artifact path must be a different file from the session: {}",
                artifact.display()
            );
        }
        let label = artifact_queue_label(artifact)?;
        if labels.iter().any(|existing| labels_match(existing, &label)) {
            continue;
        }
        refuse_control_file_as_artifact(artifact)?;
        if !artifact.exists() {
            create.push(artifact.clone());
        }
        labels.push(label);
    }
    let bound = bound_identities(&current, &runs);
    let fresh: Vec<String> = labels
        .iter()
        .filter(|label| !bound.iter().any(|existing| labels_match(existing, label)))
        .cloned()
        .collect();
    let target = if fresh.is_empty() {
        current.clone()
    } else {
        let primary = primary_run_index(&runs);
        map_run_bodies(&current, &runs, |index, body| {
            if index != primary {
                return Ok(body.to_string());
            }
            Ok(append_artifact_rows(body, &fresh))
        })?
    };
    let created = write_artifact_files(&create)?;
    if target != current
        && let Err(error) = store.write(session, &current, &target)
    {
        remove_created_artifacts(&created);
        return Err(error);
    }
    for label in &labels {
        if fresh.iter().any(|fresh| fresh == label) {
            eprintln!("Bound {label}");
        } else {
            eprintln!("already bound {label}");
        }
    }
    for artifact in &created {
        eprintln!("Created artifact {}", artifact.display());
    }
    Ok(())
}

/// Remove artifact paths from an existing template control session.
///
/// The matching top-level `agent:run` row and the nested lines under it are
/// deleted. The markdown file is left in place. Nothing is committed.
pub fn unbind_artifacts(
    session: &Path,
    artifacts: &[PathBuf],
    store: &dyn SessionRewrite,
) -> Result<()> {
    if artifacts.is_empty() {
        anyhow::bail!("artifact unbind requires an artifact path");
    }
    let current = store.read(session)?;
    require_template_control(&current)?;
    let runs = run_queues(&current)?;
    let mut labels: Vec<String> = Vec::new();
    for artifact in artifacts {
        let label = artifact_queue_label(artifact)?;
        if labels.iter().any(|existing| labels_match(existing, &label)) {
            continue;
        }
        labels.push(label);
    }
    let mut removed = Vec::new();
    let target = map_run_bodies(&current, &runs, |_index, body| {
        let (next, gone) = remove_artifact_rows(body, &labels);
        removed.extend(gone);
        Ok(next)
    })?;
    for label in &labels {
        if !removed.iter().any(|gone| labels_match(gone, label)) {
            anyhow::bail!("not bound: {label}");
        }
    }
    if target != current {
        store.write(session, &current, &target)?;
    }
    for label in &labels {
        eprintln!("Unbound {label}");
    }
    Ok(())
}

#[cfg(test)]
pub fn bind_artifacts_on_disk(session: &Path, artifacts: &[PathBuf]) -> Result<()> {
    bind_artifacts(session, artifacts, &DiskSessionRewrite)
}

#[cfg(test)]
pub fn unbind_artifacts_on_disk(session: &Path, artifacts: &[PathBuf]) -> Result<()> {
    unbind_artifacts(session, artifacts, &DiskSessionRewrite)
}

struct LiveSessionRewrite;

impl SessionRewrite for LiveSessionRewrite {
    fn read(&self, session: &Path) -> Result<String> {
        agent_doc_document_realtime_io::try_resolve_current_document_content(
            session,
            "artifact_session",
        )
    }

    fn write(&self, session: &Path, current: &str, target: &str) -> Result<()> {
        agent_doc_write_converge_io::converge_or_disk_write(
            &agent_doc_document_realtime_io::RUNTIME_WRITE_CONVERGENCE_EFFECTS,
            session,
            current,
            target,
            "artifact_session",
        )
    }
}

pub fn bind_artifacts_live(session: &Path, artifacts: &[PathBuf]) -> Result<()> {
    bind_artifacts(session, artifacts, &LiveSessionRewrite)
}

pub fn unbind_artifacts_live(session: &Path, artifacts: &[PathBuf]) -> Result<()> {
    unbind_artifacts(session, artifacts, &LiveSessionRewrite)
}

fn require_template_control(content: &str) -> Result<()> {
    let (frontmatter, _) = agent_doc_frontmatter::frontmatter::parse(content)?;
    if frontmatter.format != Some(agent_doc_frontmatter::AgentDocFormat::Template) {
        anyhow::bail!(
            "binding an artifact requires agent_doc_format: template on the control session"
        );
    }
    Ok(())
}

fn refuse_control_file_as_artifact(artifact: &Path) -> Result<()> {
    if !artifact.exists() {
        return Ok(());
    }
    let text = std::fs::read_to_string(artifact)
        .with_context(|| format!("failed to read {}", artifact.display()))?;
    if text.contains("<!-- agent:") || text.contains("agent_doc_format:") {
        anyhow::bail!(
            "artifact must be ordinary markdown, not a control session: {}",
            artifact.display()
        );
    }
    Ok(())
}

fn run_queues(doc: &str) -> Result<Vec<agent_doc_element::element::Component>> {
    let components = agent_doc_element::element::parse(doc)?;
    let runs: Vec<_> = components
        .into_iter()
        .filter(|component| is_run_queue(doc, component))
        .collect();
    if runs.is_empty() {
        anyhow::bail!("binding an artifact requires agent:run on the control session");
    }
    Ok(runs)
}

fn is_run_queue(doc: &str, component: &agent_doc_element::element::Component) -> bool {
    if component.name != agent_doc_element::element::QUEUE_COMPONENT {
        return false;
    }
    let opener = &doc[component.open_start..component.open_end];
    opener.contains("agent:run") || opener.contains("agent:queue")
}

fn primary_run_index(runs: &[agent_doc_element::element::Component]) -> usize {
    runs.iter()
        .position(|component| component.attrs.get("id").map(String::as_str) == Some("main"))
        .unwrap_or(0)
}

fn map_run_bodies(
    doc: &str,
    runs: &[agent_doc_element::element::Component],
    mut edit: impl FnMut(usize, &str) -> Result<String>,
) -> Result<String> {
    let mut order: Vec<usize> = (0..runs.len()).collect();
    order.sort_by_key(|&index| std::cmp::Reverse(runs[index].open_end));
    let mut next = doc.to_string();
    for index in order {
        let body = runs[index].content(&next).to_string();
        let edited = edit(index, &body)?;
        if edited != body {
            next = runs[index].replace_content(&next, &edited);
        }
    }
    Ok(next)
}

fn bound_identities(doc: &str, runs: &[agent_doc_element::element::Component]) -> Vec<String> {
    runs.iter()
        .flat_map(|component| top_level_identities(component.content(doc)))
        .collect()
}

fn top_level_identities(body: &str) -> Vec<String> {
    body.lines()
        .filter_map(top_level_list_payload)
        .map(artifact_row_identity)
        .filter(|identity| !identity.is_empty())
        .collect()
}

fn append_artifact_rows(body: &str, labels: &[String]) -> String {
    let rows = artifact_rows(labels);
    if body.trim().is_empty() {
        return rows;
    }
    if body.ends_with('\n') {
        format!("{body}{rows}")
    } else {
        format!("{body}\n{rows}")
    }
}

fn remove_artifact_rows(body: &str, labels: &[String]) -> (String, Vec<String>) {
    let lines: Vec<&str> = body.split('\n').collect();
    let mut kept = Vec::with_capacity(lines.len());
    let mut removed = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if let Some(payload) = top_level_list_payload(lines[index]) {
            let identity = artifact_row_identity(payload);
            if labels.iter().any(|label| labels_match(&identity, label)) {
                removed.push(identity);
                index += 1;
                while index < lines.len() && is_row_continuation(lines[index]) {
                    index += 1;
                }
                continue;
            }
        }
        kept.push(lines[index]);
        index += 1;
    }
    (kept.join("\n"), removed)
}

fn top_level_list_payload(line: &str) -> Option<&str> {
    let line = line.trim_end();
    if line.starts_with(' ') || line.starts_with('\t') {
        return None;
    }
    let rest = line.strip_prefix("- ")?;
    Some(strip_checkbox(rest))
}

fn strip_checkbox(text: &str) -> &str {
    let bytes = text.as_bytes();
    if bytes.len() >= 4 && bytes[0] == b'[' && bytes[2] == b']' && bytes[3] == b' ' {
        let mark = bytes[1];
        if matches!(mark, b' ' | b'x' | b'X' | b'/' | b'-' | b'~') {
            return &text[4..];
        }
    }
    text
}

fn is_row_continuation(line: &str) -> bool {
    top_level_list_payload(line).is_none()
}

fn artifact_row_identity(payload: &str) -> String {
    let unstruck = unwrap_strike(payload.trim()).to_string();
    let mut work = agent_doc_element_queue::struck_work_text(&unstruck)
        .trim()
        .to_string();
    loop {
        let next = {
            let (_, without_tree) = agent_doc_element_queue::split_leading_tree_indicators(&work);
            let without_tree = without_tree.trim();
            agent_doc_element_queue::strip_open_count_token(without_tree)
                .map(str::trim)
                .unwrap_or(without_tree)
                .to_string()
        };
        if next == work {
            break;
        }
        work = next;
    }
    let without_id = agent_doc_element_queue::strip_generated_queue_prompt_id_annotation(&work);
    let bare = unwrap_strike(without_id.trim());
    let identity = agent_doc_element_queue::strip_intent_tags(bare);
    normalize_artifact_label(&identity)
}

fn unwrap_strike(text: &str) -> &str {
    text.strip_prefix("~~")
        .and_then(|inner| inner.strip_suffix("~~"))
        .map(str::trim)
        .unwrap_or(text)
}

fn normalize_artifact_label(label: &str) -> String {
    label
        .trim()
        .strip_prefix("./")
        .unwrap_or(label.trim())
        .to_string()
}

fn labels_match(existing: &str, requested: &str) -> bool {
    let existing = normalize_artifact_label(existing);
    let requested = normalize_artifact_label(requested);
    if existing == requested {
        return true;
    }
    paths_name_the_same_file(Path::new(&existing), Path::new(&requested))
}

fn write_session_file(file: &Path, content: &str) -> Result<()> {
    if let Some(parent) = file.parent()
        && !parent.as_os_str().is_empty()
        && !parent.exists()
    {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(file, content)?;
    Ok(())
}

fn paths_name_the_same_file(left: &Path, right: &Path) -> bool {
    fn normalized(path: &Path) -> PathBuf {
        let path = if path.is_absolute() {
            path.to_path_buf()
        } else {
            std::env::current_dir()
                .unwrap_or_else(|_| PathBuf::from("."))
                .join(path)
        };
        let mut out = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir => {
                    if !out.pop() {
                        out.push("..");
                    }
                }
                other => out.push(other.as_os_str()),
            }
        }
        out
    }
    normalized(left) == normalized(right)
}

pub fn run(
    file: Option<&Path>,
    title: Option<&str>,
    agent: Option<&str>,
    mode: Option<&str>,
    artifacts: &[PathBuf],
    config: &Config,
) -> Result<()> {
    if !artifacts.is_empty() && file.is_none() {
        anyhow::bail!("--artifact requires a session document path");
    }
    match file {
        None => init_project(),
        Some(path) => init_file(path, title, agent, mode, artifacts, config),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn project_init_next_steps_name_the_harness_install_when_none_was_detected() {
        let generic = project_init_next_steps(true).join("\n");
        assert!(
            generic.contains("agent-doc skill install --harness claude"),
            "{generic}"
        );
        // Init must never claim a `.claude/skills` install it did not make.
        assert!(!generic.contains("Installed .claude/skills"), "{generic}");

        let detected = project_init_next_steps(false).join("\n");
        assert!(
            !detected.contains("No agent harness detected"),
            "{detected}"
        );
        for lines in [&generic, &detected] {
            assert!(lines.contains("agent-doc init notes.md"), "{lines}");
            assert!(lines.contains(".gitignore"), "{lines}");
            assert!(lines.contains(GETTING_STARTED_URL), "{lines}");
        }
    }

    #[test]
    fn editor_session_uses_filename_title_and_preserves_plain_markdown() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("release-notes.md");
        std::fs::write(&file, "Keep this prompt.\n").unwrap();

        let project_init = std::cell::Cell::new(false);
        let outcome =
            prepare_editor_session_with(&file, None, Some("codex"), &Config::default(), || {
                project_init.set(true);
                Ok(())
            })
            .unwrap();
        let content = std::fs::read_to_string(&file).unwrap();

        assert_eq!(outcome, SessionPreparation::Initialized);
        assert!(
            project_init.get(),
            "a new session ensures its project first"
        );
        assert!(content.contains("# Release notes"));
        assert!(content.contains("agent_doc_format: template"));
        let components = agent_doc_element::element::parse(&content).unwrap();
        assert_eq!(
            components
                .iter()
                .map(|component| component.name.as_str())
                .collect::<Vec<_>>(),
            [
                "status",
                "worktrees",
                "recap",
                "active",
                "queue",
                "review",
                "todo",
                "backlog",
                "icebox",
                "notes",
                "done",
                "exchange",
            ]
        );
        let exchange = components.last().unwrap();
        assert_eq!(exchange.content(&content), "Keep this prompt.\n");
    }

    #[test]
    fn editor_session_leaves_existing_session_unchanged() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("existing.md");
        let content = "---\nagent_doc_session: existing\n---\nBody\n";
        std::fs::write(&file, content).unwrap();

        let outcome = prepare_editor_session_with(&file, None, None, &Config::default(), || {
            panic!("an existing session must not initialize a project")
        })
        .unwrap();

        assert_eq!(outcome, SessionPreparation::Existing);
        assert_eq!(std::fs::read_to_string(&file).unwrap(), content);
    }

    #[test]
    fn artifact_session_is_template_aimed_at_one_plain_markdown_file() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("sessions").join("plan-session.md");
        let artifact = dir.path().join("plans").join("my-plan.md");

        write_new_session(
            &session,
            None,
            Some("claude"),
            None,
            std::slice::from_ref(&artifact),
            &Config::default(),
        )
        .unwrap();

        let session_text = std::fs::read_to_string(&session).unwrap();
        let artifact_text = std::fs::read_to_string(&artifact).unwrap();
        let label = artifact.display().to_string();
        assert_eq!(
            artifact_queue_label(Path::new("plans/my-plan.md")).unwrap(),
            "plans/my-plan.md"
        );
        assert_eq!(
            artifact_queue_label(Path::new("./plans/my-plan.md")).unwrap(),
            "plans/my-plan.md"
        );

        assert!(session_text.contains("agent_doc_format: template"));
        assert!(session_text.contains("<!-- agent:run"));
        assert!(session_text.contains("<!-- agent:exchange"));
        assert!(!session_text.contains("agent:artifact"));
        assert!(session_text.contains("# My plan"));

        let components = agent_doc_element::element::parse(&session_text).unwrap();
        assert!(
            components
                .iter()
                .any(|component| component.name == "exchange"),
            "session keeps agent:exchange"
        );
        for component in &components {
            let body = component.content(&session_text);
            if component.name == agent_doc_element::element::QUEUE_COMPONENT {
                assert_eq!(body, format!("- [ ] {label}\n"));
                assert_eq!(body.matches("- [ ]").count(), 1);
            } else {
                assert_eq!(
                    body.matches("- [ ]").count(),
                    0,
                    "{} must not name other work",
                    component.name
                );
            }
        }

        assert!(artifact_text.starts_with("# My plan\n"));
        assert!(!artifact_text.contains("<!-- agent:"));
        assert!(!artifact_text.contains("agent_doc_format"));
        assert!(!artifact_text.contains("## Draft"));
        assert!(!artifact_text.contains("agent:run"));
        assert!(!artifact_text.contains("agent:exchange"));
        assert!(!artifact_text.contains("agent:artifact"));
    }

    #[test]
    fn artifact_session_refuses_append_mode_and_a_missing_session_file() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session.md");
        let artifact = dir.path().join("plan.md");
        let error = write_new_session(
            &session,
            None,
            None,
            Some("append"),
            std::slice::from_ref(&artifact),
            &Config::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("template session"), "{error}");
        assert!(!session.exists());
        assert!(!artifact.exists());

        let error = run(
            None,
            None,
            None,
            None,
            &[PathBuf::from("plan.md")],
            &Config::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("--artifact"), "{error}");
    }

    #[test]
    fn artifact_session_keeps_an_existing_ordinary_markdown_file() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session.md");
        let artifact = dir.path().join("plan.md");
        std::fs::write(&artifact, "already\n").unwrap();
        write_new_session(
            &session,
            None,
            None,
            Some("template"),
            std::slice::from_ref(&artifact),
            &Config::default(),
        )
        .unwrap();
        assert_eq!(std::fs::read_to_string(&artifact).unwrap(), "already\n");
        let session_text = std::fs::read_to_string(&session).unwrap();
        assert!(session_text.contains("agent_doc_format: template"));
        assert!(session_text.contains(&artifact.display().to_string()));
        assert!(!session_text.contains("agent:artifact"));

        let session_file = dir.path().join("taken.md");
        std::fs::write(&artifact, "<!-- agent:run -->\nalready\n").unwrap();
        let error = write_new_session(
            &session_file,
            None,
            None,
            Some("template"),
            std::slice::from_ref(&artifact),
            &Config::default(),
        )
        .unwrap_err();
        assert!(
            error.to_string().contains("already an agent-doc session"),
            "{error}"
        );
        assert!(!session_file.exists());
        assert!(
            std::fs::read_to_string(&artifact)
                .unwrap()
                .contains("already\n")
        );
    }

    #[test]
    fn artifact_session_refuses_a_non_markdown_artifact() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session.md");
        let notes = dir.path().join("notes.txt");
        let error = write_new_session(
            &session,
            None,
            None,
            None,
            std::slice::from_ref(&notes),
            &Config::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains(".md"), "{error}");
        assert!(!session.exists());
        assert!(!notes.exists());
    }

    #[test]
    fn artifact_session_names_every_artifact_and_nothing_else() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session.md");
        let first = dir.path().join("plans").join("one.md");
        let second = dir.path().join("plans").join("two.md");
        let duplicate = dir.path().join("plans").join("one.md");

        write_new_session(
            &session,
            None,
            Some("claude"),
            None,
            &[first.clone(), second.clone()],
            &Config::default(),
        )
        .unwrap();

        let session_text = std::fs::read_to_string(&session).unwrap();
        assert!(session_text.contains("agent_doc_format: template"));
        assert!(session_text.contains("# Untitled Session"));
        assert!(!session_text.contains("agent:artifact"));
        let components = agent_doc_element::element::parse(&session_text).unwrap();
        for component in &components {
            let body = component.content(&session_text);
            if component.name == agent_doc_element::element::QUEUE_COMPONENT {
                assert_eq!(
                    body,
                    format!("- [ ] {}\n- [ ] {}\n", first.display(), second.display())
                );
            } else {
                assert_eq!(body.matches("- [ ]").count(), 0, "{}", component.name);
            }
        }
        for artifact in [&first, &second] {
            let text = std::fs::read_to_string(artifact).unwrap();
            assert!(text.starts_with("# "));
            assert!(!text.contains("<!-- agent:"));
            assert!(!text.contains("agent_doc_format"));
            assert!(!text.contains("## Draft"));
        }

        let kept = std::fs::read_to_string(&first).unwrap();
        let again = dir.path().join("other-session.md");
        let error = write_new_session(
            &again,
            None,
            None,
            None,
            &[duplicate.clone(), duplicate],
            &Config::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("already listed"), "{error}");
        assert!(!again.exists());
        assert_eq!(std::fs::read_to_string(&first).unwrap(), kept);

        let repeated = dir.path().join("plans").join("three.md");
        let error = write_new_session(
            &again,
            None,
            None,
            None,
            &[repeated.clone(), repeated],
            &Config::default(),
        )
        .unwrap_err();
        assert!(error.to_string().contains("already listed"), "{error}");
        assert!(!again.exists());
        assert!(!dir.path().join("plans").join("three.md").exists());
    }

    #[test]
    fn bind_appends_an_artifact_and_unbind_leaves_the_file() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session.md");
        let first = dir.path().join("plans").join("one.md");
        let second = dir.path().join("specs").join("two.md");
        write_new_session(
            &session,
            Some("Control"),
            Some("claude"),
            None,
            std::slice::from_ref(&first),
            &Config::default(),
        )
        .unwrap();
        let first_bytes = std::fs::read_to_string(&first).unwrap();

        bind_artifacts_on_disk(&session, std::slice::from_ref(&second)).unwrap();
        let bound = std::fs::read_to_string(&session).unwrap();
        let queue = queue_body(&bound);
        assert!(queue.contains(&format!("- [ ] {}", first.display())));
        assert!(queue.contains(&format!("- [ ] {}", second.display())));
        assert_eq!(queue.matches("- [ ]").count(), 2);
        assert!(
            std::fs::read_to_string(&second)
                .unwrap()
                .starts_with("# Two\n")
        );
        assert!(!bound.contains("agent:artifact"));

        let existing = dir.path().join("specs").join("kept.md");
        std::fs::write(&existing, "Leave this byte.\n").unwrap();
        bind_artifacts_on_disk(&session, std::slice::from_ref(&existing)).unwrap();
        assert_eq!(
            std::fs::read_to_string(&existing).unwrap(),
            "Leave this byte.\n"
        );

        let control = dir.path().join("specs").join("other-session.md");
        std::fs::write(&control, "---\nagent_doc_format: template\n---\n").unwrap();
        let error = bind_artifacts_on_disk(&session, std::slice::from_ref(&control)).unwrap_err();
        assert!(error.to_string().contains("ordinary markdown"), "{error}");

        bind_artifacts_on_disk(&session, std::slice::from_ref(&second)).unwrap();
        assert_eq!(
            queue_body(&std::fs::read_to_string(&session).unwrap())
                .matches(&format!("- [ ] {}", second.display()))
                .count(),
            1
        );

        unbind_artifacts_on_disk(&session, std::slice::from_ref(&first)).unwrap();
        let after = std::fs::read_to_string(&session).unwrap();
        let queue = queue_body(&after);
        assert!(!queue.contains(&first.display().to_string()));
        assert!(queue.contains(&format!("- [ ] {}", second.display())));
        assert!(queue.contains(&format!("- [ ] {}", existing.display())));
        assert_eq!(std::fs::read_to_string(&first).unwrap(), first_bytes);
        assert!(first.exists());

        let missing = dir.path().join("plans").join("absent.md");
        let error = unbind_artifacts_on_disk(&session, std::slice::from_ref(&missing)).unwrap_err();
        assert!(error.to_string().contains("not bound"), "{error}");
        assert!(!missing.exists());
    }

    #[test]
    fn unbind_removes_a_decorated_row_and_its_nested_lines_only() {
        let dir = tempfile::tempdir().unwrap();
        let session = dir.path().join("session.md");
        let artifact = dir.path().join("plans").join("one.md");
        write_new_session(
            &session,
            Some("Control"),
            Some("claude"),
            None,
            std::slice::from_ref(&artifact),
            &Config::default(),
        )
        .unwrap();
        let label = artifact.display().to_string();
        let mut text = std::fs::read_to_string(&session).unwrap();
        let plain = format!("- [ ] {label}\n");
        let decorated = format!(
            "- 🚧 [#subagent:action-aaaa#1] {label} [#q-e9d00e286b6c]\n  - nested note\n- [ ] keep this other work\n"
        );
        assert!(text.contains(&plain), "{text}");
        text = text.replacen(&plain, &decorated, 1);
        std::fs::write(&session, &text).unwrap();

        unbind_artifacts_on_disk(&session, std::slice::from_ref(&artifact)).unwrap();
        let queue = queue_body(&std::fs::read_to_string(&session).unwrap());
        assert_eq!(queue, "- [ ] keep this other work\n");
        assert!(artifact.exists());
    }

    fn queue_body(session: &str) -> String {
        let components = agent_doc_element::element::parse(session).unwrap();
        components
            .iter()
            .find(|component| component.name == agent_doc_element::element::QUEUE_COMPONENT)
            .unwrap()
            .content(session)
            .to_string()
    }
}
