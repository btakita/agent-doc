//! Shared pure tmux model state for agent-doc.
//!
//! This crate owns reusable tmux observations, realtime state, and layout
//! projection policy. It does not execute `tmux` commands, dispatch turns,
//! merge document text, or commit document changes.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, HashSet},
    fmt,
};

pub const TMUX_PANE_GEOMETRY_FORMAT: &str =
    "#{pane_id} #{pane_left} #{pane_top} #{pane_width} #{pane_height}";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TmuxPaneActivity {
    Unknown,
    PromptReady,
    Busy,
    OutputActive,
    Exited,
    Missing,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TmuxSupervisorHealth {
    Unknown,
    Healthy,
    Stale,
    Crashed,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxRealtimeState {
    pub pane: TmuxPaneActivity,
    pub supervisor: TmuxSupervisorHealth,
    pub last_output_watermark: Option<String>,
}

impl Default for TmuxRealtimeState {
    fn default() -> Self {
        Self {
            pane: TmuxPaneActivity::Unknown,
            supervisor: TmuxSupervisorHealth::Unknown,
            last_output_watermark: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TmuxObservation {
    PaneMissing,
    PaneExited,
    PromptReady,
    Busy,
    OutputAdvanced { watermark: String },
    SupervisorHeartbeat,
    SupervisorStale,
    SupervisorCrashed,
}

/// Outcome of reconciling a candidate pane against the document's provably-live
/// owner pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FocusPaneDecision {
    /// Keep the candidate pane resolved from projection or registry.
    UseCandidate,
    /// Focus the pane that currently owns the document instead of the stale
    /// projection/registry candidate.
    RepairToLiveOwner(String),
}

/// Decide whether focus should keep its candidate pane or swap to a provable
/// live owner pane.
///
/// The live owner wins only when it is a different, non-empty pane. When no live
/// owner is provable, or it matches the candidate, the existing selection is
/// preserved so the happy path is unchanged.
pub fn decide_focus_pane(candidate: &str, live_owner: Option<&str>) -> FocusPaneDecision {
    match live_owner {
        Some(owner) if !owner.is_empty() && owner != candidate => {
            FocusPaneDecision::RepairToLiveOwner(owner.to_string())
        }
        _ => FocusPaneDecision::UseCandidate,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PanePosition {
    Left,
    Right,
    Top,
    Bottom,
}

impl PanePosition {
    pub fn parse(input: &str) -> Result<Self, PanePositionError> {
        match input {
            "left" => Ok(Self::Left),
            "right" => Ok(Self::Right),
            "top" => Ok(Self::Top),
            "bottom" => Ok(Self::Bottom),
            _ => Err(PanePositionError::InvalidPosition {
                position: input.to_string(),
            }),
        }
    }

    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Right => "right",
            Self::Top => "top",
            Self::Bottom => "bottom",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxPaneGeometry {
    pub id: String,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
}

impl TmuxPaneGeometry {
    fn right_edge(&self) -> u32 {
        self.left.saturating_add(self.width)
    }

    fn bottom_edge(&self) -> u32 {
        self.top.saturating_add(self.height)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PanePositionError {
    NoPanes { scope: String },
    InvalidPosition { position: String },
    Unresolved { position: String },
}

impl fmt::Display for PanePositionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPanes { scope } => write!(f, "no panes found in {scope}"),
            Self::InvalidPosition { position } => write!(
                f,
                "invalid position '{position}' - use left, right, top, or bottom"
            ),
            Self::Unresolved { position } => {
                write!(f, "could not resolve pane for position '{position}'")
            }
        }
    }
}

impl std::error::Error for PanePositionError {}

pub fn parse_tmux_pane_geometry(text: &str) -> Vec<TmuxPaneGeometry> {
    text.lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let id = parts.next()?;
            let left = parts.next()?.parse().unwrap_or(0);
            let top = parts.next()?.parse().unwrap_or(0);
            let width = parts.next()?.parse().unwrap_or(0);
            let height = parts.next()?.parse().unwrap_or(0);
            Some(TmuxPaneGeometry {
                id: id.to_string(),
                left,
                top,
                width,
                height,
            })
        })
        .collect()
}

pub fn select_pane_by_position(
    text: &str,
    position: &str,
    scope: &str,
) -> Result<String, PanePositionError> {
    let panes = parse_tmux_pane_geometry(text);
    if panes.is_empty() {
        return Err(PanePositionError::NoPanes {
            scope: scope.to_string(),
        });
    }
    if panes.len() == 1 {
        return Ok(panes[0].id.clone());
    }

    let position = PanePosition::parse(position)?;
    let selected = match position {
        PanePosition::Left => panes.iter().min_by_key(|pane| pane.left),
        PanePosition::Right => panes.iter().max_by_key(|pane| pane.right_edge()),
        PanePosition::Top => panes.iter().min_by_key(|pane| pane.top),
        PanePosition::Bottom => panes.iter().max_by_key(|pane| pane.bottom_edge()),
    };

    selected
        .map(|pane| pane.id.clone())
        .ok_or_else(|| PanePositionError::Unresolved {
            position: position.as_str().to_string(),
        })
}

/// Foreground program names tmux reports via `#{pane_current_command}` when a
/// pane has fallen back to a bare interactive shell. Login shells can show a
/// leading `-`, for example `-zsh`.
pub fn pane_current_command_is_bare_shell(cmd: &str) -> bool {
    let name = cmd.trim().trim_start_matches('-');
    matches!(
        name,
        "zsh" | "bash" | "sh" | "fish" | "dash" | "ksh" | "tcsh" | "csh"
    )
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxLayoutColumn {
    pub raw: String,
    pub agent_doc: Option<String>,
}

impl TmuxLayoutColumn {
    pub fn new(raw: impl Into<String>, agent_doc: Option<String>) -> Self {
        Self {
            raw: raw.into(),
            agent_doc,
        }
    }

    fn trimmed_raw(&self) -> String {
        self.raw.trim().to_string()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColumnMemoryRestoration {
    pub column_index: usize,
    pub remembered: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ColumnMemoryApplication {
    pub columns: Vec<String>,
    pub restorations: Vec<ColumnMemoryRestoration>,
}

pub fn apply_column_memory(
    columns: &[TmuxLayoutColumn],
    saved_layout: &[String],
) -> ColumnMemoryApplication {
    let visible_docs: HashSet<String> = columns
        .iter()
        .filter_map(|col| col.agent_doc.clone())
        .collect();
    let mut reserved_docs = visible_docs;
    let mut restorations = Vec::new();
    let columns = columns
        .iter()
        .enumerate()
        .map(|(i, col)| {
            if col.agent_doc.is_some() {
                col.trimmed_raw()
            } else if let Some(remembered) = saved_layout.get(i) {
                let remembered = remembered.trim();
                if !remembered.is_empty() && !reserved_docs.contains(remembered) {
                    let remembered = remembered.to_string();
                    reserved_docs.insert(remembered.clone());
                    restorations.push(ColumnMemoryRestoration {
                        column_index: i,
                        remembered: remembered.clone(),
                    });
                    remembered
                } else {
                    col.trimmed_raw()
                }
            } else {
                col.trimmed_raw()
            }
        })
        .collect();
    ColumnMemoryApplication {
        columns,
        restorations,
    }
}

pub fn build_layout_state(
    columns: &[TmuxLayoutColumn],
    saved_layout: &[TmuxLayoutColumn],
) -> Vec<String> {
    let current_docs: Vec<Option<String>> =
        columns.iter().map(|col| col.agent_doc.clone()).collect();
    let mut current_counts = HashMap::new();
    for doc in current_docs.iter().flatten() {
        *current_counts.entry(doc.clone()).or_insert(0usize) += 1;
    }
    let mut duplicate_keepers = HashMap::new();
    for (i, current_doc) in current_docs.iter().enumerate() {
        let Some(current_doc) = current_doc else {
            continue;
        };
        if current_counts.get(current_doc).copied().unwrap_or_default() <= 1 {
            duplicate_keepers.entry(current_doc.clone()).or_insert(i);
            continue;
        }
        if saved_layout
            .get(i)
            .map(|saved| saved.raw.trim() == current_doc)
            .unwrap_or(false)
        {
            duplicate_keepers.entry(current_doc.clone()).or_insert(i);
        }
    }
    for (i, current_doc) in current_docs.iter().enumerate() {
        if let Some(current_doc) = current_doc {
            duplicate_keepers.entry(current_doc.clone()).or_insert(i);
        }
    }
    let mut reserved_docs = HashSet::new();
    current_docs
        .iter()
        .enumerate()
        .map(|(i, current_doc)| {
            if let Some(current_doc) = current_doc {
                let keep_current = duplicate_keepers
                    .get(current_doc)
                    .copied()
                    .is_some_and(|keeper| keeper == i);
                if keep_current && reserved_docs.insert(current_doc.clone()) {
                    return current_doc.clone();
                }
            }
            if let Some(remembered) = saved_layout.get(i) {
                let remembered_raw = remembered.raw.trim();
                if !remembered_raw.is_empty()
                    && remembered.agent_doc.is_some()
                    && reserved_docs.insert(remembered_raw.to_string())
                {
                    return remembered_raw.to_string();
                }
            }
            if let Some(current_doc) = current_doc
                && reserved_docs.insert(current_doc.clone())
            {
                return current_doc.clone();
            }
            String::new()
        })
        .collect()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TmuxFocusOnlyExpansionMode {
    SafePassive,
    LiteralProjection,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum TmuxFocusOnlyExpansionEvent {
    ExactVisibleProjection,
    Expanded { active_column_index: usize },
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxFocusOnlyExpansion {
    pub columns: Vec<String>,
    pub event: Option<TmuxFocusOnlyExpansionEvent>,
}

pub fn expand_focus_only_columns_for_editor_switch(
    columns: &[String],
    remembered_layout: &[String],
    active_column_index: Option<usize>,
    mode: TmuxFocusOnlyExpansionMode,
) -> TmuxFocusOnlyExpansion {
    if !matches!(mode, TmuxFocusOnlyExpansionMode::SafePassive)
        || columns.len() != 1
        || remembered_layout.len() < 2
    {
        return TmuxFocusOnlyExpansion {
            columns: columns.to_vec(),
            event: None,
        };
    }
    let Some(active_column_index) =
        active_column_index.filter(|index| *index < remembered_layout.len())
    else {
        return TmuxFocusOnlyExpansion {
            columns: columns.to_vec(),
            event: None,
        };
    };
    let focused_column = columns[0].trim();
    if focused_column.is_empty() {
        return TmuxFocusOnlyExpansion {
            columns: columns.to_vec(),
            event: None,
        };
    }

    let mut expanded: Vec<String> = remembered_layout
        .iter()
        .map(|col| col.trim().to_string())
        .collect();
    expanded[active_column_index] = focused_column.to_string();
    for (index, col) in expanded.iter_mut().enumerate() {
        if index != active_column_index && col == focused_column {
            col.clear();
        }
    }
    TmuxFocusOnlyExpansion {
        columns: expanded,
        event: Some(TmuxFocusOnlyExpansionEvent::Expanded {
            active_column_index,
        }),
    }
}

pub fn apply_focus_only_expansion_policy(
    columns: &[String],
    remembered_layout: &[String],
    active_column_index: Option<usize>,
    mode: TmuxFocusOnlyExpansionMode,
    exact_visible_projection: bool,
) -> TmuxFocusOnlyExpansion {
    if exact_visible_projection {
        TmuxFocusOnlyExpansion {
            columns: columns.to_vec(),
            event: Some(TmuxFocusOnlyExpansionEvent::ExactVisibleProjection),
        }
    } else {
        expand_focus_only_columns_for_editor_switch(
            columns,
            remembered_layout,
            active_column_index,
            mode,
        )
    }
}

pub fn repair_layout_skips_rescue_phase(
    has_target: bool,
    stash_count: usize,
    has_exact_stash: bool,
) -> bool {
    has_target && (stash_count == 0 || (stash_count == 1 && has_exact_stash))
}

/// Minimum interval between destructive doctor-repair passes for one tmux
/// session. Rapid `agent-doc sync` invocations within this window skip the
/// destructive `repair_layout` window-op sequence while the non-destructive
/// reconciler still runs.
pub const DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS: u64 = 1500;

/// Pure throttle decision: `true` means skip the destructive repair this pass.
///
/// `last_ms` is the timestamp of the previous destructive repair for this
/// session, if any. Throttle only when the previous repair is in the past and
/// strictly within `min_interval_ms` of `now_ms`; a missing or future stamp runs
/// the repair.
pub fn destructive_repair_throttled(
    last_ms: Option<u64>,
    now_ms: u64,
    min_interval_ms: u64,
) -> bool {
    match last_ms {
        Some(last) if now_ms >= last => now_ms - last < min_interval_ms,
        _ => false,
    }
}

pub fn transition_tmux(
    current: &TmuxRealtimeState,
    event: &TmuxObservation,
) -> Option<TmuxRealtimeState> {
    let mut next = current.clone();
    match event {
        TmuxObservation::PaneMissing => next.pane = TmuxPaneActivity::Missing,
        TmuxObservation::PaneExited => next.pane = TmuxPaneActivity::Exited,
        TmuxObservation::PromptReady => next.pane = TmuxPaneActivity::PromptReady,
        TmuxObservation::Busy => next.pane = TmuxPaneActivity::Busy,
        TmuxObservation::OutputAdvanced { watermark } => {
            next.pane = TmuxPaneActivity::OutputActive;
            next.last_output_watermark = Some(watermark.clone());
        }
        TmuxObservation::SupervisorHeartbeat => next.supervisor = TmuxSupervisorHealth::Healthy,
        TmuxObservation::SupervisorStale => next.supervisor = TmuxSupervisorHealth::Stale,
        TmuxObservation::SupervisorCrashed => next.supervisor = TmuxSupervisorHealth::Crashed,
    }
    Some(next)
}

pub struct TmuxModelMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<TmuxRealtimeState, TmuxObservation>,
}

impl TmuxModelMachine {
    pub fn new(initial: TmuxRealtimeState) -> Self {
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_tmux);
        Self { ctx, machine }
    }

    pub fn default_tmux() -> Self {
        Self::new(TmuxRealtimeState::default())
    }

    pub fn send(&self, event: TmuxObservation) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> TmuxRealtimeState {
        self.machine.state(&self.ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn layout_col(raw: &str, agent_doc: Option<&str>) -> TmuxLayoutColumn {
        TmuxLayoutColumn::new(raw, agent_doc.map(str::to_string))
    }

    #[test]
    fn prompt_ready_updates_pane_activity() {
        let machine = TmuxModelMachine::default_tmux();

        assert!(machine.send(TmuxObservation::PromptReady));

        assert_eq!(machine.state().pane, TmuxPaneActivity::PromptReady);
    }

    #[test]
    fn output_advanced_records_watermark() {
        let machine = TmuxModelMachine::default_tmux();

        assert!(machine.send(TmuxObservation::OutputAdvanced {
            watermark: "42".to_string()
        }));

        let state = machine.state();
        assert_eq!(state.pane, TmuxPaneActivity::OutputActive);
        assert_eq!(state.last_output_watermark.as_deref(), Some("42"));
    }

    #[test]
    fn focus_decision_keeps_candidate_when_no_live_owner() {
        assert_eq!(
            decide_focus_pane("%36", None),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    fn focus_decision_keeps_candidate_when_owner_matches() {
        assert_eq!(
            decide_focus_pane("%8", Some("%8")),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    fn focus_decision_repairs_to_live_owner_when_candidate_is_stale() {
        assert_eq!(
            decide_focus_pane("%36", Some("%8")),
            FocusPaneDecision::RepairToLiveOwner("%8".to_string())
        );
    }

    #[test]
    fn focus_decision_ignores_empty_live_owner() {
        assert_eq!(
            decide_focus_pane("%36", Some("")),
            FocusPaneDecision::UseCandidate
        );
    }

    #[test]
    fn pane_position_selection_uses_tmux_geometry() {
        let panes = "%left 0 0 80 24\n%right 120 0 80 24\n%bottom 0 24 160 24\n";

        assert_eq!(
            select_pane_by_position(panes, "left", "test").unwrap(),
            "%left"
        );
        assert_eq!(
            select_pane_by_position(panes, "right", "test").unwrap(),
            "%right"
        );
        assert_eq!(
            select_pane_by_position(panes, "bottom", "test").unwrap(),
            "%bottom"
        );
    }

    #[test]
    fn pane_position_selection_rejects_unknown_direction() {
        let err = select_pane_by_position("%0 0 0 80 24\n%1 80 0 80 24\n", "middle", "test")
            .unwrap_err()
            .to_string();

        assert!(err.contains("invalid position 'middle'"));
    }

    #[test]
    fn pane_geometry_parser_treats_bad_numbers_as_zero() {
        assert_eq!(
            parse_tmux_pane_geometry("%0 bad 3 80 nope\n"),
            vec![TmuxPaneGeometry {
                id: "%0".to_string(),
                left: 0,
                top: 3,
                width: 80,
                height: 0,
            }]
        );
    }

    #[test]
    fn pane_current_command_classifies_bare_shells() {
        for shell in [
            "zsh", "bash", "sh", "fish", "dash", "ksh", "tcsh", "csh", "-zsh", "-bash", " zsh ",
        ] {
            assert!(
                pane_current_command_is_bare_shell(shell),
                "{shell:?} should classify as a bare shell"
            );
        }
        for not_shell in [
            "claude",
            "node",
            "codex",
            "opencode",
            "bun",
            "agent-doc",
            "sleep",
            "cat",
            "vim",
            "",
        ] {
            assert!(
                !pane_current_command_is_bare_shell(not_shell),
                "{not_shell:?} should NOT classify as a bare shell"
            );
        }
    }

    #[test]
    fn column_memory_restores_empty_placeholder_columns() {
        let remembered = vec!["tasks/left.md".to_string(), String::new()];
        let cols = vec![
            layout_col("", None),
            layout_col("tasks/right.md", Some("tasks/right.md")),
        ];

        let applied = apply_column_memory(&cols, &remembered);
        assert_eq!(
            applied.columns,
            vec!["tasks/left.md".to_string(), "tasks/right.md".to_string()],
            "blank editor columns should keep their position long enough to restore remembered panes"
        );
        assert_eq!(
            applied.restorations,
            vec![ColumnMemoryRestoration {
                column_index: 0,
                remembered: "tasks/left.md".to_string()
            }]
        );
    }

    #[test]
    fn column_memory_skips_duplicate_remembered_doc_already_visible_elsewhere() {
        let remembered = vec!["tasks/right.md".to_string(), String::new()];
        let cols = vec![
            layout_col("", None),
            layout_col("tasks/right.md", Some("tasks/right.md")),
        ];

        let applied = apply_column_memory(&cols, &remembered);
        assert_eq!(
            applied.columns,
            vec![String::new(), "tasks/right.md".to_string()],
            "a remembered doc should not be duplicated into an empty sibling column when it is already visible"
        );
        assert!(applied.restorations.is_empty());
    }

    #[test]
    fn build_layout_state_preserves_prior_distinct_doc_when_current_cols_duplicate() {
        let saved_layout = vec![
            layout_col("tasks/left.md", Some("tasks/left.md")),
            layout_col("tasks/right.md", Some("tasks/right.md")),
        ];
        let duplicate_cols = vec![
            layout_col("tasks/right.md", Some("tasks/right.md")),
            layout_col("tasks/right.md", Some("tasks/right.md")),
        ];

        assert_eq!(
            build_layout_state(&duplicate_cols, &saved_layout),
            vec!["tasks/left.md".to_string(), "tasks/right.md".to_string()],
            "duplicate current columns should not overwrite a previously distinct remembered layout"
        );
    }

    #[test]
    fn column_memory_round_trip_persists_and_restores_across_cycles() {
        let left_path = "tasks/left.md".to_string();
        let right_path = "tasks/right.md".to_string();

        let cols_filled = vec![
            layout_col(&left_path, Some(&left_path)),
            layout_col(&right_path, Some(&right_path)),
        ];
        let no_prior = vec![];
        let state_1 = build_layout_state(&cols_filled, &no_prior);
        assert_eq!(state_1, vec![left_path.clone(), right_path.clone()]);

        let cols_empty_left = vec![
            layout_col("", None),
            layout_col(&right_path, Some(&right_path)),
        ];
        let restored = apply_column_memory(&cols_empty_left, &state_1);
        assert_eq!(
            restored.columns,
            vec![left_path.clone(), right_path.clone()],
            "round-trip: empty left column should be restored from prior cycle's layout state"
        );

        let restored_cols = vec![
            layout_col(&left_path, Some(&left_path)),
            layout_col(&right_path, Some(&right_path)),
        ];
        let saved_cols = vec![
            layout_col(&left_path, Some(&left_path)),
            layout_col(&right_path, Some(&right_path)),
        ];
        let state_2 = build_layout_state(&restored_cols, &saved_cols);
        assert_eq!(
            state_2,
            vec![left_path.clone(), right_path.clone()],
            "round-trip: layout state should survive through restore + re-persist"
        );
    }

    #[test]
    fn column_memory_cross_root_doc_restores_from_submodule_path() {
        let root_path = "/repo/tasks/bugs.md".to_string();
        let child_path = "/repo/src/sample-app/tasks/sampleorders.md".to_string();
        let saved = vec![root_path.clone(), child_path.clone()];
        let cols = vec![
            layout_col("", None),
            layout_col(&child_path, Some(&child_path)),
        ];

        let restored = apply_column_memory(&cols, &saved);
        assert_eq!(
            restored.columns,
            vec![root_path.clone(), child_path.clone()],
            "cross-root doc in submodule path should restore from column memory"
        );
    }

    #[test]
    fn column_memory_preserves_right_column_when_left_is_empty() {
        let right_path = "tasks/right.md".to_string();
        let cols = vec![
            layout_col("", None),
            layout_col(&right_path, Some(&right_path)),
        ];
        let no_saved: Vec<String> = vec![];
        let result = apply_column_memory(&cols, &no_saved);
        assert_eq!(
            result.columns,
            vec![String::new(), right_path.clone()],
            "right-column doc must stay in position even with no column memory to restore"
        );

        let state = build_layout_state(&cols, &[]);
        assert_eq!(
            state,
            vec![String::new(), right_path],
            "layout state must preserve column positions including empty slots"
        );
    }

    #[test]
    fn safe_passive_focus_only_switch_expands_active_column_from_memory() {
        let saved_layout = vec!["tasks/left.md".to_string(), "tasks/right.md".to_string()];
        let focused = vec!["tasks/new-left.md".to_string()];

        let expanded = expand_focus_only_columns_for_editor_switch(
            &focused,
            &saved_layout,
            Some(0),
            TmuxFocusOnlyExpansionMode::SafePassive,
        );
        assert_eq!(
            expanded.columns,
            vec![
                "tasks/new-left.md".to_string(),
                "tasks/right.md".to_string()
            ],
            "a focus-only editor switch should replace the active tmux side and keep the sibling side visible"
        );
        assert_eq!(
            expanded.event,
            Some(TmuxFocusOnlyExpansionEvent::Expanded {
                active_column_index: 0
            })
        );

        let full_mode = expand_focus_only_columns_for_editor_switch(
            &focused,
            &saved_layout,
            Some(0),
            TmuxFocusOnlyExpansionMode::LiteralProjection,
        );
        assert_eq!(
            full_mode.columns, focused,
            "manual/full sync keeps the literal editor projection"
        );
        assert!(full_mode.event.is_none());
    }

    #[test]
    fn exact_visible_projection_does_not_expand_from_remembered_focus_only_layout() {
        let saved_layout = vec![
            "tasks/tsift.md".to_string(),
            "tasks/software/corky.md".to_string(),
        ];
        let focused = vec!["tasks/tsift.md".to_string()];

        let expanded = apply_focus_only_expansion_policy(
            &focused,
            &saved_layout,
            Some(0),
            TmuxFocusOnlyExpansionMode::SafePassive,
            false,
        );
        assert_eq!(
            expanded.columns, saved_layout,
            "legacy focus-only sync still preserves remembered sibling columns"
        );

        let exact = apply_focus_only_expansion_policy(
            &focused,
            &saved_layout,
            Some(0),
            TmuxFocusOnlyExpansionMode::SafePassive,
            true,
        );
        assert_eq!(
            exact.columns, focused,
            "editor snapshots marked exact-visible must not reintroduce stale remembered siblings"
        );
        assert_eq!(
            exact.event,
            Some(TmuxFocusOnlyExpansionEvent::ExactVisibleProjection)
        );
    }

    #[test]
    fn destructive_repair_throttled_within_interval_only() {
        let min = DESTRUCTIVE_REPAIR_MIN_INTERVAL_MS;
        assert!(!destructive_repair_throttled(None, 10_000, min));
        assert!(destructive_repair_throttled(Some(10_000), 10_000, min));
        assert!(destructive_repair_throttled(
            Some(10_000),
            10_000 + min - 1,
            min
        ));
        assert!(!destructive_repair_throttled(
            Some(10_000),
            10_000 + min,
            min
        ));
        assert!(!destructive_repair_throttled(
            Some(10_000),
            10_000 + min + 500,
            min
        ));
        assert!(!destructive_repair_throttled(Some(10_000), 9_000, min));
    }

    #[test]
    fn repair_layout_rescue_phase_skips_zero_stash_target_tmuxsynccrash() {
        assert!(
            repair_layout_skips_rescue_phase(true, 0, false),
            "target+zero-stash is already converged for the destructive rescue phase"
        );
        assert!(repair_layout_skips_rescue_phase(true, 1, true));
        assert!(
            !repair_layout_skips_rescue_phase(true, 1, false),
            "a non-canonical single stash still needs normalization through repair"
        );
        assert!(!repair_layout_skips_rescue_phase(false, 0, false));
        assert!(!repair_layout_skips_rescue_phase(true, 2, true));
    }
}
