//! Pure fleet-board projection: many documents' tracked work in one view.
//!
//! `agent-doc admin dashboard` answers "which controllers are alive"; this
//! module answers the different question "where is the work, and what is not
//! moving". It is facts in, typed board out: the caller supplies already-parsed
//! per-document counts, and this module owns classification, severity ordering,
//! project grouping, and rendering. No file IO, no git, no controller access.
//!
//! The load-bearing signal is a drainable queue head with no live actor: the
//! queue says there is agent work and nothing is draining it, so it needs a
//! person. Everything else on the board is ordered under that.

use serde::Serialize;

use crate::{AutoDag, Lane};

/// Stable contract version for `--json` consumers.
pub const BOARD_CONTRACT_VERSION: &str = "agent-doc-fleet-board-v1";

/// Auto-DAG lane rollup for one document.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
pub struct LaneCounts {
    pub implementable: usize,
    pub live_verify: usize,
    pub ipc_capture: usize,
    pub blocked: usize,
    pub likely_done: usize,
}

impl LaneCounts {
    /// Roll an `AutoDag` up into per-lane counts.
    pub fn from_dag(dag: &AutoDag) -> Self {
        let mut counts = Self::default();
        for item in &dag.items {
            match item.lane {
                Lane::Implementable => counts.implementable += 1,
                Lane::LiveVerify => counts.live_verify += 1,
                Lane::IpcCapture => counts.ipc_capture += 1,
                Lane::Blocked => counts.blocked += 1,
                Lane::LikelyDone => counts.likely_done += 1,
            }
        }
        counts
    }

    pub fn total(&self) -> usize {
        self.implementable + self.live_verify + self.ipc_capture + self.blocked + self.likely_done
    }

    /// Compact single-cell rendering for the board's `auto-dag` column.
    pub fn cell(&self) -> String {
        if self.total() == 0 {
            return "—".to_string();
        }
        format!(
            "impl:{} live:{} ipc:{} blk:{}",
            self.implementable, self.live_verify, self.ipc_capture, self.blocked
        )
    }
}

/// The severity-ordered state of one session document. Lower `rank` is worse.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DocState {
    /// At least one tracked item is blocked on a decision nobody has made.
    Blocked,
    /// The queue has a drainable head and no live actor is draining it.
    Stalled,
    /// Only operator-gated heads remain: a person must verify or clear them.
    OperatorVerify,
    /// A live actor is draining a queue that still has drainable heads.
    Draining,
    /// Nothing queued, but tracked work is still open.
    Ready,
    /// Nothing queued and nothing open.
    Clear,
}

impl DocState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Blocked => "BLOCKED",
            Self::Stalled => "STALLED",
            Self::OperatorVerify => "OPERATOR",
            Self::Draining => "DRAINING",
            Self::Ready => "READY",
            Self::Clear => "CLEAR",
        }
    }

    /// Severity rank, worst first. Used for row and group ordering.
    pub const fn rank(self) -> u8 {
        match self {
            Self::Blocked => 0,
            Self::Stalled => 1,
            Self::OperatorVerify => 2,
            Self::Draining => 3,
            Self::Ready => 4,
            Self::Clear => 5,
        }
    }

    /// True when this row cannot advance without a person looking at it.
    pub const fn needs_attention(self) -> bool {
        matches!(self, Self::Blocked | Self::Stalled | Self::OperatorVerify)
    }
}

/// Everything the board needs to know about one session document.
///
/// The caller parses these out of document text (and, for `actor_*`, out of the
/// project controller's actor store). Keeping them plain keeps this module pure
/// and keeps the queue/controller crates out of the work-graph dependency tree.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct DocumentFacts {
    /// Grouping key: the submodule path, or the superproject's own label.
    pub project: String,
    /// Absolute project root that owns this document's `.agent-doc/`.
    pub project_root: String,
    /// Document path relative to `project_root`.
    pub path: String,
    /// `agent:queue` is activated for this document.
    pub queue_active: bool,
    /// Frontmatter explicitly parks the queue (`queue_active: false`).
    pub queue_stopped: bool,
    /// Heads the in-session loop may drain right now.
    pub drainable_heads: usize,
    /// Heads deferred to an operator (`[operator-verify]` and friends).
    pub deferred_heads: usize,
    /// Open `agent:backlog` items.
    pub backlog_open: usize,
    /// Open `agent:review` items.
    pub review_open: usize,
    /// Open `agent:review` items whose gate is machine-checkable.
    pub review_gated: usize,
    /// Auto-DAG lane rollup over backlog + review + icebox.
    pub lanes: LaneCounts,
    /// A controller actor is registered and its pane is alive.
    pub actor_alive: bool,
    /// Actor pane id, when one is registered.
    pub actor_pane: Option<String>,
    /// Harness driving the actor, when one is registered.
    pub actor_harness: Option<String>,
}

impl DocumentFacts {
    fn open_work(&self) -> usize {
        self.backlog_open + self.review_open
    }
}

/// Classify one document into its severity lane.
pub fn classify_document(facts: &DocumentFacts) -> DocState {
    if facts.lanes.blocked > 0 {
        return DocState::Blocked;
    }
    if facts.queue_active && facts.drainable_heads > 0 && !facts.actor_alive {
        return DocState::Stalled;
    }
    if facts.drainable_heads == 0 && facts.deferred_heads > 0 {
        return DocState::OperatorVerify;
    }
    if facts.queue_active && facts.drainable_heads > 0 {
        return DocState::Draining;
    }
    if facts.open_work() > 0 {
        return DocState::Ready;
    }
    DocState::Clear
}

/// One short phrase naming why an attention row needs a person.
pub fn attention_reason(facts: &DocumentFacts, state: DocState) -> Option<String> {
    match state {
        DocState::Blocked => Some(format!(
            "{} item(s) blocked on a decision",
            facts.lanes.blocked
        )),
        DocState::Stalled => Some(format!(
            "{} drainable head(s), no live actor",
            facts.drainable_heads
        )),
        DocState::OperatorVerify => Some(format!(
            "{} operator-gated head(s)",
            facts.deferred_heads
        )),
        DocState::Draining | DocState::Ready | DocState::Clear => None,
    }
}

/// One classified document row.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct BoardRow {
    pub state: DocState,
    #[serde(flatten)]
    pub facts: DocumentFacts,
    pub reason: Option<String>,
}

/// One project (superproject or submodule) section of the board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProjectGroup {
    pub project: String,
    pub project_root: String,
    pub rows: Vec<BoardRow>,
    pub attention_count: usize,
}

impl ProjectGroup {
    fn worst_rank(&self) -> u8 {
        self.rows
            .iter()
            .map(|row| row.state.rank())
            .min()
            .unwrap_or(u8::MAX)
    }
}

/// The whole board.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct FleetBoard {
    pub contract_version: &'static str,
    pub groups: Vec<ProjectGroup>,
    pub document_count: usize,
    pub attention_count: usize,
}

/// Build the board from per-document facts: classify, group by project, then
/// order rows and groups worst-first.
pub fn build_board(facts: impl IntoIterator<Item = DocumentFacts>) -> FleetBoard {
    let mut groups: Vec<ProjectGroup> = Vec::new();
    for facts in facts {
        let state = classify_document(&facts);
        let reason = attention_reason(&facts, state);
        let project = facts.project.clone();
        let project_root = facts.project_root.clone();
        let row = BoardRow {
            state,
            facts,
            reason,
        };
        match groups.iter_mut().find(|group| group.project == project) {
            Some(group) => group.rows.push(row),
            None => groups.push(ProjectGroup {
                project,
                project_root,
                rows: vec![row],
                attention_count: 0,
            }),
        }
    }

    for group in &mut groups {
        group.rows.sort_by(|a, b| {
            a.state
                .rank()
                .cmp(&b.state.rank())
                .then_with(|| {
                    (b.facts.drainable_heads + b.facts.open_work())
                        .cmp(&(a.facts.drainable_heads + a.facts.open_work()))
                })
                .then_with(|| a.facts.path.cmp(&b.facts.path))
        });
        group.attention_count = group
            .rows
            .iter()
            .filter(|row| row.state.needs_attention())
            .count();
    }

    groups.sort_by(|a, b| {
        a.worst_rank()
            .cmp(&b.worst_rank())
            .then_with(|| b.attention_count.cmp(&a.attention_count))
            .then_with(|| a.project.cmp(&b.project))
    });

    let document_count = groups.iter().map(|group| group.rows.len()).sum();
    let attention_count = groups.iter().map(|group| group.attention_count).sum();
    FleetBoard {
        contract_version: BOARD_CONTRACT_VERSION,
        groups,
        document_count,
        attention_count,
    }
}

/// Drop `CLEAR` rows (and any project left with none) so the board stays
/// scannable. The counts on the returned board describe what survived.
pub fn without_clear_rows(board: FleetBoard) -> FleetBoard {
    let mut groups: Vec<ProjectGroup> = board
        .groups
        .into_iter()
        .filter_map(|mut group| {
            group.rows.retain(|row| row.state != DocState::Clear);
            (!group.rows.is_empty()).then_some(group)
        })
        .collect();
    for group in &mut groups {
        group.attention_count = group
            .rows
            .iter()
            .filter(|row| row.state.needs_attention())
            .count();
    }
    let document_count = groups.iter().map(|group| group.rows.len()).sum();
    let attention_count = groups.iter().map(|group| group.attention_count).sum();
    FleetBoard {
        contract_version: board.contract_version,
        groups,
        document_count,
        attention_count,
    }
}

const HEADERS: [&str; 8] = [
    "state",
    "document",
    "queue",
    "backlog",
    "review",
    "auto-dag",
    "actor",
    "needs",
];

pub fn queue_cell(facts: &DocumentFacts) -> String {
    if facts.queue_stopped {
        return "stopped".to_string();
    }
    if !facts.queue_active {
        return "—".to_string();
    }
    if facts.deferred_heads > 0 {
        return format!("{}+{}", facts.drainable_heads, facts.deferred_heads);
    }
    facts.drainable_heads.to_string()
}

/// Every open review item is normally gated, so only a partial gate is news.
pub fn review_cell(facts: &DocumentFacts) -> String {
    if facts.review_gated == 0 || facts.review_gated >= facts.review_open {
        return facts.review_open.to_string();
    }
    format!("{} ({} gated)", facts.review_open, facts.review_gated)
}

pub fn actor_cell(facts: &DocumentFacts) -> String {
    match (&facts.actor_pane, &facts.actor_harness) {
        (Some(pane), Some(harness)) if facts.actor_alive => format!("{pane} {harness}"),
        (Some(pane), _) if facts.actor_alive => pane.clone(),
        (Some(pane), _) => format!("{pane} dead"),
        _ => "—".to_string(),
    }
}

fn row_cells(row: &BoardRow) -> [String; 8] {
    [
        row.state.as_str().to_string(),
        row.facts.path.clone(),
        queue_cell(&row.facts),
        row.facts.backlog_open.to_string(),
        review_cell(&row.facts),
        row.facts.lanes.cell(),
        actor_cell(&row.facts),
        row.reason.clone().unwrap_or_else(|| "—".to_string()),
    ]
}

fn push_table(out: &mut String, rows: &[BoardRow]) {
    let mut widths = HEADERS.map(str::len);
    let cells: Vec<[String; 8]> = rows.iter().map(row_cells).collect();
    for row in &cells {
        for (index, cell) in row.iter().enumerate() {
            widths[index] = widths[index].max(cell.chars().count());
        }
    }
    let push_line = |values: &[String; 8], out: &mut String| {
        out.push_str("  ");
        for (index, value) in values.iter().enumerate() {
            if index + 1 == values.len() {
                out.push_str(value);
            } else {
                let pad = widths[index].saturating_sub(value.chars().count());
                out.push_str(value);
                out.push_str(&" ".repeat(pad + 2));
            }
        }
        out.push('\n');
    };
    push_line(&HEADERS.map(ToString::to_string), out);
    for row in &cells {
        push_line(row, out);
    }
}

/// Render the board as a severity-ordered terminal report.
pub fn render_board_text(board: &FleetBoard) -> String {
    let mut out = format!(
        "agent-doc board — {} of {} document(s) need attention\n",
        board.attention_count, board.document_count
    );
    out.push_str(
        "Ordered by severity, not alphabetically. The load-bearing signal is a drainable\n\
         queue head with no live actor: the queue says there is agent work and nothing is\n\
         draining it. A document that is draining does not appear as attention — anything\n\
         counted above needs a person.\n",
    );
    if board.groups.is_empty() {
        out.push_str("\nNo session documents found.\n");
        return out;
    }
    for group in &board.groups {
        out.push_str(&format!(
            "\n{} · {} of {} need attention\n",
            group.project,
            group.attention_count,
            group.rows.len()
        ));
        push_table(&mut out, &group.rows);
    }
    out
}

/// Render the per-lane auto-DAG rollup for every project on the board.
pub fn render_board_dag(board: &FleetBoard) -> String {
    let mut out = String::from("auto-dag rollup by project\n");
    for group in &board.groups {
        let mut totals = LaneCounts::default();
        for row in &group.rows {
            totals.implementable += row.facts.lanes.implementable;
            totals.live_verify += row.facts.lanes.live_verify;
            totals.ipc_capture += row.facts.lanes.ipc_capture;
            totals.blocked += row.facts.lanes.blocked;
            totals.likely_done += row.facts.lanes.likely_done;
        }
        if totals.total() == 0 {
            continue;
        }
        out.push_str(&format!("\n{} ({} open)\n", group.project, totals.total()));
        for (label, count) in [
            (Lane::Implementable.title(), totals.implementable),
            (Lane::LiveVerify.title(), totals.live_verify),
            (Lane::IpcCapture.title(), totals.ipc_capture),
            (Lane::Blocked.title(), totals.blocked),
            (Lane::LikelyDone.title(), totals.likely_done),
        ] {
            if count > 0 {
                out.push_str(&format!("  - {label}: {count}\n"));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DagItem;

    fn facts(project: &str, path: &str) -> DocumentFacts {
        DocumentFacts {
            project: project.to_string(),
            project_root: format!("/repo/{project}"),
            path: path.to_string(),
            ..Default::default()
        }
    }

    #[test]
    fn lane_counts_roll_up_every_lane() {
        let dag = AutoDag {
            items: vec![
                DagItem {
                    id: "a".into(),
                    lane: Lane::Implementable,
                    summary: String::new(),
                },
                DagItem {
                    id: "b".into(),
                    lane: Lane::Blocked,
                    summary: String::new(),
                },
                DagItem {
                    id: "c".into(),
                    lane: Lane::Blocked,
                    summary: String::new(),
                },
            ],
        };
        let counts = LaneCounts::from_dag(&dag);
        assert_eq!(counts.implementable, 1);
        assert_eq!(counts.blocked, 2);
        assert_eq!(counts.total(), 3);
        assert_eq!(counts.cell(), "impl:1 live:0 ipc:0 blk:2");
    }

    #[test]
    fn empty_lane_counts_render_as_a_dash() {
        assert_eq!(LaneCounts::default().cell(), "—");
    }

    #[test]
    fn a_drainable_head_with_no_live_actor_is_stalled() {
        let mut f = facts("agent-doc", "tasks/bugs.md");
        f.queue_active = true;
        f.drainable_heads = 2;
        assert_eq!(classify_document(&f), DocState::Stalled);
        assert_eq!(
            attention_reason(&f, DocState::Stalled).as_deref(),
            Some("2 drainable head(s), no live actor")
        );
    }

    #[test]
    fn the_same_head_with_a_live_actor_is_draining_not_attention() {
        let mut f = facts("agent-doc", "tasks/bugs.md");
        f.queue_active = true;
        f.drainable_heads = 2;
        f.actor_alive = true;
        let state = classify_document(&f);
        assert_eq!(state, DocState::Draining);
        assert!(!state.needs_attention());
        assert_eq!(attention_reason(&f, state), None);
    }

    #[test]
    fn a_blocked_item_outranks_a_live_drain() {
        let mut f = facts("agent-doc", "tasks/bugs.md");
        f.queue_active = true;
        f.drainable_heads = 2;
        f.actor_alive = true;
        f.lanes.blocked = 1;
        assert_eq!(classify_document(&f), DocState::Blocked);
    }

    #[test]
    fn only_deferred_heads_left_is_operator_verify() {
        let mut f = facts("agent-doc", "tasks/bugs.md");
        f.queue_active = true;
        f.drainable_heads = 0;
        f.deferred_heads = 3;
        let state = classify_document(&f);
        assert_eq!(state, DocState::OperatorVerify);
        assert!(state.needs_attention());
    }

    #[test]
    fn open_work_with_no_queue_is_ready_and_no_work_is_clear() {
        let mut f = facts("agent-doc", "tasks/bugs.md");
        f.backlog_open = 4;
        assert_eq!(classify_document(&f), DocState::Ready);
        f.backlog_open = 0;
        assert_eq!(classify_document(&f), DocState::Clear);
    }

    #[test]
    fn a_stopped_queue_with_pending_heads_is_not_stalled() {
        let mut f = facts("agent-doc", "tasks/bugs.md");
        f.queue_stopped = true;
        f.queue_active = false;
        f.drainable_heads = 2;
        f.backlog_open = 1;
        assert_eq!(
            classify_document(&f),
            DocState::Ready,
            "an operator-parked queue is not an un-owned drain"
        );
        assert_eq!(queue_cell(&f), "stopped");
    }

    #[test]
    fn groups_and_rows_order_worst_first() {
        let mut clean = facts("aaa-first-alphabetically", "clean.md");
        clean.backlog_open = 1;

        let mut stalled = facts("zzz-last-alphabetically", "stalled.md");
        stalled.queue_active = true;
        stalled.drainable_heads = 1;

        let mut blocked = facts("zzz-last-alphabetically", "blocked.md");
        blocked.lanes.blocked = 1;

        let board = build_board([clean, stalled, blocked]);
        assert_eq!(board.document_count, 3);
        assert_eq!(board.attention_count, 2);
        assert_eq!(
            board.groups[0].project, "zzz-last-alphabetically",
            "the worst project sorts first even though it sorts last by name"
        );
        assert_eq!(board.groups[0].attention_count, 2);
        assert_eq!(board.groups[0].rows[0].state, DocState::Blocked);
        assert_eq!(board.groups[0].rows[1].state, DocState::Stalled);
        assert_eq!(board.groups[1].rows[0].state, DocState::Ready);
    }

    #[test]
    fn rows_within_a_state_order_by_open_volume_then_path() {
        let mut small = facts("p", "a-small.md");
        small.backlog_open = 1;
        let mut large = facts("p", "z-large.md");
        large.backlog_open = 9;
        let board = build_board([small, large]);
        assert_eq!(board.groups[0].rows[0].facts.path, "z-large.md");
        assert_eq!(board.groups[0].rows[1].facts.path, "a-small.md");
    }

    #[test]
    fn without_clear_rows_drops_rows_and_empty_projects_and_recounts() {
        let mut open = facts("p", "open.md");
        open.backlog_open = 1;
        let board = build_board([open, facts("p", "clear.md"), facts("q", "clear.md")]);
        assert_eq!(board.document_count, 3);

        let trimmed = without_clear_rows(board);
        assert_eq!(trimmed.document_count, 1);
        assert_eq!(trimmed.groups.len(), 1, "project `q` had only CLEAR rows");
        assert_eq!(trimmed.groups[0].rows[0].facts.path, "open.md");
    }

    #[test]
    fn text_render_names_the_counts_and_every_column() {
        let mut stalled = facts("agent-doc", "tasks/bugs.md");
        stalled.queue_active = true;
        stalled.drainable_heads = 2;
        stalled.deferred_heads = 1;
        stalled.backlog_open = 5;
        stalled.review_open = 3;
        stalled.review_gated = 2;
        stalled.lanes.live_verify = 2;
        let text = render_board_text(&build_board([stalled]));

        assert!(text.starts_with("agent-doc board — 1 of 1 document(s) need attention"));
        for header in HEADERS {
            assert!(text.contains(header), "missing column {header}");
        }
        assert!(text.contains("agent-doc · 1 of 1 need attention"));
        assert!(text.contains("STALLED"));
        assert!(text.contains("2+1"), "drainable+deferred head split");
        assert!(text.contains("3 (2 gated)"), "gated review split");
        assert!(text.contains("impl:0 live:2 ipc:0 blk:0"));
        assert!(text.contains("2 drainable head(s), no live actor"));
    }

    #[test]
    fn text_render_reports_an_empty_board_without_a_table() {
        let text = render_board_text(&build_board([]));
        assert!(text.contains("No session documents found."));
        assert!(!text.contains("auto-dag"));
    }

    #[test]
    fn dag_rollup_sums_lanes_per_project_and_skips_empty_projects() {
        let mut one = facts("p", "a.md");
        one.lanes.implementable = 2;
        let mut two = facts("p", "b.md");
        two.lanes.implementable = 1;
        two.lanes.blocked = 1;
        let empty = facts("q", "c.md");

        let rendered = render_board_dag(&build_board([one, two, empty]));
        assert!(rendered.contains("p (4 open)"));
        assert!(rendered.contains("Path C — implementable now (no live pane, agent-executable): 3"));
        assert!(rendered.contains("Blocked on a design decision: 1"));
        assert!(!rendered.contains("\nq ("), "a project with no open items is skipped");
    }

    #[test]
    fn actor_cell_distinguishes_alive_registered_and_dead_panes() {
        let mut f = facts("p", "a.md");
        assert_eq!(actor_cell(&f), "—");
        f.actor_pane = Some("%241".into());
        assert_eq!(actor_cell(&f), "%241 dead");
        f.actor_alive = true;
        assert_eq!(actor_cell(&f), "%241");
        f.actor_harness = Some("claude-code".into());
        assert_eq!(actor_cell(&f), "%241 claude-code");
    }

    #[test]
    fn board_json_carries_the_contract_version() {
        let board = build_board([facts("p", "a.md")]);
        let json = serde_json::to_value(&board).unwrap();
        assert_eq!(json["contract_version"], BOARD_CONTRACT_VERSION);
        assert_eq!(json["groups"][0]["rows"][0]["state"], "clear");
        assert_eq!(json["groups"][0]["rows"][0]["path"], "a.md");
    }
}
