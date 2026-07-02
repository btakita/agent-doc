//! Shared pure tmux model state for agent-doc.
//!
//! This crate owns reusable tmux observations, realtime state, and layout
//! projection policy. It does not execute `tmux` commands, dispatch turns,
//! merge document text, or commit document changes.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fmt,
    path::{Path, PathBuf},
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

/// Determine if the file is in the first column of the editor layout.
///
/// When true, a new agent pane should be split before the existing pane. Returns
/// false when there are fewer than two columns, because there is no meaningful
/// split-before decision to make.
pub fn is_first_column(file: &Path, col_args: &[String]) -> bool {
    if col_args.len() < 2 {
        return false;
    }
    let file_str = file.to_string_lossy();
    col_args
        .first()
        .is_some_and(|first_col| first_col.split(',').any(|f| f.trim() == file_str.as_ref()))
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

pub fn parse_session_window_line(line: &str) -> Option<(String, String, String)> {
    let mut parts = line.splitn(3, ' ');
    let index = parts.next()?.to_string();
    let id = parts.next()?.to_string();
    let name = parts.next()?.to_string();
    Some((index, id, name))
}

pub fn parse_session_windows(text: &str) -> Vec<(String, String, String)> {
    text.lines().filter_map(parse_session_window_line).collect()
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TmuxPaneProcessKind {
    Agent(String),
    IdleShell(String),
    Foreign(String),
    UnknownTransient,
}

pub fn pane_current_command_is_agent_process(cmd: &str) -> bool {
    matches!(cmd, "agent-doc" | "claude" | "codex" | "node")
}

pub fn pane_process_kind_from_current_command(cmd: &str) -> TmuxPaneProcessKind {
    if pane_current_command_is_agent_process(cmd) {
        TmuxPaneProcessKind::Agent(cmd.to_string())
    } else if pane_current_command_is_bare_shell(cmd) {
        TmuxPaneProcessKind::IdleShell(cmd.to_string())
    } else if cmd.is_empty() {
        TmuxPaneProcessKind::UnknownTransient
    } else {
        TmuxPaneProcessKind::Foreign(cmd.to_string())
    }
}

pub fn pane_process_kind_from_current_command_samples<'a>(
    samples: impl IntoIterator<Item = Option<&'a str>>,
) -> TmuxPaneProcessKind {
    let mut first_foreign: Option<String> = None;
    let mut foreign_stable = true;

    for sample in samples {
        let Some(cmd) = sample else {
            foreign_stable = false;
            continue;
        };
        match pane_process_kind_from_current_command(cmd) {
            TmuxPaneProcessKind::Agent(cmd) => return TmuxPaneProcessKind::Agent(cmd),
            TmuxPaneProcessKind::IdleShell(cmd) => return TmuxPaneProcessKind::IdleShell(cmd),
            TmuxPaneProcessKind::Foreign(cmd) => match &first_foreign {
                Some(prev) if prev != &cmd => foreign_stable = false,
                None => first_foreign = Some(cmd),
                _ => {}
            },
            TmuxPaneProcessKind::UnknownTransient => foreign_stable = false,
        }
    }

    match (first_foreign, foreign_stable) {
        (Some(cmd), true) => TmuxPaneProcessKind::Foreign(cmd),
        _ => TmuxPaneProcessKind::UnknownTransient,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum AssociatedPaneSource {
    Registered,
    SessionLog,
    RegistryRebind,
    ProcessTree,
    SupervisorPid,
}

impl AssociatedPaneSource {
    fn as_str(&self) -> &'static str {
        match self {
            Self::Registered => "registered",
            Self::SessionLog => "session-log",
            Self::RegistryRebind => "registry-rebind",
            Self::ProcessTree => "process-tree",
            Self::SupervisorPid => "supervisor-pid",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociatedPaneCandidate {
    pub pane_id: String,
    pub pane_pid: String,
    pub session_name: String,
    pub window_id: String,
    pub window_name: String,
    pub current_command: String,
    pub sources: BTreeSet<AssociatedPaneSource>,
}

impl AssociatedPaneCandidate {
    pub fn is_stash(&self) -> bool {
        self.window_name == "stash" || self.window_name.starts_with("stash-")
    }

    pub fn source_summary(&self) -> String {
        self.sources
            .iter()
            .map(AssociatedPaneSource::as_str)
            .collect::<Vec<_>>()
            .join(",")
    }
}

pub fn associated_pane_candidates_detail<'a>(
    candidates: impl IntoIterator<Item = &'a AssociatedPaneCandidate>,
) -> String {
    candidates
        .into_iter()
        .map(|candidate| {
            format!(
                "{}:{}:{}:{}",
                candidate.pane_id,
                candidate.window_name,
                candidate.window_id,
                candidate.source_summary()
            )
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub fn format_associated_pane_resolution_error(
    file_display: impl fmt::Display,
    candidates: &[AssociatedPaneCandidate],
    preferred_window: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "multiple tmux panes are associated with {}; route cannot safely auto-pick one.",
        file_display
    )];
    if let Some(window_id) = preferred_window {
        lines.push(format!(
            "Preferred active window: {}. Resolve by inspecting one pane, claiming it explicitly, then killing the redundant panes.",
            window_id
        ));
    } else {
        lines.push(
            "Resolve by inspecting one pane, claiming it explicitly, then killing the redundant panes."
                .to_string(),
        );
    }
    for candidate in candidates {
        lines.push(format!(
            "  - {} session={} window={} ({}) cmd={} sources={}",
            candidate.pane_id,
            candidate.session_name,
            candidate.window_id,
            candidate.window_name,
            candidate.current_command,
            candidate.source_summary()
        ));
        lines.push(format!(
            "    view: tmux capture-pane -pt {} | tail -n 80",
            candidate.pane_id
        ));
        lines.push(format!(
            "    assign: agent-doc claim {} --pane {} --force",
            file_display, candidate.pane_id
        ));
        lines.push(format!("    kill: tmux kill-pane -t {}", candidate.pane_id));
    }
    lines.join("\n")
}

pub fn format_associated_pane_selected_error(
    file_display: impl fmt::Display,
    winner: &AssociatedPaneCandidate,
    redundant: &[AssociatedPaneCandidate],
) -> String {
    let mut lines = vec![format!(
        "route found legacy pane-association evidence for {}, but the normal path will not re-elect ownership from {}.",
        file_display, winner.pane_id
    )];
    lines.push(
        "Inspect the candidate, claim it explicitly if it is authoritative, or kill it before rerouting."
            .to_string(),
    );
    lines.push(format!(
        "  - {} session={} window={} ({}) cmd={} sources={}",
        winner.pane_id,
        winner.session_name,
        winner.window_id,
        winner.window_name,
        winner.current_command,
        winner.source_summary()
    ));
    lines.push(format!(
        "    view: tmux capture-pane -pt {} | tail -n 80",
        winner.pane_id
    ));
    lines.push(format!(
        "    assign: agent-doc claim {} --pane {} --force",
        file_display, winner.pane_id
    ));
    lines.push(format!("    kill: tmux kill-pane -t {}", winner.pane_id));
    for candidate in redundant {
        lines.push(format!(
            "  - redundant {} session={} window={} ({}) cmd={} sources={}",
            candidate.pane_id,
            candidate.session_name,
            candidate.window_id,
            candidate.window_name,
            candidate.current_command,
            candidate.source_summary()
        ));
    }
    lines.join("\n")
}

pub fn format_associated_pane_fix_error(
    file_display: impl fmt::Display,
    candidates: &[AssociatedPaneCandidate],
    preferred_window: Option<&str>,
) -> String {
    let mut lines = vec![format!(
        "multiple tmux panes are associated with {}; fix will not auto-pick one.",
        file_display
    )];
    if let Some(window_id) = preferred_window {
        lines.push(format!(
            "Preferred active window: {}. Inspect one pane, claim it explicitly, then kill the redundant panes.",
            window_id
        ));
    } else {
        lines.push(
            "Inspect one pane, claim it explicitly, then kill the redundant panes.".to_string(),
        );
    }
    for candidate in candidates {
        lines.push(format!(
            "  - {} session={} window={} ({}) cmd={} sources={}",
            candidate.pane_id,
            candidate.session_name,
            candidate.window_id,
            candidate.window_name,
            candidate.current_command,
            candidate.source_summary()
        ));
        lines.push(format!(
            "    view: tmux capture-pane -pt {} | tail -n 80",
            candidate.pane_id
        ));
        lines.push(format!(
            "    assign: agent-doc claim {} --pane {} --force",
            file_display, candidate.pane_id
        ));
        lines.push(format!("    kill: tmux kill-pane -t {}", candidate.pane_id));
    }
    lines.join("\n")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssociatedPaneResolution {
    None,
    Selected {
        winner: AssociatedPaneCandidate,
        redundant: Vec<AssociatedPaneCandidate>,
    },
    Ambiguous(Vec<AssociatedPaneCandidate>),
}

pub fn parse_pane_inventory_line(line: &str) -> Option<AssociatedPaneCandidate> {
    let mut parts = line.splitn(6, '\t');
    let pane_id = parts.next()?.trim();
    let pane_pid = parts.next()?.trim();
    let window_id = parts.next()?.trim();
    let window_name = parts.next()?.trim();
    let session_name = parts.next()?.trim();
    let current_command = parts.next()?.trim();
    if pane_id.is_empty() {
        return None;
    }
    Some(AssociatedPaneCandidate {
        pane_id: pane_id.to_string(),
        pane_pid: pane_pid.to_string(),
        session_name: session_name.to_string(),
        window_id: window_id.to_string(),
        window_name: window_name.to_string(),
        current_command: current_command.to_string(),
        sources: BTreeSet::new(),
    })
}

pub fn resolve_associated_panes(
    mut candidates: Vec<AssociatedPaneCandidate>,
    preferred_window: Option<&str>,
) -> AssociatedPaneResolution {
    if candidates.is_empty() {
        return AssociatedPaneResolution::None;
    }
    candidates.sort_by(|left, right| left.pane_id.cmp(&right.pane_id));
    if candidates.len() == 1 {
        return AssociatedPaneResolution::Selected {
            winner: candidates.remove(0),
            redundant: Vec::new(),
        };
    }

    if let Some(window_id) = preferred_window {
        let mut preferred_matches = candidates
            .iter()
            .filter(|candidate| candidate.window_id == window_id)
            .cloned()
            .collect::<Vec<_>>();
        let non_preferred = candidates
            .iter()
            .filter(|candidate| candidate.window_id != window_id)
            .cloned()
            .collect::<Vec<_>>();
        if preferred_matches.len() == 1
            && non_preferred.iter().all(AssociatedPaneCandidate::is_stash)
        {
            let winner = preferred_matches.remove(0);
            let redundant = candidates
                .into_iter()
                .filter(|candidate| candidate.pane_id != winner.pane_id)
                .collect();
            return AssociatedPaneResolution::Selected { winner, redundant };
        }
    }

    let mut stash_matches = candidates
        .iter()
        .filter(|candidate| candidate.is_stash())
        .cloned()
        .collect::<Vec<_>>();
    if stash_matches.len() == 1 && stash_matches.len() == candidates.len() {
        let winner = stash_matches.remove(0);
        let redundant = candidates
            .into_iter()
            .filter(|candidate| candidate.pane_id != winner.pane_id)
            .collect();
        return AssociatedPaneResolution::Selected { winner, redundant };
    }

    AssociatedPaneResolution::Ambiguous(candidates)
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

pub fn classify_sync_layout_columns<F>(
    col_args: &[String],
    mut first_agent_doc_in_col: F,
) -> Vec<TmuxLayoutColumn>
where
    F: FnMut(&str) -> Option<String>,
{
    col_args
        .iter()
        .map(|col| TmuxLayoutColumn::new(col.clone(), first_agent_doc_in_col(col)))
        .collect()
}

fn canonicalize_sync_file(file: &Path) -> Option<PathBuf> {
    let candidate = if file.is_absolute() {
        file.to_path_buf()
    } else {
        std::env::current_dir().ok()?.join(file)
    };
    Some(candidate.canonicalize().unwrap_or(candidate))
}

pub fn same_sync_file(lhs: &str, rhs: &str) -> bool {
    let lhs = lhs.trim();
    let rhs = rhs.trim();
    if lhs.is_empty() || rhs.is_empty() {
        return false;
    }
    if lhs == rhs {
        return true;
    }
    match (
        canonicalize_sync_file(Path::new(lhs)),
        canonicalize_sync_file(Path::new(rhs)),
    ) {
        (Some(lhs), Some(rhs)) => lhs == rhs,
        _ => false,
    }
}

pub fn focused_column_index(remembered_layout: &[String], focus: Option<&str>) -> Option<usize> {
    let focus = focus?.trim();
    if focus.is_empty() {
        return None;
    }
    remembered_layout.iter().position(|col| {
        col.split(',')
            .map(str::trim)
            .any(|candidate| same_sync_file(candidate, focus))
    })
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PruneCleanupMode {
    Full,
    PreserveLiveAgentStashPanes,
    SkipExpensiveStashCleanup,
}

/// `#stash-session-ttl-prune`: one stash pane's candidacy for opt-in TTL reaping.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StashTtlCandidate {
    pub pane_id: String,
    /// Seconds since the pane's window last saw activity (`#{window_activity}`).
    pub idle_secs: u64,
    /// The currently active/visible pane is never reaped, regardless of idle time.
    pub is_active_pane: bool,
    /// Only an agent-doc pane parked in the `stash` window is eligible.
    pub is_agent_doc_stash_pane: bool,
}

/// `#stash-session-ttl-prune`: pure, **non-destructive** decision for whether a
/// stash-parked agent-doc pane is eligible for opt-in TTL reaping. Conservative
/// by construction (see `tasks/agent-doc/plan-stash-session-ttl-prune.md`): the
/// actual `kill-pane` wiring, the idle-signal query, the config knob, and live
/// verification are all gated — this is only the reusable decision core.
///
/// Returns true only when ALL hold: TTL is enabled (`ttl_secs > 0`; `0`/unset
/// disables), the pane is an agent-doc pane parked in the stash window, it is not
/// the active/visible pane, and it has been idle strictly longer than the TTL.
pub fn stash_ttl_prune_candidate(
    idle_secs: u64,
    ttl_secs: u64,
    is_active_pane: bool,
    is_agent_doc_stash_pane: bool,
) -> bool {
    ttl_secs > 0 && is_agent_doc_stash_pane && !is_active_pane && idle_secs > ttl_secs
}

/// Filter a candidate list to the pane ids eligible for TTL reaping. With
/// `ttl_secs == 0` (disabled / unset) this always returns empty, so the feature
/// is inert until explicitly configured.
pub fn stash_ttl_prune_targets(candidates: &[StashTtlCandidate], ttl_secs: u64) -> Vec<String> {
    candidates
        .iter()
        .filter(|c| {
            stash_ttl_prune_candidate(
                c.idle_secs,
                ttl_secs,
                c.is_active_pane,
                c.is_agent_doc_stash_pane,
            )
        })
        .map(|c| c.pane_id.clone())
        .collect()
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
    fn classify_sync_layout_columns_marks_agent_doc_from_callback() {
        let columns = classify_sync_layout_columns(
            &["plain.md".to_string(), "left.md,right.md".to_string()],
            |col| {
                col.split(',')
                    .find(|item| item.trim() == "right.md")
                    .map(str::to_string)
            },
        );

        assert_eq!(columns[0], layout_col("plain.md", None));
        assert_eq!(columns[1], layout_col("left.md,right.md", Some("right.md")));
    }

    #[test]
    fn focused_column_index_matches_exact_and_canonical_paths() {
        let tmp = tempfile::TempDir::new().unwrap();
        let root = tmp.path();
        let left = root.join("tasks/left.md");
        let right = root.join("tasks/right.md");
        std::fs::create_dir_all(left.parent().unwrap()).unwrap();
        std::fs::write(&left, "").unwrap();
        std::fs::write(&right, "").unwrap();
        let saved_layout = vec![
            left.to_string_lossy().to_string(),
            format!("notes.md,{}", right.to_string_lossy()),
        ];

        assert_eq!(
            focused_column_index(&saved_layout, Some(&right.to_string_lossy())),
            Some(1)
        );
        assert_eq!(
            focused_column_index(&saved_layout, Some(&left.to_string_lossy())),
            Some(0)
        );
        assert_eq!(focused_column_index(&saved_layout, Some("   ")), None);
    }

    #[test]
    fn same_sync_file_rejects_empty_paths() {
        assert!(!same_sync_file("", ""));
        assert!(!same_sync_file("left.md", ""));
        assert!(same_sync_file("left.md", "left.md"));
    }

    fn associated_candidate(
        pane_id: &str,
        window_id: &str,
        window_name: &str,
        sources: &[AssociatedPaneSource],
    ) -> AssociatedPaneCandidate {
        AssociatedPaneCandidate {
            pane_id: pane_id.to_string(),
            pane_pid: "100".to_string(),
            session_name: "14".to_string(),
            window_id: window_id.to_string(),
            window_name: window_name.to_string(),
            current_command: "agent-doc".to_string(),
            sources: sources.iter().cloned().collect(),
        }
    }

    fn stash_ttl_candidate(
        pane: &str,
        idle: u64,
        active: bool,
        agentdoc_stash: bool,
    ) -> StashTtlCandidate {
        StashTtlCandidate {
            pane_id: pane.to_string(),
            idle_secs: idle,
            is_active_pane: active,
            is_agent_doc_stash_pane: agentdoc_stash,
        }
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
    fn first_column_requires_layout_context() {
        let file = Path::new("tasks/agent-doc.md");
        assert!(!is_first_column(file, &[]));
        assert!(!is_first_column(file, &["tasks/agent-doc.md".to_string()]));
    }

    #[test]
    fn first_column_detects_first_and_second_columns() {
        let cols = vec![
            "tasks/agent-doc.md".to_string(),
            "tasks/email.md".to_string(),
        ];
        assert!(is_first_column(Path::new("tasks/agent-doc.md"), &cols));
        assert!(!is_first_column(Path::new("tasks/email.md"), &cols));
    }

    #[test]
    fn first_column_accepts_comma_separated_entries() {
        let cols = vec![
            "tasks/agent-doc.md,tasks/corky.md".to_string(),
            "tasks/email.md".to_string(),
        ];
        assert!(is_first_column(Path::new("tasks/agent-doc.md"), &cols));
        assert!(is_first_column(Path::new("tasks/corky.md"), &cols));
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
    fn session_window_parser_reads_tmux_list_windows_output() {
        assert_eq!(
            parse_session_windows("0 @1 agent-doc\n1 @2 stash\n2 @3 notes window\nbad\n"),
            vec![
                ("0".to_string(), "@1".to_string(), "agent-doc".to_string()),
                ("1".to_string(), "@2".to_string(), "stash".to_string()),
                (
                    "2".to_string(),
                    "@3".to_string(),
                    "notes window".to_string()
                ),
            ]
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
    fn pane_process_kind_classifies_current_command() {
        assert!(matches!(
            pane_process_kind_from_current_command("zsh"),
            TmuxPaneProcessKind::IdleShell(cmd) if cmd == "zsh"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command("agent-doc"),
            TmuxPaneProcessKind::Agent(cmd) if cmd == "agent-doc"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command("sleep"),
            TmuxPaneProcessKind::Foreign(cmd) if cmd == "sleep"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command(""),
            TmuxPaneProcessKind::UnknownTransient
        ));
    }

    #[test]
    fn pane_process_kind_samples_require_stable_foreign_command() {
        assert!(matches!(
            pane_process_kind_from_current_command_samples([Some("vim"), Some("vim"), Some("vim")]),
            TmuxPaneProcessKind::Foreign(cmd) if cmd == "vim"
        ));
    }

    #[test]
    fn pane_process_kind_samples_return_unknown_for_empty_or_changing_foreign() {
        assert!(matches!(
            pane_process_kind_from_current_command_samples([Some(""), Some(""), Some("")]),
            TmuxPaneProcessKind::UnknownTransient
        ));
        assert!(matches!(
            pane_process_kind_from_current_command_samples([Some("mv"), Some("sed"), Some("sed")]),
            TmuxPaneProcessKind::UnknownTransient
        ));
        assert!(matches!(
            pane_process_kind_from_current_command_samples([None, Some("vim"), Some("vim")]),
            TmuxPaneProcessKind::UnknownTransient
        ));
    }

    #[test]
    fn pane_process_kind_samples_prefer_agent_or_idle_shell_immediately() {
        assert!(matches!(
            pane_process_kind_from_current_command_samples([Some("mv"), Some("codex")]),
            TmuxPaneProcessKind::Agent(cmd) if cmd == "codex"
        ));
        assert!(matches!(
            pane_process_kind_from_current_command_samples([Some("mv"), Some("bash")]),
            TmuxPaneProcessKind::IdleShell(cmd) if cmd == "bash"
        ));
    }

    #[test]
    fn pane_inventory_parser_reads_tmux_tab_separated_fields() {
        let candidate =
            parse_pane_inventory_line("%419\t22401\t@3\tagent-doc\tsession-a\tcargo").unwrap();

        assert_eq!(candidate.pane_id, "%419");
        assert_eq!(candidate.pane_pid, "22401");
        assert_eq!(candidate.window_id, "@3");
        assert_eq!(candidate.window_name, "agent-doc");
        assert_eq!(candidate.session_name, "session-a");
        assert_eq!(candidate.current_command, "cargo");
        assert!(candidate.sources.is_empty());
    }

    #[test]
    fn pane_inventory_parser_rejects_blank_pane_id() {
        assert!(parse_pane_inventory_line("\t22401\t@3\tagent-doc\tsession-a\tcargo").is_none());
    }

    #[test]
    fn associated_pane_source_summary_is_stable() {
        let candidate = associated_candidate(
            "%419",
            "@3",
            "agent-doc",
            &[
                AssociatedPaneSource::SupervisorPid,
                AssociatedPaneSource::Registered,
                AssociatedPaneSource::ProcessTree,
            ],
        );

        assert_eq!(
            candidate.source_summary(),
            "registered,process-tree,supervisor-pid"
        );
    }

    #[test]
    fn associated_pane_candidates_detail_formats_log_fields() {
        let active = associated_candidate(
            "%419",
            "@3",
            "agent-doc",
            &[
                AssociatedPaneSource::Registered,
                AssociatedPaneSource::SupervisorPid,
            ],
        );
        let stashed =
            associated_candidate("%417", "@9", "stash", &[AssociatedPaneSource::ProcessTree]);

        assert_eq!(
            associated_pane_candidates_detail([&active, &stashed]),
            "%419:agent-doc:@3:registered,supervisor-pid, %417:stash:@9:process-tree"
        );
    }

    #[test]
    fn associated_pane_resolution_error_formats_manual_recovery_steps() {
        let candidates = vec![
            associated_candidate(
                "%419",
                "@3",
                "agent-doc",
                &[AssociatedPaneSource::Registered],
            ),
            associated_candidate("%417", "@9", "stash", &[AssociatedPaneSource::ProcessTree]),
        ];

        let error =
            format_associated_pane_resolution_error("tasks/left.md", &candidates, Some("@3"));

        assert_eq!(
            error,
            [
                "multiple tmux panes are associated with tasks/left.md; route cannot safely auto-pick one.",
                "Preferred active window: @3. Resolve by inspecting one pane, claiming it explicitly, then killing the redundant panes.",
                "  - %419 session=14 window=@3 (agent-doc) cmd=agent-doc sources=registered",
                "    view: tmux capture-pane -pt %419 | tail -n 80",
                "    assign: agent-doc claim tasks/left.md --pane %419 --force",
                "    kill: tmux kill-pane -t %419",
                "  - %417 session=14 window=@9 (stash) cmd=agent-doc sources=process-tree",
                "    view: tmux capture-pane -pt %417 | tail -n 80",
                "    assign: agent-doc claim tasks/left.md --pane %417 --force",
                "    kill: tmux kill-pane -t %417",
            ]
            .join("\n")
        );
    }

    #[test]
    fn associated_pane_selected_error_formats_fail_closed_claim_guidance() {
        let winner = associated_candidate(
            "%419",
            "@3",
            "agent-doc",
            &[
                AssociatedPaneSource::Registered,
                AssociatedPaneSource::SupervisorPid,
            ],
        );
        let redundant = vec![associated_candidate(
            "%417",
            "@9",
            "stash",
            &[AssociatedPaneSource::ProcessTree],
        )];

        let error = format_associated_pane_selected_error("tasks/left.md", &winner, &redundant);

        assert_eq!(
            error,
            [
                "route found legacy pane-association evidence for tasks/left.md, but the normal path will not re-elect ownership from %419.",
                "Inspect the candidate, claim it explicitly if it is authoritative, or kill it before rerouting.",
                "  - %419 session=14 window=@3 (agent-doc) cmd=agent-doc sources=registered,supervisor-pid",
                "    view: tmux capture-pane -pt %419 | tail -n 80",
                "    assign: agent-doc claim tasks/left.md --pane %419 --force",
                "    kill: tmux kill-pane -t %419",
                "  - redundant %417 session=14 window=@9 (stash) cmd=agent-doc sources=process-tree",
            ]
            .join("\n")
        );
    }

    #[test]
    fn associated_pane_fix_error_formats_resync_manual_recovery_steps() {
        let candidates = vec![associated_candidate(
            "%419",
            "@3",
            "agent-doc",
            &[AssociatedPaneSource::Registered],
        )];

        let error = format_associated_pane_fix_error("tasks/left.md", &candidates, Some("@3"));

        assert_eq!(
            error,
            [
                "multiple tmux panes are associated with tasks/left.md; fix will not auto-pick one.",
                "Preferred active window: @3. Inspect one pane, claim it explicitly, then kill the redundant panes.",
                "  - %419 session=14 window=@3 (agent-doc) cmd=agent-doc sources=registered",
                "    view: tmux capture-pane -pt %419 | tail -n 80",
                "    assign: agent-doc claim tasks/left.md --pane %419 --force",
                "    kill: tmux kill-pane -t %419",
            ]
            .join("\n")
        );
    }

    #[test]
    fn resolve_associated_panes_prefers_unique_active_window() {
        let candidates = vec![
            associated_candidate("%417", "@9", "stash", &[AssociatedPaneSource::ProcessTree]),
            associated_candidate(
                "%419",
                "@3",
                "agent-doc",
                &[
                    AssociatedPaneSource::Registered,
                    AssociatedPaneSource::SupervisorPid,
                ],
            ),
        ];

        let resolution = resolve_associated_panes(candidates, Some("@3"));
        match resolution {
            AssociatedPaneResolution::Selected { winner, redundant } => {
                assert_eq!(winner.pane_id, "%419");
                assert_eq!(redundant.len(), 1);
                assert_eq!(redundant[0].pane_id, "%417");
            }
            other => panic!("expected selected winner, got {other:?}"),
        }
    }

    #[test]
    fn resolve_associated_panes_accepts_single_stash_candidate() {
        let candidates = vec![associated_candidate(
            "%420",
            "@9",
            "stash",
            &[AssociatedPaneSource::ProcessTree],
        )];

        let resolution = resolve_associated_panes(candidates, Some("@7"));
        match resolution {
            AssociatedPaneResolution::Selected { winner, redundant } => {
                assert_eq!(winner.pane_id, "%420");
                assert!(redundant.is_empty());
            }
            other => panic!("expected selected stash winner, got {other:?}"),
        }
    }

    #[test]
    fn resolve_associated_panes_reports_ambiguity_when_multiple_candidates_remain() {
        let candidates = vec![
            associated_candidate("%417", "@9", "stash", &[AssociatedPaneSource::ProcessTree]),
            associated_candidate(
                "%419",
                "@3",
                "agent-doc",
                &[AssociatedPaneSource::Registered],
            ),
            associated_candidate(
                "%420",
                "@5",
                "agent-doc",
                &[AssociatedPaneSource::SupervisorPid],
            ),
        ];

        let resolution = resolve_associated_panes(candidates, Some("@7"));
        match resolution {
            AssociatedPaneResolution::Ambiguous(candidates) => {
                assert_eq!(candidates.len(), 3);
            }
            other => panic!("expected ambiguous resolution, got {other:?}"),
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

    #[test]
    fn stash_ttl_disabled_ttl_never_prunes() {
        assert!(!stash_ttl_prune_candidate(99_999, 0, false, true));
        let targets = stash_ttl_prune_targets(&[stash_ttl_candidate("%1", 99_999, false, true)], 0);
        assert!(targets.is_empty(), "disabled TTL must reap nothing");
    }

    #[test]
    fn stash_ttl_active_pane_is_never_reaped() {
        assert!(!stash_ttl_prune_candidate(10_000, 300, true, true));
    }

    #[test]
    fn stash_ttl_only_agent_doc_stash_panes_are_eligible() {
        assert!(!stash_ttl_prune_candidate(10_000, 300, false, false));
    }

    #[test]
    fn stash_ttl_idle_must_strictly_exceed_ttl() {
        assert!(!stash_ttl_prune_candidate(300, 300, false, true));
        assert!(stash_ttl_prune_candidate(301, 300, false, true));
    }

    #[test]
    fn stash_ttl_targets_filters_only_eligible_panes() {
        let candidates = vec![
            stash_ttl_candidate("%idle-old", 1_000, false, true),
            stash_ttl_candidate("%active", 1_000, true, true),
            stash_ttl_candidate("%fresh", 100, false, true),
            stash_ttl_candidate("%not-stash", 1_000, false, false),
        ];
        let targets = stash_ttl_prune_targets(&candidates, 300);
        assert_eq!(targets, vec!["%idle-old".to_string()]);
    }
}
