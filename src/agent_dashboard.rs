//! # Module: agent_dashboard
//!
//! ## Spec
//! - `agent-doc dashboard [ROOT] [--json] [--write [PATH]] [--watch]
//!   [--interval-ms N] [--all] [--no-submodules]` renders the Agent Doc
//!   dashboard (`gvqv`): the fleet work board (`agent-doc board`: where the work
//!   is and what is not moving) composed with controller/supervisor liveness
//!   (`agent-doc admin dashboard`: which controllers and actors are alive), as
//!   one markdown view.
//! - Without `--write` the markdown (or `--json` model) goes to stdout.
//! - `--write` writes the projection atomically to PATH, default
//!   `.agent-doc/dashboard.md` under the project root. Writing the default path
//!   arms the project controller, which then keeps the file current: it
//!   re-renders after a debounced controller state change and on a slow poll
//!   (`agent_doc_controller_io::dashboard_refresh`). `--watch` keeps this
//!   process re-rendering instead (for a custom PATH or with no controller).
//! - The first line of the projection records its render parameters
//!   (`scope=fleet|project`, `all=`), so the controller re-renders exactly what
//!   the operator asked for.
//!
//! ## Agentic Contracts
//! - The projection is never a session document: no frontmatter, no
//!   `<!-- agent:` component marker, and no `agent_doc_*` token anywhere in the
//!   rendered text (editor adapters classify session documents by those
//!   substrings). Every classifier also rejects its first-line marker.
//! - Writes are atomic (temp file + rename) and skipped when the rendered body
//!   is unchanged, so an open editor reloads only on a real state change.
//! - `--write` refuses to replace an existing file that is not a dashboard
//!   projection.
//! - Read-only over sessions: never writes a document, claims a pane, or
//!   touches controller state.
//!
//! ## Evals
//! - `render_composes_board_and_controller_liveness`
//! - `render_never_emits_session_document_tokens`
//! - `write_projection_is_atomic_and_skips_unchanged_bodies`
//! - `write_projection_refuses_to_replace_a_session_document`
//! - `refresh_if_armed_honours_the_recorded_scope`

use anyhow::{Context, Result, bail};
use serde::Serialize;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use agent_doc_controller::fleet::DashboardModel;
use agent_doc_frontmatter::dashboard_projection::{
    DASHBOARD_DEFAULT_RELATIVE_PATH, DASHBOARD_MARKER_PREFIX, is_dashboard_projection,
};
use agent_doc_work_graph::fleet_board::{
    BoardRow, FleetBoard, actor_cell, queue_cell, review_cell,
};

use crate::fleet_board_cmd::{self, ProjectRoot};

/// Stable contract version for `--json` consumers.
pub(crate) const DASHBOARD_CONTRACT_VERSION: &str = "agent-doc-dashboard-v1";

/// Default refresh interval for `--watch`.
pub(crate) const DEFAULT_WATCH_INTERVAL_MS: u64 = 2000;

/// Line that carries the last-change time. Excluded from the unchanged-body
/// comparison so a refresh with no state change writes nothing.
const STAMP_PREFIX: &str = "_Last change: ";

/// Substrings that make an editor adapter or the controller classify a file as
/// a session document. The rendered projection must contain none of them.
const SESSION_TOKENS: [&str; 4] = [
    "<!-- agent:",
    "agent_doc_session",
    "agent_doc_format",
    "agent_doc_write",
];

/// Which projects the dashboard covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DashboardScope {
    /// Just the resolved project root.
    Project,
    /// The outermost superproject and every submodule with `.agent-doc/`.
    Fleet,
}

impl DashboardScope {
    fn as_str(self) -> &'static str {
        match self {
            Self::Project => "project",
            Self::Fleet => "fleet",
        }
    }
}

/// Render parameters, recorded on the projection's first line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DashboardParams {
    pub scope: DashboardScope,
    pub include_clear: bool,
}

impl DashboardParams {
    fn marker_line(self) -> String {
        format!(
            "{DASHBOARD_MARKER_PREFIX}v1 scope={} all={} -->",
            self.scope.as_str(),
            self.include_clear
        )
    }

    /// Parse the first line of an existing projection; `None` when it is not one.
    fn from_projection(content: &str) -> Option<Self> {
        if !is_dashboard_projection(content) {
            return None;
        }
        let first = content.lines().next().unwrap_or_default();
        let mut params = Self {
            scope: DashboardScope::Project,
            include_clear: false,
        };
        for token in first.split_whitespace() {
            match token {
                "scope=fleet" => params.scope = DashboardScope::Fleet,
                "scope=project" => params.scope = DashboardScope::Project,
                "all=true" => params.include_clear = true,
                _ => {}
            }
        }
        Some(params)
    }
}

/// Controller and actor liveness for one project section.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct ProjectControllers {
    pub project: String,
    pub project_root: String,
    /// Live `agent-doc controller serve` processes for this root.
    pub controller_pids: Vec<u32>,
    pub fleet: Option<DashboardModel>,
    /// Why `fleet` could not be read, when it could not.
    pub error: Option<String>,
}

/// The whole Agent Doc dashboard model.
#[derive(Debug, Clone, Serialize)]
pub(crate) struct AgentDashboard {
    pub contract_version: &'static str,
    pub scope: DashboardScope,
    pub include_clear: bool,
    pub board: FleetBoard,
    pub controllers: Vec<ProjectControllers>,
}

/// Options for `agent-doc dashboard`.
pub(crate) struct DashboardOptions {
    pub root: Option<PathBuf>,
    pub json: bool,
    /// `Some(None)`: write the default projection path.
    pub write: Option<Option<PathBuf>>,
    pub watch: bool,
    pub interval_ms: u64,
    pub all: bool,
    pub no_submodules: bool,
}

pub(crate) fn run_command(options: DashboardOptions) -> Result<()> {
    let base = resolve_base_root(options.root.as_deref())?;
    let params = DashboardParams {
        scope: if options.no_submodules {
            DashboardScope::Project
        } else {
            DashboardScope::Fleet
        },
        include_clear: options.all,
    };

    let Some(write) = options.write else {
        if options.watch {
            bail!("`agent-doc dashboard --watch` needs --write [PATH]");
        }
        let dashboard = collect(&base, params);
        if options.json {
            println!("{}", serde_json::to_string_pretty(&dashboard)?);
        } else {
            print!(
                "{}",
                render_markdown(&dashboard, params, None, &now_stamp())
            );
        }
        return Ok(());
    };

    let path = match write {
        Some(path) if path.is_absolute() => path,
        Some(path) => std::env::current_dir()
            .context("failed to read current directory")?
            .join(path),
        None => base.join(DASHBOARD_DEFAULT_RELATIVE_PATH),
    };
    let interval = Duration::from_millis(options.interval_ms.max(250));
    loop {
        let dashboard = collect(&base, params);
        let changed = write_projection(&path, &dashboard, params)?;
        if !options.watch {
            println!("{}", path.display());
            if path == base.join(DASHBOARD_DEFAULT_RELATIVE_PATH) {
                eprintln!("{}", live_update_hint(&base));
            } else {
                eprintln!(
                    "[dashboard] custom path: run with --watch to keep it updated (the controller only refreshes {DASHBOARD_DEFAULT_RELATIVE_PATH})"
                );
            }
            return Ok(());
        }
        if changed {
            eprintln!("[dashboard] updated {}", path.display());
        }
        std::thread::sleep(interval);
    }
}

fn live_update_hint(base: &Path) -> String {
    if agent_doc_controller_io::process::project_controller_pids(base).is_empty() {
        "[dashboard] no project controller is running for this root; the file updates once one starts (or run with --watch)".to_string()
    } else {
        "[dashboard] the project controller keeps this file updated; delete it to stop".to_string()
    }
}

/// Controller-owned refresh entry point (`ProjectControllerRuntimeEffects`).
/// Re-renders the armed default projection with its recorded parameters;
/// returns `Ok(false)` without rendering when the projection is absent.
pub(crate) fn refresh_if_armed(project_root: &Path) -> Result<bool> {
    let path = project_root.join(DASHBOARD_DEFAULT_RELATIVE_PATH);
    let Some(existing) = agent_doc_fs::read_optional_text(&path)? else {
        return Ok(false);
    };
    let Some(params) = DashboardParams::from_projection(&existing) else {
        // Never overwrite something that is not ours.
        return Ok(false);
    };
    let dashboard = collect(project_root, params);
    write_projection(&path, &dashboard, params)
}

fn resolve_base_root(root: Option<&Path>) -> Result<PathBuf> {
    if let Some(root) = root {
        return Ok(root.canonicalize().unwrap_or_else(|_| root.to_path_buf()));
    }
    let cwd = std::env::current_dir().context("failed to read current directory")?;
    Ok(agent_doc_fs::find_project_root(&cwd).unwrap_or(cwd))
}

/// Read the board and every covered project's controller liveness.
pub(crate) fn collect(base: &Path, params: DashboardParams) -> AgentDashboard {
    let roots = fleet_board_cmd::project_roots(base, params.scope == DashboardScope::Project);
    let board = fleet_board_cmd::board_for_projects(&roots, params.include_clear);
    let running = running_controllers();
    let controllers = roots
        .iter()
        .map(|project| project_controllers(project, &running))
        .collect();
    AgentDashboard {
        contract_version: DASHBOARD_CONTRACT_VERSION,
        scope: params.scope,
        include_clear: params.include_clear,
        board,
        controllers,
    }
}

/// One `/proc` scan: canonical project root -> live controller pids.
fn running_controllers() -> BTreeMap<PathBuf, Vec<u32>> {
    let mut running: BTreeMap<PathBuf, Vec<u32>> = BTreeMap::new();
    for pid in agent_doc_controller_io::process::process_pids() {
        if let Some(root) = agent_doc_controller_io::process::controller_serve_project_root(pid) {
            let root =
                agent_doc_controller::command_line::canonical_path_for_command_line_compare(&root);
            running.entry(root).or_default().push(pid);
        }
    }
    running
}

fn project_controllers(
    project: &ProjectRoot,
    running: &BTreeMap<PathBuf, Vec<u32>>,
) -> ProjectControllers {
    let key =
        agent_doc_controller::command_line::canonical_path_for_command_line_compare(&project.root);
    let (fleet, error) = match crate::dashboard_cmd::fleet_model_without_diagnostics(&project.root)
    {
        Ok(model) => (Some(model), None),
        Err(error) => (None, Some(format!("{error:#}"))),
    };
    ProjectControllers {
        project: project.label.clone(),
        project_root: project.root.display().to_string(),
        controller_pids: running.get(&key).cloned().unwrap_or_default(),
        fleet,
        error,
    }
}

fn now_stamp() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or_default();
    format!("{} UTC", agent_doc_log_time::format_human_timestamp(secs))
}

/// Render the dashboard as markdown. `link_base` is the directory the file
/// will live in; document cells become relative links from there.
pub(crate) fn render_markdown(
    dashboard: &AgentDashboard,
    params: DashboardParams,
    link_base: Option<&Path>,
    stamp: &str,
) -> String {
    let mut out = String::new();
    out.push_str(&params.marker_line());
    out.push('\n');
    out.push_str("# Agent Doc dashboard\n\n");
    out.push_str(&format!("{STAMP_PREFIX}{}_\n\n", text(stamp)));

    let actor_count: usize = dashboard
        .controllers
        .iter()
        .filter_map(|project| project.fleet.as_ref())
        .map(|fleet| fleet.rows.len())
        .sum();
    let flagged: usize = dashboard
        .controllers
        .iter()
        .filter_map(|project| project.fleet.as_ref())
        .map(|fleet| fleet.problem_count)
        .sum();
    let live_controllers = dashboard
        .controllers
        .iter()
        .filter(|project| !project.controller_pids.is_empty())
        .count();
    out.push_str(&format!(
        "**{}** of {} document(s) need attention · {} of {} project controller(s) running · {} actor(s), {} flagged · scope: {}\n\n",
        dashboard.board.attention_count,
        dashboard.board.document_count,
        live_controllers,
        dashboard.controllers.len(),
        actor_count,
        flagged,
        params.scope.as_str(),
    ));
    out.push_str(
        "Generated by `agent-doc dashboard`; the project controller rewrites this file when session, queue, or controller state changes. Do not edit it.\n",
    );

    render_attention(&mut out, dashboard, link_base);
    render_board(&mut out, dashboard, link_base);
    render_controllers(&mut out, dashboard, link_base);
    neutralize_session_tokens(out)
}

fn render_attention(out: &mut String, dashboard: &AgentDashboard, link_base: Option<&Path>) {
    out.push_str("\n## Needs attention\n\n");
    let rows: Vec<&BoardRow> = dashboard
        .board
        .groups
        .iter()
        .flat_map(|group| group.rows.iter())
        .filter(|row| row.state.needs_attention())
        .collect();
    if rows.is_empty() {
        out.push_str("Nothing needs a person right now.\n");
        return;
    }
    for row in rows {
        out.push_str(&format!(
            "- **{}** {} ({}): {}\n",
            row.state.as_str(),
            document_link(row, link_base),
            text(&row.facts.project),
            text(row.reason.as_deref().unwrap_or("needs a look")),
        ));
    }
}

fn render_board(out: &mut String, dashboard: &AgentDashboard, link_base: Option<&Path>) {
    out.push_str("\n## Work board\n");
    if dashboard.board.groups.is_empty() {
        out.push_str("\nNo session documents with queued or open work.\n");
        return;
    }
    for group in &dashboard.board.groups {
        out.push_str(&format!(
            "\n### {} · {} of {} need attention\n\n",
            text(&group.project),
            group.attention_count,
            group.rows.len()
        ));
        out.push_str(
            "| State | Document | Queue | Backlog | Review | Auto-DAG | Actor | Needs |\n",
        );
        out.push_str("|---|---|---|---|---|---|---|---|\n");
        for row in &group.rows {
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                row.state.as_str(),
                document_link(row, link_base),
                cell(&queue_cell(&row.facts)),
                row.facts.backlog_open,
                cell(&review_cell(&row.facts)),
                cell(&row.facts.lanes.cell()),
                cell(&actor_cell(&row.facts)),
                cell(row.reason.as_deref().unwrap_or("—")),
            ));
        }
    }
}

fn render_controllers(out: &mut String, dashboard: &AgentDashboard, link_base: Option<&Path>) {
    out.push_str("\n## Controllers and supervisors\n");
    for project in &dashboard.controllers {
        let controller = if project.controller_pids.is_empty() {
            "no controller running".to_string()
        } else {
            format!(
                "controller pid {}",
                project
                    .controller_pids
                    .iter()
                    .map(u32::to_string)
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        };
        out.push_str(&format!(
            "\n### {} · {}\n\n",
            text(&project.project),
            controller
        ));
        let Some(fleet) = &project.fleet else {
            out.push_str(&format!(
                "Actor state unavailable: {}\n",
                text(project.error.as_deref().unwrap_or("unknown error"))
            ));
            continue;
        };
        if fleet.rows.is_empty() {
            out.push_str("No registered actors.\n");
            continue;
        }
        out.push_str("| | Document | Harness | Pane | Alive | State | Supervisor | Flags |\n");
        out.push_str("|---|---|---|---|---|---|---|---|\n");
        for row in &fleet.rows {
            let document =
                actor_document_link(&project.project_root, &row.actor.document_id, link_base);
            out.push_str(&format!(
                "| {} | {} | {} | {} | {} | {} | {} | {} |\n",
                if row.problem { "⚠" } else { "" },
                document,
                cell(&row.actor.harness),
                cell(&row.actor.pane),
                if row.actor.pane_alive {
                    "alive"
                } else {
                    "dead"
                },
                cell(&format!("{} g{}", row.actor.state, row.actor.generation)),
                row.actor
                    .supervisor_pid
                    .map(|pid| format!("pid {pid}"))
                    .unwrap_or_else(|| "—".to_string()),
                cell(&if row.highlight_kinds.is_empty() {
                    "—".to_string()
                } else {
                    row.highlight_kinds.join(", ")
                }),
            ));
        }
        if !fleet.findings.is_empty() {
            out.push_str("\nFindings:\n\n");
            for finding in &fleet.findings {
                out.push_str(&format!(
                    "- [{}] {}\n",
                    text(&finding.kind),
                    text(&finding.detail)
                ));
            }
        }
    }
}

fn document_link(row: &BoardRow, link_base: Option<&Path>) -> String {
    let target = Path::new(&row.facts.project_root).join(&row.facts.path);
    link(&row.facts.path, &target, link_base)
}

fn actor_document_link(project_root: &str, document_id: &str, link_base: Option<&Path>) -> String {
    let path = Path::new(document_id);
    let target = if path.is_absolute() {
        path.to_path_buf()
    } else {
        Path::new(project_root).join(path)
    };
    let label = target
        .strip_prefix(project_root)
        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
        .unwrap_or_else(|_| document_id.to_string());
    link(&label, &target, link_base)
}

fn link(label: &str, target: &Path, link_base: Option<&Path>) -> String {
    let Some(base) = link_base else {
        return cell(label);
    };
    let relative = relative_path(base, target);
    // Percent-encode what would break the link or read as a session token.
    let mut href = relative
        .to_string_lossy()
        .replace('\\', "/")
        .replace('%', "%25")
        .replace(' ', "%20")
        .replace('(', "%28")
        .replace(')', "%29")
        .replace('|', "%7C");
    if href.contains("agent_doc_") {
        href = href.replace('_', "%5F");
    }
    format!("[{}]({href})", cell(label))
}

/// Lexical relative path from directory `base` to `target` (both absolute).
fn relative_path(base: &Path, target: &Path) -> PathBuf {
    let base: Vec<Component<'_>> = base.components().collect();
    let target_components: Vec<Component<'_>> = target.components().collect();
    let common = base
        .iter()
        .zip(target_components.iter())
        .take_while(|(a, b)| a == b)
        .count();
    if common == 0 {
        return target.to_path_buf();
    }
    let mut relative = PathBuf::new();
    for _ in common..base.len() {
        relative.push("..");
    }
    for component in &target_components[common..] {
        relative.push(component.as_os_str());
    }
    relative
}

/// Escape free text for a markdown table cell.
fn cell(value: &str) -> String {
    text(value).replace('|', "\\|")
}

/// Escape free text so it renders literally and cannot inject markup.
fn text(value: &str) -> String {
    value
        .replace(['\r', '\n'], " ")
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

/// Final guard: no session-classification token survives into the projection.
fn neutralize_session_tokens(mut out: String) -> String {
    out = out.replace("agent_doc_", "agent\\_doc\\_");
    for token in SESSION_TOKENS {
        if out.contains(token) {
            out = out.replace(token, &token.replace('<', "&lt;"));
        }
    }
    out
}

/// Strip the last-change stamp so two renders of the same state compare equal.
fn body_without_stamp(content: &str) -> String {
    content
        .lines()
        .filter(|line| !line.starts_with(STAMP_PREFIX))
        .collect::<Vec<_>>()
        .join("\n")
}

/// Atomically write the rendered projection to `path`. Returns `Ok(false)` and
/// writes nothing when the body is unchanged.
pub(crate) fn write_projection(
    path: &Path,
    dashboard: &AgentDashboard,
    params: DashboardParams,
) -> Result<bool> {
    let link_base = path.parent().map(|parent| {
        parent
            .canonicalize()
            .unwrap_or_else(|_| parent.to_path_buf())
    });
    let rendered = render_markdown(dashboard, params, link_base.as_deref(), &now_stamp());
    write_rendered(path, &rendered)
}

fn write_rendered(path: &Path, rendered: &str) -> Result<bool> {
    if let Some(existing) = agent_doc_fs::read_optional_text(path)? {
        if !existing.trim().is_empty() && !is_dashboard_projection(&existing) {
            bail!(
                "refusing to overwrite {}: it is not an agent-doc dashboard projection",
                path.display()
            );
        }
        if body_without_stamp(&existing) == body_without_stamp(rendered) {
            return Ok(false);
        }
    }
    agent_doc_fs::write_atomic(path, rendered.as_bytes())
        .with_context(|| format!("failed to write dashboard {}", path.display()))?;
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_controller::fleet::{AdminActor, AdminFinding, build_dashboard_model};
    use agent_doc_work_graph::fleet_board::{DocumentFacts, build_board};
    use tempfile::TempDir;

    const PARAMS: DashboardParams = DashboardParams {
        scope: DashboardScope::Project,
        include_clear: false,
    };

    fn sample_dashboard(root: &str, document: &str) -> AgentDashboard {
        let facts = DocumentFacts {
            project: "agent-loop".to_string(),
            project_root: root.to_string(),
            path: document.to_string(),
            queue_active: true,
            drainable_heads: 2,
            backlog_open: 3,
            ..DocumentFacts::default()
        };
        let actor = AdminActor {
            document_id: format!("{root}/{document}"),
            session_id: "session-a".to_string(),
            pane: "%7".to_string(),
            window: "@1".to_string(),
            harness: "claude".to_string(),
            generation: 4,
            state: "ready".to_string(),
            pane_alive: false,
            supervisor_pid: Some(4242),
            cwd: None,
        };
        let finding = AdminFinding {
            kind: "dead_pane".to_string(),
            detail: "pane %7 is gone".to_string(),
            documents: vec![actor.document_id.clone()],
            pane: Some("%7".to_string()),
        };
        AgentDashboard {
            contract_version: DASHBOARD_CONTRACT_VERSION,
            scope: DashboardScope::Project,
            include_clear: false,
            board: build_board([facts]),
            controllers: vec![ProjectControllers {
                project: "agent-loop".to_string(),
                project_root: root.to_string(),
                controller_pids: vec![99],
                fleet: Some(build_dashboard_model(vec![actor], vec![finding])),
                error: None,
            }],
        }
    }

    fn assert_not_a_session_document(rendered: &str) {
        for token in SESSION_TOKENS {
            assert!(!rendered.contains(token), "{token} leaked:\n{rendered}");
        }
        assert!(!rendered.starts_with("---"), "projection has frontmatter");
        assert!(is_dashboard_projection(rendered));
    }

    #[test]
    fn render_composes_board_and_controller_liveness() {
        let dashboard = sample_dashboard("/w/agent-loop", "tasks/plan.md");
        let rendered = render_markdown(
            &dashboard,
            PARAMS,
            Some(Path::new("/w/agent-loop/.agent-doc")),
            "2026-10-04 12:00:00 UTC",
        );
        assert!(rendered.starts_with("<!-- agent-doc-dashboard v1 scope=project all=false -->\n"));
        assert!(rendered.contains("## Needs attention"));
        assert!(rendered.contains("**STALLED** [tasks/plan.md](../tasks/plan.md)"));
        assert!(rendered.contains("## Work board"));
        assert!(rendered.contains("## Controllers and supervisors"));
        assert!(rendered.contains("### agent-loop · controller pid 99"));
        assert!(rendered.contains("| ⚠ | [tasks/plan.md](../tasks/plan.md) | claude | %7 | dead | ready g4 | pid 4242 | dead_pane |"));
        assert!(rendered.contains("- [dead_pane] pane %7 is gone"));
        assert_not_a_session_document(&rendered);
    }

    #[test]
    fn render_never_emits_session_document_tokens() {
        let mut dashboard =
            sample_dashboard("/w/p", "tasks/agent_doc_session <!-- agent:queue -->.md");
        dashboard.controllers[0].error = Some("agent_doc_write failed".to_string());
        dashboard.controllers[0].fleet = None;
        let rendered = render_markdown(
            &dashboard,
            PARAMS,
            Some(Path::new("/w/p/.agent-doc")),
            "now",
        );
        assert_not_a_session_document(&rendered);
        let unlinked = render_markdown(&dashboard, PARAMS, None, "now");
        assert_not_a_session_document(&unlinked);
    }

    #[test]
    fn params_round_trip_through_the_marker_line() {
        let params = DashboardParams {
            scope: DashboardScope::Fleet,
            include_clear: true,
        };
        let line = format!("{}\n# Agent Doc dashboard\n", params.marker_line());
        assert_eq!(DashboardParams::from_projection(&line), Some(params));
        assert_eq!(DashboardParams::from_projection("# notes\n"), None);
    }

    #[test]
    fn write_projection_is_atomic_and_skips_unchanged_bodies() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join(".agent-doc/dashboard.md");
        let first = render_markdown(
            &sample_dashboard("/w/p", "a.md"),
            PARAMS,
            None,
            "2026-10-04 12:00:00 UTC",
        );
        assert!(write_rendered(&path, &first).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);

        // Same state, later clock: nothing is written, the stamp stays put.
        let same = render_markdown(
            &sample_dashboard("/w/p", "a.md"),
            PARAMS,
            None,
            "2026-10-04 12:05:00 UTC",
        );
        assert!(!write_rendered(&path, &same).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), first);

        // Changed state rewrites via rename, leaving no temp file behind.
        let changed = render_markdown(
            &sample_dashboard("/w/p", "b.md"),
            PARAMS,
            None,
            "2026-10-04 12:06:00 UTC",
        );
        assert!(write_rendered(&path, &changed).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), changed);
        let leftovers: Vec<_> = std::fs::read_dir(path.parent().unwrap())
            .unwrap()
            .flatten()
            .map(|entry| entry.file_name().to_string_lossy().to_string())
            .filter(|name| name != "dashboard.md")
            .collect();
        assert!(leftovers.is_empty(), "temp files left: {leftovers:?}");
    }

    #[test]
    fn write_projection_refuses_to_replace_a_session_document() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("plan.md");
        let session = "---\nagent_doc_session: abc\n---\n<!-- agent:exchange -->\n";
        std::fs::write(&path, session).unwrap();
        let rendered = render_markdown(&sample_dashboard("/w/p", "a.md"), PARAMS, None, "now");
        assert!(write_rendered(&path, &rendered).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), session);
    }

    #[test]
    fn refresh_if_armed_honours_the_recorded_scope() {
        let dir = TempDir::new().unwrap();
        let root = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
        std::fs::create_dir_all(root.join("tasks")).unwrap();
        std::fs::write(
            root.join("tasks/plan.md"),
            "<!-- agent:queue go -->\n- do [#alpha]\n<!-- /agent:queue -->\n",
        )
        .unwrap();

        // Not armed: nothing rendered, nothing created.
        assert!(!refresh_if_armed(&root).unwrap());
        let path = root.join(DASHBOARD_DEFAULT_RELATIVE_PATH);
        assert!(!path.exists());

        // A foreign file at the projection path is never overwritten.
        std::fs::write(&path, "# my notes\n").unwrap();
        assert!(!refresh_if_armed(&root).unwrap());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "# my notes\n");

        // Armed with project scope + all=true: rendered with those parameters.
        std::fs::write(
            &path,
            "<!-- agent-doc-dashboard v1 scope=project all=true -->\n",
        )
        .unwrap();
        assert!(refresh_if_armed(&root).unwrap());
        let rendered = std::fs::read_to_string(&path).unwrap();
        assert!(rendered.starts_with("<!-- agent-doc-dashboard v1 scope=project all=true -->\n"));
        assert!(
            rendered.contains("[tasks/plan.md](../tasks/plan.md)"),
            "{rendered}"
        );
        assert_not_a_session_document(&rendered);
        // A second refresh with no state change writes nothing.
        assert!(!refresh_if_armed(&root).unwrap());
    }

    #[test]
    fn relative_path_climbs_out_of_the_projection_directory() {
        assert_eq!(
            relative_path(Path::new("/w/p/.agent-doc"), Path::new("/w/p/tasks/a.md")),
            PathBuf::from("../tasks/a.md")
        );
        assert_eq!(
            relative_path(Path::new("/w/p/.agent-doc"), Path::new("/w/p/src/sub/b.md")),
            PathBuf::from("../src/sub/b.md")
        );
    }
}
