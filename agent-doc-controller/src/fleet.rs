//! Pure fleet status and dashboard view model policy.

use serde::Serialize;
use std::collections::BTreeMap;

/// One enumerated actor row (`admin list`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AdminActor {
    pub document_id: String,
    pub session_id: String,
    pub pane: String,
    pub window: String,
    pub harness: String,
    pub generation: u64,
    pub state: String,
    pub pane_alive: bool,
    pub supervisor_pid: Option<u32>,
    pub cwd: Option<String>,
}

/// Minimal stored actor snapshot used to build `admin list` rows.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorListRecord {
    pub document_id: String,
    pub session_id: String,
    pub pane: String,
    pub window: String,
    pub harness: String,
    pub generation: u64,
    pub state: String,
}

/// Registry metadata that enriches an actor row by session id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActorListRegistryBinding {
    pub session_id: String,
    pub supervisor_pid: u32,
    pub cwd: String,
}

/// One derived diagnostic (`admin detect`).
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct AdminFinding {
    pub kind: String,
    pub detail: String,
    pub documents: Vec<String>,
    pub pane: Option<String>,
}

/// Plain-text fields rendered for mutating admin command receipts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdminReceiptLine<'a> {
    pub operation_kind: &'a str,
    pub status: &'a str,
    pub receipt_id: u64,
    pub document_id: Option<&'a str>,
    pub failed_stage: Option<&'a str>,
    pub current_generation: Option<u64>,
    pub unblock_hint: Option<&'a str>,
}

/// One rendered dashboard row: an enumerated actor plus the finding kinds that
/// implicate it. `problem` is the highlight flag the view paints.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DashboardRow {
    #[serde(flatten)]
    pub actor: AdminActor,
    /// Distinct `admin detect` finding kinds naming this actor's document or pane.
    pub highlight_kinds: Vec<String>,
    /// Effective queue control state, when the controller has one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_control_state: Option<String>,
    /// Latest typed queue pressure class, when dispatch was blocked/rejected.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pressure: Option<String>,
    /// Whether projection diagnostics report non-completed repair/emit state.
    #[serde(default)]
    pub projection_lag: bool,
    /// Whether this row is flagged (any finding implicates it).
    pub problem: bool,
}

/// Controller diagnostics attached to a dashboard actor row.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct DashboardActorDiagnostics {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_control_state: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub queue_pressure: Option<String>,
    #[serde(default)]
    pub projection_lag: bool,
}

/// The full dashboard model: highlighted rows plus the raw findings that drove
/// the highlighting, so the view can render a summary footer.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DashboardModel {
    pub rows: Vec<DashboardRow>,
    pub findings: Vec<AdminFinding>,
    pub problem_count: usize,
}

/// Build the enumerated actor list from stored actor rows + registry metadata.
///
/// Pure over its inputs (liveness is injected) so callers can adapt from SQLite
/// and tmux state without making this crate depend on those adapters.
pub fn build_admin_actor_list(
    actors: impl IntoIterator<Item = ActorListRecord>,
    registry: impl IntoIterator<Item = ActorListRegistryBinding>,
    pane_alive: impl Fn(&str) -> bool,
) -> Vec<AdminActor> {
    let by_session: BTreeMap<String, ActorListRegistryBinding> = registry
        .into_iter()
        .map(|entry| (entry.session_id.clone(), entry))
        .collect();

    actors
        .into_iter()
        .map(|record| {
            let reg = by_session.get(record.session_id.as_str());
            let is_pane_alive = !record.pane.is_empty() && pane_alive(&record.pane);
            AdminActor {
                document_id: record.document_id,
                session_id: record.session_id,
                pane: record.pane,
                window: record.window,
                harness: record.harness,
                generation: record.generation,
                state: record.state,
                pane_alive: is_pane_alive,
                supervisor_pid: reg.map(|entry| entry.supervisor_pid),
                cwd: reg.map(|entry| entry.cwd.clone()),
            }
        })
        .collect()
}

/// Render the concise non-JSON line for mutating admin command receipts.
pub fn format_admin_receipt_line(receipt: AdminReceiptLine<'_>) -> String {
    let mut line = format!(
        "{} {} receipt_id={}",
        receipt.operation_kind, receipt.status, receipt.receipt_id
    );
    if let Some(document_id) = receipt.document_id {
        line.push_str(&format!(" document={document_id}"));
    }
    if let Some(stage) = receipt.failed_stage {
        line.push_str(&format!(" failed_stage={stage}"));
    }
    if let Some(current) = receipt.current_generation {
        line.push_str(&format!(" current_generation={current}"));
    }
    if let Some(hint) = receipt.unblock_hint {
        line.push_str(&format!(" hint={hint}"));
    }
    line
}

/// Derive cross-document / staleness findings from enumerated admin actor rows.
///
/// Pure over already-adapted rows (no DB, no tmux):
///
/// - `cross_document_pane`: a single live pane is the authoritative binding of
///   more than one non-closed document.
/// - `stale_dead_pane`: a non-closed actor whose pane is not alive.
pub fn detect_admin_findings(rows: &[AdminActor]) -> Vec<AdminFinding> {
    let mut findings = Vec::new();

    let mut by_pane: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for row in rows {
        if row.state == "closed" || row.pane.is_empty() || !row.pane_alive {
            continue;
        }
        by_pane
            .entry(row.pane.as_str())
            .or_default()
            .push(row.document_id.as_str());
    }
    for (pane, docs) in &by_pane {
        if docs.len() > 1 {
            let mut documents: Vec<String> = docs.iter().map(|d| d.to_string()).collect();
            documents.sort();
            findings.push(AdminFinding {
                kind: "cross_document_pane".to_string(),
                detail: format!(
                    "pane {} is the live binding of {} documents; only one actor may own a pane",
                    pane,
                    documents.len()
                ),
                documents,
                pane: Some((*pane).to_string()),
            });
        }
    }

    for row in rows {
        if row.state == "closed" || row.pane.is_empty() || row.pane_alive {
            continue;
        }
        findings.push(AdminFinding {
            kind: "stale_dead_pane".to_string(),
            detail: format!(
                "actor for {} is {} but its pane {} is not alive (orphaned binding)",
                row.document_id, row.state, row.pane
            ),
            documents: vec![row.document_id.clone()],
            pane: Some(row.pane.clone()),
        });
    }

    findings
}

/// Fold enumerated actors + derived findings into the deterministic view model.
///
/// Pure over its inputs (no I/O, no clock, no tmux), so the highlight logic is
/// unit-testable. A row is flagged when its `document_id` appears in any
/// finding's `documents`, or when the finding's `pane` matches the row's pane.
/// Rows are sorted by `document_id` for stable rendering across ticks.
pub fn build_dashboard_model(
    actors: Vec<AdminActor>,
    findings: Vec<AdminFinding>,
) -> DashboardModel {
    build_dashboard_model_with_diagnostics(actors, findings, BTreeMap::new())
}

/// Build a dashboard model with controller queue/projection diagnostics.
pub fn build_dashboard_model_with_diagnostics(
    mut actors: Vec<AdminActor>,
    findings: Vec<AdminFinding>,
    diagnostics: BTreeMap<String, DashboardActorDiagnostics>,
) -> DashboardModel {
    actors.sort_by(|a, b| {
        a.document_id
            .cmp(&b.document_id)
            .then_with(|| a.pane.cmp(&b.pane))
    });

    let rows: Vec<DashboardRow> = actors
        .into_iter()
        .map(|actor| {
            let mut highlight_kinds: Vec<String> = findings
                .iter()
                .filter(|f| {
                    f.documents.contains(&actor.document_id)
                        || f.pane.as_deref() == Some(actor.pane.as_str())
                })
                .map(|f| f.kind.clone())
                .collect();
            highlight_kinds.sort();
            highlight_kinds.dedup();
            let diagnostics = diagnostics
                .get(&actor.document_id)
                .cloned()
                .unwrap_or_default();
            let problem = !highlight_kinds.is_empty()
                || diagnostics.queue_pressure.is_some()
                || diagnostics.projection_lag;
            DashboardRow {
                actor,
                highlight_kinds,
                queue_control_state: diagnostics.queue_control_state,
                queue_pressure: diagnostics.queue_pressure,
                projection_lag: diagnostics.projection_lag,
                problem,
            }
        })
        .collect();

    let problem_count = rows.iter().filter(|r| r.problem).count();
    DashboardModel {
        rows,
        findings,
        problem_count,
    }
}

const RED: &str = "\x1b[31m";
const BOLD: &str = "\x1b[1m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

/// Render the dashboard model into a stable terminal frame.
///
/// `color` gates ANSI styling so tests can assert on plain text. The layout is
/// deterministic given the model, which keeps redraws flicker-stable and makes
/// the render unit-testable.
pub fn render_dashboard(model: &DashboardModel, color: bool) -> String {
    let (red, bold, dim, reset) = if color {
        (RED, BOLD, DIM, RESET)
    } else {
        ("", "", "", "")
    };

    let mut out = String::new();
    out.push_str(&format!(
        "{bold}agent-doc fleet — {} actor(s), {} flagged{reset}\n",
        model.rows.len(),
        model.problem_count,
    ));

    if model.rows.is_empty() {
        out.push_str(&format!("{dim}  (no registered actors){reset}\n"));
        return out;
    }

    out.push_str(&format!(
        "{dim}  {:<2} {:<28} {:<9} {:<6} {:<6} {:<13} {:<8} {:<13} {:<5} {}{reset}\n",
        "", "document", "harness", "pane", "alive", "state", "pid", "queue", "proj", "flags",
    ));

    for row in &model.rows {
        let marker = if row.problem { "!" } else { " " };
        let pid = row
            .actor
            .supervisor_pid
            .map(|p| p.to_string())
            .unwrap_or_else(|| "?".to_string());
        let alive = if row.actor.pane_alive {
            "alive"
        } else {
            "dead"
        };
        let flags = row_flags(row).join(",");
        let queue = queue_summary(row);
        let projection = if row.projection_lag { "lag" } else { "ok" };
        let line = format!(
            "  {:<2} {:<28} {:<9} {:<6} {:<6} {:<13} {:<8} {:<13} {:<5} {}",
            marker,
            truncate(&row.actor.document_id, 28),
            truncate(&row.actor.harness, 9),
            truncate(&row.actor.pane, 6),
            alive,
            truncate(
                &format!("{} g{}", row.actor.state, row.actor.generation),
                13
            ),
            truncate(&pid, 8),
            truncate(&queue, 13),
            projection,
            flags,
        );
        if row.problem {
            out.push_str(&format!("{red}{line}{reset}\n"));
        } else {
            out.push_str(&line);
            out.push('\n');
        }
    }

    if !model.findings.is_empty() {
        out.push_str(&format!("\n{bold}findings:{reset}\n"));
        for f in &model.findings {
            out.push_str(&format!("{red}  [{}] {}{reset}\n", f.kind, f.detail));
        }
    }

    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if max <= 1 {
        return s.chars().take(max).collect();
    }
    let head: String = s.chars().take(max - 1).collect();
    format!("{head}…")
}

fn queue_summary(row: &DashboardRow) -> String {
    if let Some(pressure) = row.queue_pressure.as_deref() {
        return format!("pressure:{pressure}");
    }
    row.queue_control_state
        .clone()
        .unwrap_or_else(|| "ok".to_string())
}

fn row_flags(row: &DashboardRow) -> Vec<String> {
    let mut flags = row.highlight_kinds.clone();
    if let Some(pressure) = row.queue_pressure.as_deref() {
        flags.push(format!("queue_pressure:{pressure}"));
    }
    if row.projection_lag {
        flags.push("projection_lag".to_string());
    }
    flags
}

#[cfg(test)]
mod tests {
    use super::*;

    fn actor_list_record(
        document_id: &str,
        session_id: &str,
        pane: &str,
        state: &str,
    ) -> ActorListRecord {
        ActorListRecord {
            document_id: document_id.to_string(),
            session_id: session_id.to_string(),
            pane: pane.to_string(),
            window: "@1".to_string(),
            harness: "codex".to_string(),
            generation: 1,
            state: state.to_string(),
        }
    }

    fn registry_binding(
        session_id: &str,
        supervisor_pid: u32,
        cwd: &str,
    ) -> ActorListRegistryBinding {
        ActorListRegistryBinding {
            session_id: session_id.to_string(),
            supervisor_pid,
            cwd: cwd.to_string(),
        }
    }

    fn actor(document_id: &str, pane: &str, state: &str, alive: bool) -> AdminActor {
        AdminActor {
            document_id: document_id.to_string(),
            session_id: format!("sid-{document_id}"),
            pane: pane.to_string(),
            window: "@1".to_string(),
            harness: "codex".to_string(),
            generation: 1,
            state: state.to_string(),
            pane_alive: alive,
            supervisor_pid: Some(1234),
            cwd: Some("/proj".to_string()),
        }
    }

    #[test]
    fn build_admin_actor_list_enriches_with_registry_and_liveness() {
        let actors = vec![
            actor_list_record("tasks/a.md", "sid-a", "%1", "busy"),
            actor_list_record("tasks/b.md", "sid-b", "%2", "ready"),
        ];
        let registry = vec![
            registry_binding("sid-a", 1001, "/proj"),
            registry_binding("sid-b", 1002, "/proj"),
        ];

        let rows = build_admin_actor_list(actors, registry, |p| p == "%1");
        assert_eq!(rows.len(), 2);

        let a = rows.iter().find(|r| r.document_id == "tasks/a.md").unwrap();
        assert_eq!(a.pane, "%1");
        assert!(a.pane_alive);
        assert_eq!(a.state, "busy");
        assert_eq!(a.supervisor_pid, Some(1001));
        assert_eq!(a.cwd.as_deref(), Some("/proj"));

        let b = rows.iter().find(|r| r.document_id == "tasks/b.md").unwrap();
        assert!(!b.pane_alive, "dead pane must be marked not alive");
        assert_eq!(b.supervisor_pid, Some(1002));
    }

    #[test]
    fn format_admin_receipt_line_includes_optional_fields() {
        let line = format_admin_receipt_line(AdminReceiptLine {
            operation_kind: "queue_control",
            status: "rejected",
            receipt_id: 42,
            document_id: Some("tasks/a.md"),
            failed_stage: Some("cas"),
            current_generation: Some(7),
            unblock_hint: Some("retry"),
        });
        assert_eq!(
            line,
            "queue_control rejected receipt_id=42 document=tasks/a.md failed_stage=cas current_generation=7 hint=retry"
        );
    }

    #[test]
    fn detect_flags_two_documents_sharing_one_live_pane() {
        let rows = vec![
            actor("tasks/agent-doc-bugs2.md", "%70", "busy", true),
            actor("tasks/lazily-rs.md", "%70", "ready", true),
        ];
        let findings = detect_admin_findings(&rows);
        assert_eq!(findings.len(), 1, "exactly one cross-document finding");
        let f = &findings[0];
        assert_eq!(f.kind, "cross_document_pane");
        assert_eq!(f.pane.as_deref(), Some("%70"));
        assert_eq!(
            f.documents,
            vec![
                "tasks/agent-doc-bugs2.md".to_string(),
                "tasks/lazily-rs.md".to_string()
            ]
        );
    }

    #[test]
    fn detect_flags_non_closed_actor_with_dead_pane() {
        let rows = vec![actor("tasks/a.md", "%9", "busy", false)];
        let findings = detect_admin_findings(&rows);
        assert_eq!(findings.len(), 1);
        assert_eq!(findings[0].kind, "stale_dead_pane");
        assert_eq!(findings[0].pane.as_deref(), Some("%9"));
    }

    #[test]
    fn detect_clean_fleet_reports_nothing() {
        let rows = vec![
            actor("tasks/a.md", "%1", "busy", true),
            actor("tasks/b.md", "%2", "ready", true),
        ];
        let findings = detect_admin_findings(&rows);
        assert!(
            findings.is_empty(),
            "distinct live panes, one owner each must have no findings, got {findings:?}"
        );
    }

    #[test]
    fn detect_does_not_flag_cross_document_on_dead_shared_pane() {
        let rows = vec![
            actor("tasks/a.md", "%9", "ready", false),
            actor("tasks/b.md", "%9", "ready", false),
        ];
        let findings = detect_admin_findings(&rows);
        assert!(
            findings.iter().all(|f| f.kind == "stale_dead_pane"),
            "dead shared pane must not produce cross_document_pane, got {findings:?}"
        );
        assert_eq!(
            findings
                .iter()
                .filter(|f| f.kind == "stale_dead_pane")
                .count(),
            2,
            "each non-closed actor on the dead pane is an orphaned binding"
        );
    }

    #[test]
    fn detect_ignores_closed_actors() {
        let rows = vec![
            actor("tasks/old.md", "%5", "closed", true),
            actor("tasks/new.md", "%5", "busy", true),
            actor("tasks/closed-dead.md", "%9", "closed", false),
        ];
        let findings = detect_admin_findings(&rows);
        assert!(
            findings.is_empty(),
            "closed actor rows must not emit findings, got {findings:?}"
        );
    }

    #[test]
    fn model_sorts_rows_and_flags_nothing_on_clean_fleet() {
        let actors = vec![
            actor("tasks/b.md", "%2", "ready", true),
            actor("tasks/a.md", "%1", "busy", true),
        ];
        let model = build_dashboard_model(actors, vec![]);
        assert_eq!(model.rows.len(), 2);
        assert_eq!(model.rows[0].actor.document_id, "tasks/a.md");
        assert_eq!(model.rows[1].actor.document_id, "tasks/b.md");
        assert_eq!(model.problem_count, 0);
        assert!(model.rows.iter().all(|r| !r.problem));
    }

    #[test]
    fn cross_document_finding_flags_both_rows() {
        let actors = vec![
            actor("tasks/agent-doc-bugs2.md", "%70", "busy", true),
            actor("tasks/lazily-rs.md", "%70", "ready", true),
            actor("tasks/other.md", "%5", "ready", true),
        ];
        let findings = vec![AdminFinding {
            kind: "cross_document_pane".to_string(),
            detail: "pane %70 is the live binding of 2 documents".to_string(),
            documents: vec![
                "tasks/agent-doc-bugs2.md".to_string(),
                "tasks/lazily-rs.md".to_string(),
            ],
            pane: Some("%70".to_string()),
        }];
        let model = build_dashboard_model(actors, findings);
        assert_eq!(model.problem_count, 2);

        let flagged: Vec<&str> = model
            .rows
            .iter()
            .filter(|r| r.problem)
            .map(|r| r.actor.document_id.as_str())
            .collect();
        assert_eq!(
            flagged,
            vec!["tasks/agent-doc-bugs2.md", "tasks/lazily-rs.md"]
        );

        let clean = model
            .rows
            .iter()
            .find(|r| r.actor.document_id == "tasks/other.md")
            .unwrap();
        assert!(!clean.problem);
        assert!(clean.highlight_kinds.is_empty());

        let flagged_row = model
            .rows
            .iter()
            .find(|r| r.actor.document_id == "tasks/lazily-rs.md")
            .unwrap();
        assert_eq!(flagged_row.highlight_kinds, vec!["cross_document_pane"]);
    }

    #[test]
    fn stale_dead_pane_finding_flags_by_pane_match() {
        let actors = vec![actor("tasks/a.md", "%9", "busy", false)];
        let findings = vec![AdminFinding {
            kind: "stale_dead_pane".to_string(),
            detail: "orphaned binding".to_string(),
            documents: vec!["tasks/a.md".to_string()],
            pane: Some("%9".to_string()),
        }];
        let model = build_dashboard_model(actors, findings);
        assert_eq!(model.problem_count, 1);
        assert_eq!(model.rows[0].highlight_kinds, vec!["stale_dead_pane"]);
    }

    #[test]
    fn render_is_deterministic_plain_text() {
        let actors = vec![actor("tasks/a.md", "%1", "busy", true)];
        let model = build_dashboard_model(actors, vec![]);
        let frame = render_dashboard(&model, false);
        assert!(frame.contains("1 actor(s), 0 flagged"));
        assert!(frame.contains("tasks/a.md"));
        assert!(!frame.contains('\x1b'), "plain render must carry no ANSI");
    }

    #[test]
    fn render_flagged_row_carries_findings_footer_and_marker() {
        let actors = vec![actor("tasks/a.md", "%70", "busy", true)];
        let findings = vec![AdminFinding {
            kind: "cross_document_pane".to_string(),
            detail: "pane %70 is the live binding of 2 documents".to_string(),
            documents: vec!["tasks/a.md".to_string()],
            pane: Some("%70".to_string()),
        }];
        let model = build_dashboard_model(actors, findings);
        let frame = render_dashboard(&model, false);
        assert!(frame.contains("1 actor(s), 1 flagged"));
        assert!(frame.contains("cross_document_pane"));
        assert!(frame.contains("findings:"));
        assert!(
            frame
                .lines()
                .any(|line| line.contains("tasks/a.md") && line.contains('!'))
        );
    }

    #[test]
    fn queue_pressure_and_projection_lag_flag_dashboard_row() {
        let actors = vec![actor("tasks/a.md", "%1", "ready", true)];
        let mut diagnostics = BTreeMap::new();
        diagnostics.insert(
            "tasks/a.md".to_string(),
            DashboardActorDiagnostics {
                queue_control_state: Some("paused".to_string()),
                queue_pressure: Some("queue_full".to_string()),
                projection_lag: true,
            },
        );

        let model = build_dashboard_model_with_diagnostics(actors, vec![], diagnostics);

        assert_eq!(model.problem_count, 1);
        assert_eq!(model.rows[0].queue_control_state.as_deref(), Some("paused"));
        assert_eq!(model.rows[0].queue_pressure.as_deref(), Some("queue_full"));
        assert!(model.rows[0].projection_lag);

        let frame = render_dashboard(&model, false);
        assert!(frame.contains("lag"));
        assert!(frame.contains("queue_pressure:queue_full"));
        assert!(frame.contains("projection_lag"));
    }

    #[test]
    fn empty_fleet_renders_placeholder() {
        let model = build_dashboard_model(vec![], vec![]);
        let frame = render_dashboard(&model, false);
        assert!(frame.contains("0 actor(s), 0 flagged"));
        assert!(frame.contains("no registered actors"));
    }

    #[test]
    fn truncate_keeps_unicode_safe() {
        assert_eq!(truncate("abc", 5), "abc");
        assert_eq!(truncate("abcdef", 4), "abc…");
        let s = "αβγδεζ";
        let t = truncate(s, 3);
        assert_eq!(t, "αβ…");
    }
}
