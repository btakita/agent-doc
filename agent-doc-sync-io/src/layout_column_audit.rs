//! GH #109: validate and attribute the pane that realises each layout column.
//!
//! The layout path (sync → `tmux_router`) used to realise a column with
//! whichever pane the document resolved to, and record nothing about the
//! choice. In the reported case `%66` was a legitimate column pane — it is
//! bound to `pmt2/mr/1102.md`, which the retained desired layout names as a
//! column — but its route-owned supervisor mapped a superseded binary (agent-doc
//! had already titled it `⚠ STALE SUPERVISOR`), and it was swapped out of
//! `1:stash` into the visible window with no log line naming why it was chosen
//! or that it was stale.
//!
//! This module owns four things:
//!
//! 1. [`pane_supervisor_freshness`] — the reusable "does this pane's supervisor
//!    run replaced bytes?" predicate. GH #121: the only witness is the
//!    supervisor's own `/proc/<pid>/exe` naming an unlinked file. The #109
//!    version compared against the observer's resolved binary and fed back
//!    agent-doc's own `⚠ STALE SUPERVISOR` title, and called two healthy
//!    supervisors stale; the title is now diagnostic only.
//! 2. [`gate_stale_column_panes`] — GH #121 (GH #105 ask 2 / GH #109 ask 4):
//!    runs BEFORE `tmux_router`. A column whose own pane is stale is removed
//!    from the router's arguments, so the pane is never selected or promoted out
//!    of the stash, and its safe-boundary recycle is requested. The focused
//!    document is the one exception: its pane is that document's only harness,
//!    so it is admitted under `layout_column_pane_stale_focus_admitted`.
//! 3. [`audit_layout_column_panes`] — run once after `tmux_router` realises the
//!    layout. Every column whose pane moved windows (a stash → layout promotion
//!    in particular) or whose pane runs another document gets one
//!    `layout_column_pane_selected` line naming the candidates and why the
//!    winner was chosen. A stale pane that still reached a column is recorded as
//!    `layout_column_pane_stale_admitted`, never as a plain selection.
//! 4. The stale-supervisor consequence: the pane is neither killed nor reaped.
//!    The existing safe-boundary recycle is requested — the supervisor re-execs
//!    onto the installed build at its next idle boundary, preserving the harness
//!    child and the pane id — and the request now stays live past its TTL while
//!    the supervisor is still stale, so a long open cycle cannot make it lapse.

use crate::sync::{PaneOccupant, pane_occupant_for_document};
use agent_doc_controller::dispatch::is_stash_window_name;
use agent_doc_turn::turn_status::STALE_SUPERVISOR_PANE_MARKER;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::time::{Duration, Instant};
use tmux_router::Tmux;

/// How long one supervisor's safe-boundary recycle request is considered
/// in flight before the layout path asks again. The request is an idempotent
/// marker the supervisor consumes at its idle boundary; this window only keeps
/// every tab switch from re-requesting (and re-checkpointing) the same one.
pub const STALE_COLUMN_RECYCLE_REREQUEST_AFTER: Duration = Duration::from_secs(600);

/// Freshness of the route-owned supervisor running in one pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PaneSupervisorFreshness {
    /// The supervisor maps the installed build. `title_marker` records a
    /// leftover `⚠ STALE SUPERVISOR` title (it lags an in-place re-exec until
    /// the next title refresh); the binary identity outranks it.
    Current {
        supervisor_pid: u32,
        title_marker: bool,
    },
    /// The supervisor runs a superseded build. `evidence` names the witness.
    Stale {
        supervisor_pid: Option<u32>,
        evidence: &'static str,
    },
    /// Not enough evidence either way. Never read as fresh or as stale.
    Unknown { reason: &'static str },
}

impl PaneSupervisorFreshness {
    pub fn is_stale(&self) -> bool {
        matches!(self, Self::Stale { .. })
    }

    pub fn supervisor_pid(&self) -> Option<u32> {
        match self {
            Self::Current { supervisor_pid, .. } => Some(*supervisor_pid),
            Self::Stale { supervisor_pid, .. } => *supervisor_pid,
            Self::Unknown { .. } => None,
        }
    }

    /// Single `supervisor=` log token.
    pub fn log_token(&self) -> String {
        match self {
            Self::Current {
                supervisor_pid,
                title_marker,
            } => format!(
                "current:pid={supervisor_pid}{}",
                if *title_marker {
                    ":title_marker_lag"
                } else {
                    ""
                }
            ),
            Self::Stale {
                supervisor_pid,
                evidence,
            } => format!(
                "stale:pid={}:evidence={evidence}",
                supervisor_pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            ),
            Self::Unknown { reason } => format!("unknown:{reason}"),
        }
    }
}

/// True when a pane title carries agent-doc's own stale-supervisor marker.
pub fn title_has_stale_supervisor_marker(title: &str) -> bool {
    title.trim_start().starts_with(STALE_SUPERVISOR_PANE_MARKER)
}

/// Pure freshness decision over the observed facts.
///
/// GH #121: `binary_replaced` is whether the supervisor's `/proc/<pid>/exe`
/// names an unlinked file (`None` = unobservable) — the bytes it runs were
/// replaced on disk by an install. That is the only witness this path acts on:
///
/// - It is observer-independent. The #109 predicate compared the running inode
///   against the *observer's* resolved binary (`current_exe` first), so a layout
///   sync running a different launchable copy (`.bin` shim → `target/release`,
///   a PyPI wheel, …) called every supervisor on `~/.cargo/bin` stale — two of the
///   three panes it fired on mapped the installed inode with zero `deleted` maps.
/// - It is exactly what a recycle repairs: the supervisor re-execs onto the file
///   now at its launch path. A linked copy that merely differs from the
///   observer's copy would re-exec onto itself, so "requesting" it never has an
///   effect.
///
/// `title_marker` is agent-doc's OWN earlier verdict written into the pane title;
/// feeding it back in let one bad call persist as self-confirming evidence after
/// the condition cleared. It is recorded for diagnostics only and never decides.
pub fn classify_pane_supervisor_freshness(
    supervisor_pid: Option<u32>,
    binary_replaced: Option<bool>,
    title_marker: bool,
) -> PaneSupervisorFreshness {
    match (supervisor_pid, binary_replaced) {
        (Some(pid), Some(true)) => PaneSupervisorFreshness::Stale {
            supervisor_pid: Some(pid),
            evidence: "binary_replaced",
        },
        (Some(pid), Some(false)) => PaneSupervisorFreshness::Current {
            supervisor_pid: pid,
            title_marker,
        },
        (None, _) => PaneSupervisorFreshness::Unknown {
            reason: "no_supervisor_process",
        },
        (Some(_), None) => PaneSupervisorFreshness::Unknown {
            reason: "binary_identity_unobservable",
        },
    }
}

/// GH #121: whether `pid` runs bytes that were replaced (unlinked) on disk.
/// `None` when `/proc/<pid>/exe` cannot be read.
pub fn supervisor_binary_replaced(pid: u32) -> Option<bool> {
    agent_doc_fs::running_exe_build_for_pid(pid).map(|build| build.unlinked)
}

/// Reusable IO predicate: the freshness of the agent-doc supervisor serving
/// `file` inside `pane_id`.
///
/// `known_title` lets a caller that already snapshotted pane titles avoid a
/// second tmux round-trip; `None` reads the title live. Read-only: it never
/// requests a recycle and never touches the pane.
pub fn pane_supervisor_freshness(
    tmux: &Tmux,
    pane_id: &str,
    file: &Path,
    known_title: Option<&str>,
) -> PaneSupervisorFreshness {
    let title_marker = match known_title {
        Some(title) => title_has_stale_supervisor_marker(title),
        None => agent_doc_tmux_io::display_message_value(tmux, Some(pane_id), "#{pane_title}")
            .is_some_and(|title| title_has_stale_supervisor_marker(&title)),
    };
    let supervisor_pid = agent_doc_tmux_io::pane_pid(tmux, pane_id).and_then(|pane_pid| {
        agent_doc_process_owner_io::process_tree_agent_doc_owner_pid_for_file(
            &pane_pid.to_string(),
            &file.to_string_lossy(),
        )
        .and_then(|pid| pid.trim().parse::<u32>().ok())
    });
    let binary_replaced = supervisor_pid.and_then(supervisor_binary_replaced);
    classify_pane_supervisor_freshness(supervisor_pid, binary_replaced, title_marker)
}

/// Where the pane that realised a column came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnPaneSource {
    /// A controller/registry projection sync proved and handed to tmux-router.
    PreResolved,
    /// tmux-router's own durable-registry lookup by session key.
    Registry,
    /// tmux-router's in-memory donor/spare assignment for an unresolved file.
    RouterEphemeral,
}

impl ColumnPaneSource {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::PreResolved => "pre_resolved",
            Self::Registry => "registry",
            Self::RouterEphemeral => "router_ephemeral",
        }
    }
}

/// Pure source attribution.
pub fn classify_column_pane_source(
    pane: &str,
    pre_resolved: Option<&str>,
    registry: Option<&str>,
) -> ColumnPaneSource {
    if pre_resolved == Some(pane) {
        ColumnPaneSource::PreResolved
    } else if registry == Some(pane) {
        ColumnPaneSource::Registry
    } else {
        ColumnPaneSource::RouterEphemeral
    }
}

/// `binding=` log token for a column pane relative to the column's document.
pub fn column_binding_token(occupant: &PaneOccupant) -> String {
    match occupant {
        PaneOccupant::Free => "own".to_string(),
        PaneOccupant::OtherDocument(other) => format!("other_document:{other}"),
        PaneOccupant::ForeignHarness => "foreign_harness".to_string(),
    }
}

/// True when the pane left a stash window for a non-stash window in this pass.
pub fn promoted_from_stash(origin_window: Option<&str>, final_window: Option<&str>) -> bool {
    origin_window.is_some_and(is_stash_window_name)
        && final_window.is_some_and(|window| !is_stash_window_name(window))
}

/// Whether one realised column deserves a selection line. Steady-state syncs,
/// where the column's own fresh pane stayed where it was, stay quiet.
pub fn column_selection_is_notable(
    origin_window: Option<&str>,
    final_window: Option<&str>,
    occupant: &PaneOccupant,
    freshness: &PaneSupervisorFreshness,
) -> bool {
    origin_window != final_window || *occupant != PaneOccupant::Free || freshness.is_stale()
}

/// Pure recycle de-duplication: request when never requested, or when the
/// previous request is older than `window`.
pub fn stale_column_recycle_due(
    last_requested: Option<Instant>,
    now: Instant,
    window: Duration,
) -> bool {
    last_requested.is_none_or(|last| now.saturating_duration_since(last) >= window)
}

fn stale_column_recycle_requests() -> &'static Mutex<HashMap<String, Instant>> {
    static REQUESTS: std::sync::OnceLock<Mutex<HashMap<String, Instant>>> =
        std::sync::OnceLock::new();
    REQUESTS.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Claim the right to request a recycle for `ledger_key` (a supervisor pid,
/// or a pane when no pid was observable) now.
fn claim_stale_column_recycle(ledger_key: &str, now: Instant) -> bool {
    let mut requests = match stale_column_recycle_requests().lock() {
        Ok(requests) => requests,
        Err(poisoned) => {
            eprintln!(
                "[sync] stale-column recycle ledger was poisoned by an earlier panic; recovering it"
            );
            poisoned.into_inner()
        }
    };
    if !stale_column_recycle_due(
        requests.get(ledger_key).copied(),
        now,
        STALE_COLUMN_RECYCLE_REREQUEST_AFTER,
    ) {
        return false;
    }
    requests.insert(ledger_key.to_string(), now);
    true
}

/// GH #121: what the pre-selection gate does with one column's candidate pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnAdmission {
    /// The pane may realise the column.
    Admit,
    /// The pane's supervisor runs replaced bytes: it is ineligible to satisfy
    /// this column until its recycle lands. The column is left unrealised for
    /// this pass (the pane stays wherever it is, typically the stash).
    ExcludeStale,
    /// The stale pane serves the FOCUSED document. It is that document's only
    /// harness, so excluding it would hide the document the operator just
    /// navigated to; it is admitted under a distinct, auditable record and
    /// never as a plain `layout_column_pane_selected`.
    AdmitStaleFocused,
    /// GH #124: the stale pane serves the focused document, but realising it
    /// would stash a pane that is running a live turn. A live agent pane is
    /// never stashed in favour of a stale-supervisor pane, so the focus
    /// exception does not apply: the column is excluded like any stale column
    /// and the next sync after the recycle (or after the turn) admits it.
    ExcludeStaleFocusedLiveTurn,
}

/// GH #124: panes running a live turn that the layout would stash — live-turn
/// panes currently in the target window that realise none of the columns.
pub fn live_turn_panes_displaced(
    live_turn_window_panes: &[String],
    column_panes: &[String],
) -> Vec<String> {
    live_turn_window_panes
        .iter()
        .filter(|pane| !column_panes.contains(pane))
        .cloned()
        .collect()
}

/// GH #124: the focus exception never displaces a live turn. A stale focused
/// pane is admitted only when no live-turn pane would be stashed for it.
pub fn protect_live_turn_from_stale_focus(
    admission: ColumnAdmission,
    displaced_live_turn_panes: &[String],
) -> ColumnAdmission {
    match admission {
        ColumnAdmission::AdmitStaleFocused if !displaced_live_turn_panes.is_empty() => {
            ColumnAdmission::ExcludeStaleFocusedLiveTurn
        }
        other => other,
    }
}

/// Pure admission rule. Only the column's OWN pane is gated here — a pane bound
/// to another document is the foreign-binding audit's concern, not staleness.
pub fn column_admission(
    freshness: &PaneSupervisorFreshness,
    own_pane: bool,
    is_focus: bool,
) -> ColumnAdmission {
    if !own_pane || !freshness.is_stale() {
        ColumnAdmission::Admit
    } else if is_focus {
        ColumnAdmission::AdmitStaleFocused
    } else {
        ColumnAdmission::ExcludeStale
    }
}

/// The observed facts for one column's candidate pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ColumnGateFacts {
    pub file: PathBuf,
    pub pane: String,
    pub freshness: PaneSupervisorFreshness,
    /// The pane is the column's own (not bound to another document).
    pub own_pane: bool,
    pub is_focus: bool,
}

/// Pure gate plan: one admission (plus the live-turn panes it would have
/// displaced) per column fact, in order.
///
/// GH #124: the focused-document exception is decided LAST, against the panes
/// the layout will actually realise. A stale focused pane is admitted only when
/// no pane in the target window running a live turn would be stashed for it —
/// a live agent pane is never stashed in favour of a stale-supervisor pane.
/// `live_turn_window_panes` is consulted only when a stale focused column exists.
pub fn plan_column_admissions(
    facts: &[ColumnGateFacts],
    live_turn_window_panes: &dyn Fn() -> Vec<String>,
) -> Vec<(ColumnAdmission, Vec<String>)> {
    let base: Vec<ColumnAdmission> = facts
        .iter()
        .map(|fact| column_admission(&fact.freshness, fact.own_pane, fact.is_focus))
        .collect();
    if !base.contains(&ColumnAdmission::AdmitStaleFocused) {
        return base.into_iter().map(|admission| (admission, Vec::new())).collect();
    }
    let live = live_turn_window_panes();
    let realised: Vec<String> = facts
        .iter()
        .zip(&base)
        .filter(|(_, admission)| {
            matches!(
                admission,
                ColumnAdmission::Admit | ColumnAdmission::AdmitStaleFocused
            )
        })
        .map(|(fact, _)| fact.pane.clone())
        .collect();
    facts
        .iter()
        .zip(base)
        .map(|(fact, admission)| {
            if admission != ColumnAdmission::AdmitStaleFocused {
                return (admission, Vec::new());
            }
            let others: Vec<String> = realised
                .iter()
                .filter(|pane| **pane != fact.pane)
                .cloned()
                .collect();
            let live_elsewhere: Vec<String> = live
                .iter()
                .filter(|pane| **pane != fact.pane)
                .cloned()
                .collect();
            let displaced = live_turn_panes_displaced(&live_elsewhere, &others);
            (
                protect_live_turn_from_stale_focus(admission, &displaced),
                displaced,
            )
        })
        .collect()
}

fn path_identity(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Pure: drop `excluded` documents from the `--col` arguments (each arg is a
/// comma-separated column), matching on canonical identity so absolute and
/// root-relative spellings agree. A column left empty is dropped.
pub fn col_args_without(col_args: &[String], excluded: &[PathBuf]) -> Vec<String> {
    if excluded.is_empty() {
        return col_args.to_vec();
    }
    let excluded: Vec<PathBuf> = excluded.iter().map(|path| path_identity(path)).collect();
    col_args
        .iter()
        .filter_map(|arg| {
            let kept: Vec<&str> = arg
                .split(',')
                .map(str::trim)
                .filter(|file| !file.is_empty())
                .filter(|file| !excluded.contains(&path_identity(Path::new(file))))
                .collect();
            (!kept.is_empty()).then(|| kept.join(","))
        })
        .collect()
}

/// Pure: the `prior_request=` token describing the recycle request already on
/// the document's ledger, so a re-request says whether the last one was ever
/// consumed instead of repeating `requested` with no outcome (GH #121 ask 4).
pub fn prior_recycle_request_token(prior: Option<(&str, u64)>, now_secs: u64) -> String {
    match prior {
        None => "none".to_string(),
        Some((reason, marked_secs)) => format!(
            "unconsumed:reason={reason}:age_secs={}",
            now_secs.saturating_sub(marked_secs)
        ),
    }
}

fn now_epoch_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Request (de-duplicated) the safe-boundary recycle of a stale column
/// supervisor and return the `action=…` log fragment.
fn request_stale_column_recycle(
    file: &Path,
    pane: &str,
    freshness: &PaneSupervisorFreshness,
    source: &str,
) -> Option<String> {
    let PaneSupervisorFreshness::Stale { supervisor_pid, .. } = freshness else {
        return None;
    };
    let ledger_key = supervisor_pid
        .map(|pid| format!("pid:{pid}"))
        .unwrap_or_else(|| format!("pane:{pane}"));
    if !claim_stale_column_recycle(&ledger_key, Instant::now()) {
        return Some("safe_boundary_recycle_already_requested".to_string());
    }
    let prior =
        agent_doc_supervisor_io::recycle_request::read_recycle_request(&file.to_string_lossy());
    let prior_token = prior_recycle_request_token(
        prior
            .as_ref()
            .map(|request| (request.reason.as_str(), request.requested_secs)),
        now_epoch_secs(),
    );
    let status = agent_doc_controller_io::project_controller::schedule_stale_supervisor_cp_recycle(
        file, source,
    );
    Some(format!(
        "safe_boundary_recycle_requested prior_request={prior_token} recycle_status={status}"
    ))
}

/// Inputs for the pre-selection gate.
pub struct StaleColumnGateInput<'a> {
    /// The `--col` arguments sync is about to hand tmux-router.
    pub col_args: &'a [String],
    /// The focused document, when the caller named one.
    pub focus: Option<&'a str>,
    /// Panes sync proved and is about to hand to tmux-router.
    pub pre_resolved: &'a HashMap<PathBuf, String>,
    /// Durable-registry pane per file, as tmux-router would look it up.
    pub registry_pane: &'a dyn Fn(&Path) -> Option<String>,
    /// Pane → window/title before tmux-router runs.
    pub before: &'a HashMap<String, PaneWindowSnapshot>,
    /// GH #124: panes in the target window running a live turn (fresh
    /// turn-active lease). Evaluated only when a stale focused column needs it.
    pub live_turn_window_panes: &'a dyn Fn() -> Vec<String>,
}

/// GH #121 (GH #105 ask 2 / GH #109 ask 4): make the staleness verdict a
/// precondition of column selection rather than a postscript to it.
///
/// Runs BEFORE tmux-router realises the layout. A column whose own pane runs a
/// replaced supervisor binary is removed from the arguments handed to the
/// router (so the pane is never selected, promoted out of the stash, or counted
/// as a column) and its safe-boundary recycle is requested; once the recycle
/// lands the pane reads fresh and the next sync admits it. The focused document
/// is the one exception (see [`ColumnAdmission::AdmitStaleFocused`]).
///
/// Returns the column arguments tmux-router should realise. Never moves, kills,
/// or reaps a pane.
pub fn gate_stale_column_panes(tmux: &Tmux, input: &StaleColumnGateInput<'_>) -> Vec<String> {
    let _observations = agent_doc_process_owner_io::begin_process_observation_scope();
    let focus = input.focus.map(|focus| path_identity(Path::new(focus)));
    let mut excluded: Vec<PathBuf> = Vec::new();
    let mut facts: Vec<ColumnGateFacts> = Vec::new();
    let mut sources: Vec<Option<String>> = Vec::new();
    for file in agent_doc_tmux::auto_start_candidate_files(input.col_args) {
        let pre_resolved = input.pre_resolved.get(&file).cloned();
        let Some(pane) = pre_resolved
            .clone()
            .or_else(|| (input.registry_pane)(&file))
        else {
            continue;
        };
        let own_pane = pane_occupant_for_document(tmux, &pane, &file) == PaneOccupant::Free;
        let freshness = if own_pane {
            let title = input.before.get(&pane).map(|snapshot| snapshot.title.as_str());
            pane_supervisor_freshness(tmux, &pane, &file, title)
        } else {
            PaneSupervisorFreshness::Unknown {
                reason: "not_document_owner",
            }
        };
        let is_focus = focus.as_ref() == Some(&path_identity(&file));
        facts.push(ColumnGateFacts {
            file,
            pane,
            freshness,
            own_pane,
            is_focus,
        });
        sources.push(pre_resolved);
    }
    let plan = plan_column_admissions(&facts, input.live_turn_window_panes);
    for ((fact, pre_resolved), (admission, displaced)) in facts.iter().zip(sources).zip(plan) {
        let (file, pane, freshness) = (&fact.file, &fact.pane, &fact.freshness);
        let before = input.before.get(pane);
        let (record, admission_token) = match admission {
            ColumnAdmission::Admit => continue,
            ColumnAdmission::ExcludeStale => {
                excluded.push(file.clone());
                ("layout_column_pane_excluded", "excluded".to_string())
            }
            ColumnAdmission::AdmitStaleFocused => (
                "layout_column_pane_stale_focus_admitted",
                "admitted_focused_document_sole_owner".to_string(),
            ),
            ColumnAdmission::ExcludeStaleFocusedLiveTurn => {
                excluded.push(file.clone());
                (
                    "layout_column_pane_excluded",
                    format!(
                        "excluded_focused_live_turn_protected:{}",
                        displaced.join("+")
                    ),
                )
            }
        };
        let source = if pre_resolved.as_deref() == Some(pane.as_str()) {
            ColumnPaneSource::PreResolved
        } else {
            ColumnPaneSource::Registry
        };
        let action = request_stale_column_recycle(file, pane, freshness, "layout_column_gate")
            .unwrap_or_else(|| "none".to_string());
        let line = format!(
            "{record} file={} pane={} source={} window={} supervisor={} admission={admission_token} reason=stale_supervisor action={action} (GH #121)",
            file.display(),
            pane,
            source.as_str(),
            before
                .map(|snapshot| snapshot.window_name.as_str())
                .unwrap_or("unknown"),
            freshness.log_token(),
        );
        // Every pass records the decision in the sync log; the per-document
        // ops log and stderr get it only when the recycle is (re)requested, so a
        // tab-switch storm cannot flood them with the same verdict.
        crate::append_sync_log(&line);
        if action.starts_with("safe_boundary_recycle_requested") {
            eprintln!("[sync] warning: {line}");
            agent_doc_ops_log_io::log_op(file, &line);
        }
    }
    col_args_without(input.col_args, &excluded)
}

/// One tmux pane's window and title, captured in a single `list-panes -a`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneWindowSnapshot {
    pub window_name: String,
    pub title: String,
}

const PANE_SNAPSHOT_FORMAT: &str = "#{pane_id}\t#{window_name}\t#{pane_title}";

/// Parse `list-panes -a -F '#{pane_id}\t#{window_name}\t#{pane_title}'`.
pub fn parse_pane_window_snapshot(output: &str) -> HashMap<String, PaneWindowSnapshot> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(3, '\t');
            let pane = fields.next()?.trim();
            if pane.is_empty() {
                return None;
            }
            let window_name = fields.next().unwrap_or_default().to_string();
            let title = fields.next().unwrap_or_default().to_string();
            Some((pane.to_string(), PaneWindowSnapshot { window_name, title }))
        })
        .collect()
}

/// Snapshot every pane's window and title. An unreachable tmux yields an empty
/// map after logging, so the audit degrades to "origin unknown" rather than
/// guessing.
pub fn snapshot_pane_windows(tmux: &Tmux) -> HashMap<String, PaneWindowSnapshot> {
    match agent_doc_tmux_io::list_panes_all(tmux, PANE_SNAPSHOT_FORMAT) {
        Ok(output) => parse_pane_window_snapshot(&output),
        Err(error) => {
            eprintln!("[sync] layout column audit: could not snapshot tmux panes: {error}");
            HashMap::new()
        }
    }
}

/// Inputs the audit needs from one sync pass.
pub struct LayoutColumnAuditInput<'a> {
    /// `(file, pane)` pairs tmux-router realised.
    pub file_panes: &'a [(PathBuf, String)],
    /// Panes sync proved and handed to tmux-router.
    pub pre_resolved: &'a HashMap<PathBuf, String>,
    /// Durable-registry pane per file, as tmux-router would look it up.
    pub registry_pane: &'a dyn Fn(&Path) -> Option<String>,
    /// Pane → window/title before tmux-router ran.
    pub before: &'a HashMap<String, PaneWindowSnapshot>,
    /// Pane → window/title after tmux-router ran.
    pub after: &'a HashMap<String, PaneWindowSnapshot>,
    /// The focused document, when the caller named one.
    pub focus: Option<&'a str>,
}

/// GH #109: validate and attribute every realised layout column.
///
/// Diagnostic plus the one non-destructive repair the evidence licenses: a
/// stale supervisor in the column's OWN pane gets a safe-boundary recycle
/// request. A pane bound to another document or to a foreign harness is only
/// reported — this audit never touches, moves, or reaps a pane.
pub fn audit_layout_column_panes(tmux: &Tmux, input: &LayoutColumnAuditInput<'_>) {
    // One `/proc` walk serves every column's ownership and supervisor lookup.
    let _observations = agent_doc_process_owner_io::begin_process_observation_scope();
    let focus = input.focus.map(|focus| path_identity(Path::new(focus)));
    for (file, pane) in input.file_panes {
        let before = input.before.get(pane);
        let after = input.after.get(pane);
        let origin_window = before.map(|snapshot| snapshot.window_name.as_str());
        let final_window = after.map(|snapshot| snapshot.window_name.as_str());
        let occupant = pane_occupant_for_document(tmux, pane, file);
        let title = after.or(before).map(|snapshot| snapshot.title.as_str());
        let freshness = if occupant == PaneOccupant::Free {
            pane_supervisor_freshness(tmux, pane, file, title)
        } else {
            // Not this document's supervisor; its freshness says nothing
            // about whether this column is served correctly.
            PaneSupervisorFreshness::Unknown {
                reason: "not_document_owner",
            }
        };
        // GH #121: the pre-selection gate already excluded every stale own pane
        // except the focused document's. Whatever stale pane still reached a
        // column is recorded under its own name — never as a plain selection —
        // and its recycle request is de-duplicated with the gate's.
        let recycle_action =
            request_stale_column_recycle(file, pane, &freshness, "layout_column_selection");
        let recycle_requested_now = recycle_action
            .as_deref()
            .is_some_and(|action| action.starts_with("safe_boundary_recycle_requested"));
        let notable_freshness = if freshness.is_stale() && !recycle_requested_now {
            &PaneSupervisorFreshness::Unknown {
                reason: "stale_recycle_already_requested",
            }
        } else {
            &freshness
        };
        if !column_selection_is_notable(origin_window, final_window, &occupant, notable_freshness) {
            continue;
        }
        let pre_resolved = input.pre_resolved.get(file).map(String::as_str);
        let registry = (input.registry_pane)(file);
        let source = classify_column_pane_source(pane, pre_resolved, registry.as_deref());
        let promoted = promoted_from_stash(origin_window, final_window);
        let is_focus = focus.as_ref() == Some(&path_identity(file));
        let record = if freshness.is_stale() {
            "layout_column_pane_stale_admitted"
        } else {
            "layout_column_pane_selected"
        };
        let admission = if !freshness.is_stale() {
            ""
        } else if is_focus {
            " admission=focused_document_sole_owner"
        } else {
            " admission=ungated_router_choice"
        };
        let selection = format!(
            "{record} file={} pane={} source={} origin_window={} window={} promoted_from_stash={} binding={} supervisor={}{admission} candidates=pre_resolved:{},registry:{} (GH #109)",
            file.display(),
            pane,
            source.as_str(),
            origin_window.unwrap_or("unknown"),
            final_window.unwrap_or("unknown"),
            promoted,
            column_binding_token(&occupant),
            freshness.log_token(),
            pre_resolved.unwrap_or("none"),
            registry.as_deref().unwrap_or("none"),
        );
        crate::append_sync_log(&selection);
        agent_doc_ops_log_io::log_op(file, &selection);

        if occupant != PaneOccupant::Free {
            let violation = format!(
                "layout_column_pane_foreign_document file={} pane={} source={} binding={} promoted_from_stash={} pane_effect=none (GH #109)",
                file.display(),
                pane,
                source.as_str(),
                column_binding_token(&occupant),
                promoted,
            );
            eprintln!("[sync] warning: {violation}");
            crate::append_sync_log(&violation);
            agent_doc_ops_log_io::log_op(file, &violation);
            continue;
        }

        if let PaneSupervisorFreshness::Stale {
            supervisor_pid,
            evidence,
        } = &freshness
        {
            let stale = format!(
                "layout_column_pane_supervisor_stale file={} pane={} supervisor_pid={} evidence={} promoted_from_stash={} source={} action={} pane_effect=none harness_effect=none (GH #109)",
                file.display(),
                pane,
                supervisor_pid
                    .map(|pid| pid.to_string())
                    .unwrap_or_else(|| "unknown".to_string()),
                evidence,
                promoted,
                source.as_str(),
                recycle_action.as_deref().unwrap_or("none"),
            );
            eprintln!("[sync] warning: {stale}");
            crate::append_sync_log(&stale);
            agent_doc_ops_log_io::log_op(file, &stale);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replaced_binary_is_the_only_staleness_witness() {
        assert_eq!(
            classify_pane_supervisor_freshness(Some(42), Some(false), true),
            PaneSupervisorFreshness::Current {
                supervisor_pid: 42,
                title_marker: true
            },
            "a lagging `⚠ STALE SUPERVISOR` title never outranks the mapped binary"
        );
        assert_eq!(
            classify_pane_supervisor_freshness(Some(42), Some(true), false),
            PaneSupervisorFreshness::Stale {
                supervisor_pid: Some(42),
                evidence: "binary_replaced"
            }
        );
        assert_eq!(
            classify_pane_supervisor_freshness(Some(42), Some(true), true),
            PaneSupervisorFreshness::Stale {
                supervisor_pid: Some(42),
                evidence: "binary_replaced"
            },
            "the self-written title adds no independent evidence"
        );
    }

    #[test]
    fn title_marker_is_never_evidence_of_staleness() {
        // GH #121 ask 3: agent-doc wrote that title from an earlier verdict;
        // reading it back made one bad call self-confirming.
        assert_eq!(
            classify_pane_supervisor_freshness(Some(7), None, true),
            PaneSupervisorFreshness::Unknown {
                reason: "binary_identity_unobservable"
            }
        );
        assert_eq!(
            classify_pane_supervisor_freshness(None, None, true),
            PaneSupervisorFreshness::Unknown {
                reason: "no_supervisor_process"
            }
        );
        // Missing evidence is never "fresh".
        assert_eq!(
            classify_pane_supervisor_freshness(Some(7), None, false),
            PaneSupervisorFreshness::Unknown {
                reason: "binary_identity_unobservable"
            }
        );
        assert_eq!(
            classify_pane_supervisor_freshness(None, None, false),
            PaneSupervisorFreshness::Unknown {
                reason: "no_supervisor_process"
            }
        );
    }

    /// Spawn a long-lived process from a private copy of `sleep`, so the test
    /// controls whether its executable is later unlinked.
    #[cfg(target_os = "linux")]
    fn spawn_private_sleep(dir: &Path) -> (std::process::Child, PathBuf) {
        let sleep = ["/usr/bin/sleep", "/bin/sleep"]
            .into_iter()
            .map(PathBuf::from)
            .find(|path| path.is_file())
            .expect("a sleep binary");
        let copy = dir.join("agent-doc");
        std::fs::copy(&sleep, &copy).unwrap();
        // An OLDER build than whatever copy the observer resolves: the #109
        // predicate's directional rule called exactly this shape stale even
        // though the supervisor's own bytes were never replaced.
        std::fs::File::options()
            .write(true)
            .open(&copy)
            .unwrap()
            .set_modified(std::time::UNIX_EPOCH + Duration::from_secs(946_684_800))
            .unwrap();
        let child = std::process::Command::new(&copy).arg("30").spawn().unwrap();
        (child, copy)
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn supervisor_mapping_the_installed_inode_reads_fresh_whatever_the_observer_runs() {
        // GH #121 ask 2: `%32`/`%430` mapped the installed inode with zero
        // `deleted` maps and were still called stale, because the verdict was
        // taken against the OBSERVER's own launchable copy. The predicate now
        // looks only at the supervisor's own mapping.
        let dir = tempfile::TempDir::new().unwrap();
        let (mut child, copy) = spawn_private_sleep(dir.path());
        let pid = child.id();
        let replaced_before = supervisor_binary_replaced(pid);
        // `%33`: an install replaced the bytes it runs (`/proc/pid/exe … (deleted)`).
        std::fs::remove_file(&copy).unwrap();
        let replaced_after = supervisor_binary_replaced(pid);
        let _ = child.kill();
        let _ = child.wait();
        assert_eq!(replaced_before, Some(false));
        assert_eq!(
            classify_pane_supervisor_freshness(Some(pid), replaced_before, true),
            PaneSupervisorFreshness::Current {
                supervisor_pid: pid,
                title_marker: true
            }
        );
        assert_eq!(replaced_after, Some(true));
        assert!(classify_pane_supervisor_freshness(Some(pid), replaced_after, false).is_stale());
    }

    #[test]
    fn stale_own_pane_is_excluded_before_selection_except_for_the_focused_document() {
        // GH #121 ask 1 / GH #105 ask 2 / GH #109 ask 4.
        let stale = classify_pane_supervisor_freshness(Some(3303445), Some(true), true);
        let fresh = classify_pane_supervisor_freshness(Some(2062923), Some(false), false);
        let unknown = classify_pane_supervisor_freshness(Some(1), None, true);
        assert_eq!(
            column_admission(&stale, true, false),
            ColumnAdmission::ExcludeStale
        );
        assert_eq!(
            column_admission(&stale, true, true),
            ColumnAdmission::AdmitStaleFocused
        );
        assert_eq!(
            column_admission(&fresh, true, false),
            ColumnAdmission::Admit
        );
        assert_eq!(
            column_admission(&unknown, true, false),
            ColumnAdmission::Admit,
            "missing evidence never excludes a column"
        );
        assert_eq!(
            column_admission(&stale, false, false),
            ColumnAdmission::Admit,
            "a pane bound to another document is the foreign-binding audit's concern"
        );
    }

    /// GH #124 SimWorld: the measured tmux state, the gate plan, and a model of
    /// tmux-router realising the gated columns (column panes join the window,
    /// every other window pane is stashed, the focused column's pane is active).
    struct Gh124World {
        /// pane -> (document, supervisor replaced on disk)
        panes: Vec<(&'static str, &'static str, bool)>,
        window: Vec<&'static str>,
        live_turn: Vec<&'static str>,
    }

    struct Gh124Outcome {
        window: Vec<String>,
        stashed: Vec<String>,
        active: Option<String>,
        live_turn_queried: usize,
    }

    impl Gh124World {
        fn issue_124() -> Self {
            // `%434` is the agent running the live turn, visible in `agent-doc`;
            // `%33` (1061.md) maps an unlinked binary; `%32` (laptop.md) is fresh.
            Self {
                panes: vec![
                    ("%434", "tasks/agent-doc/bugs.md", false),
                    ("%33", "tasks/pmt2/mr/1061.md", true),
                    ("%32", "tasks/laptop/laptop.md", false),
                ],
                window: vec!["%434"],
                live_turn: vec!["%434"],
            }
        }

        fn pane_for(&self, doc: &str) -> &'static str {
            self.panes.iter().find(|(_, d, _)| *d == doc).unwrap().0
        }

        fn sync(&self, col_args: &[String], focus: &str) -> Gh124Outcome {
            let facts: Vec<ColumnGateFacts> = col_args
                .iter()
                .flat_map(|arg| arg.split(','))
                .map(|doc| {
                    let (pane, _, replaced) =
                        *self.panes.iter().find(|(_, d, _)| *d == doc).unwrap();
                    ColumnGateFacts {
                        file: PathBuf::from(doc),
                        pane: pane.to_string(),
                        freshness: classify_pane_supervisor_freshness(
                            Some(1),
                            Some(replaced),
                            false,
                        ),
                        own_pane: true,
                        is_focus: doc == focus,
                    }
                })
                .collect();
            let queried = std::cell::Cell::new(0usize);
            let live = || {
                queried.set(queried.get() + 1);
                self.window
                    .iter()
                    .filter(|pane| self.live_turn.contains(pane))
                    .map(|pane| pane.to_string())
                    .collect()
            };
            let plan = plan_column_admissions(&facts, &live);
            let excluded: Vec<PathBuf> = facts
                .iter()
                .zip(&plan)
                .filter(|(_, (admission, _))| {
                    matches!(
                        admission,
                        ColumnAdmission::ExcludeStale
                            | ColumnAdmission::ExcludeStaleFocusedLiveTurn
                    )
                })
                .map(|(fact, _)| fact.file.clone())
                .collect();
            let gated = col_args_without(col_args, &excluded);
            if gated.is_empty() {
                // sync.rs preserves the current layout when every column is gated.
                return Gh124Outcome {
                    window: self.window.iter().map(|p| p.to_string()).collect(),
                    stashed: Vec::new(),
                    active: self.window.first().map(|p| p.to_string()),
                    live_turn_queried: queried.get(),
                };
            }
            let window: Vec<String> = gated
                .iter()
                .flat_map(|arg| arg.split(','))
                .map(|doc| self.pane_for(doc).to_string())
                .collect();
            let stashed = self
                .window
                .iter()
                .map(|pane| pane.to_string())
                .filter(|pane| !window.contains(pane))
                .collect();
            let focus_pane = self.pane_for(focus).to_string();
            let active = window
                .contains(&focus_pane)
                .then_some(focus_pane)
                .or_else(|| window.first().cloned());
            Gh124Outcome {
                window,
                stashed,
                active,
                live_turn_queried: queried.get(),
            }
        }

        fn stale(&self, pane: &str) -> bool {
            self.panes.iter().any(|(p, _, replaced)| *p == pane && *replaced)
        }
    }

    #[test]
    fn gh124_live_turn_pane_is_never_stashed_in_favour_of_a_stale_focused_pane() {
        let world = Gh124World::issue_124();
        // The operator focuses 1061.md, whose only pane runs a stale supervisor.
        let outcome = world.sync(&["tasks/pmt2/mr/1061.md".to_string()], "tasks/pmt2/mr/1061.md");
        assert!(
            !outcome.stashed.contains(&"%434".to_string()),
            "the live agent pane was stashed for a stale pane: window={:?} stashed={:?}",
            outcome.window,
            outcome.stashed
        );
        assert!(outcome.window.contains(&"%434".to_string()));
        assert!(
            !outcome.window.iter().any(|pane| world.stale(pane)),
            "no stale-supervisor pane is realised while a live turn would be displaced: {:?}",
            outcome.window
        );
        assert!(
            !outcome.active.as_deref().is_some_and(|pane| world.stale(pane)),
            "a stale-supervisor pane must not be the active pane: {:?}",
            outcome.active
        );
    }

    #[test]
    fn gh124_two_column_layout_never_admits_the_stale_pane_over_a_live_turn() {
        // The measured end state: `0:agent-doc` = laptop `%32` + stale `%33`,
        // `%434` stashed. The stale column must not be the one displacing it.
        let world = Gh124World::issue_124();
        let col_args = vec![
            "tasks/laptop/laptop.md".to_string(),
            "tasks/pmt2/mr/1061.md".to_string(),
        ];
        let outcome = world.sync(&col_args, "tasks/pmt2/mr/1061.md");
        assert_eq!(outcome.window, vec!["%32".to_string()]);
        assert!(!outcome.window.contains(&"%33".to_string()));
        assert_eq!(outcome.active.as_deref(), Some("%32"));
    }

    #[test]
    fn gh124_focus_exception_still_admits_a_stale_pane_when_no_live_turn_is_displaced() {
        // No live turn anywhere: the GH #121 focus exception is unchanged.
        let mut world = Gh124World::issue_124();
        world.live_turn.clear();
        let outcome = world.sync(&["tasks/pmt2/mr/1061.md".to_string()], "tasks/pmt2/mr/1061.md");
        assert_eq!(outcome.window, vec!["%33".to_string()]);
        // The live turn runs in a pane that is itself a realised column.
        let mut world = Gh124World::issue_124();
        world.window = vec!["%434", "%32"];
        world.live_turn = vec!["%32"];
        let col_args = vec![
            "tasks/laptop/laptop.md".to_string(),
            "tasks/pmt2/mr/1061.md".to_string(),
        ];
        let outcome = world.sync(&col_args, "tasks/pmt2/mr/1061.md");
        assert_eq!(outcome.window, vec!["%32".to_string(), "%33".to_string()]);
        // The live turn is in the stale pane itself.
        let mut world = Gh124World::issue_124();
        world.window = vec!["%434", "%33"];
        world.live_turn = vec!["%33"];
        let outcome = world.sync(&["tasks/pmt2/mr/1061.md".to_string()], "tasks/pmt2/mr/1061.md");
        assert_eq!(outcome.window, vec!["%33".to_string()]);
    }

    #[test]
    fn gh124_live_turn_facts_are_read_only_for_a_stale_focused_column() {
        let world = Gh124World::issue_124();
        let outcome = world.sync(
            &["tasks/laptop/laptop.md".to_string()],
            "tasks/laptop/laptop.md",
        );
        assert_eq!(outcome.live_turn_queried, 0);
        let outcome = world.sync(&["tasks/pmt2/mr/1061.md".to_string()], "tasks/pmt2/mr/1061.md");
        assert_eq!(outcome.live_turn_queried, 1);
    }

    #[test]
    fn sync_preserves_the_layout_when_every_column_is_gated_out() {
        // GH #124: tmux-router bails on an empty column set; an all-excluded
        // pass must return before the router instead of erroring or stashing.
        let sync = include_str!("sync.rs");
        let rebind = sync
            .find("let col_args: &[String] = &router_col_args;")
            .expect("the router must receive the gated column set");
        let guard = sync
            .find("if router_col_args.is_empty() && !col_args.is_empty() {")
            .expect("sync must guard an all-excluded column set");
        let router = sync.find("tmux_router::sync_with_options(").unwrap();
        assert!(guard < rebind && rebind < router);
        assert!(sync[guard..rebind].contains("return Ok(());"));
    }

    #[test]
    fn issue_121_three_panes_only_the_replaced_one_is_excluded() {
        // The measured session: `%33` maps an unlinked inode; `%32` and `%430`
        // map the installed inode with zero deleted maps. All three carried a
        // `⚠ STALE SUPERVISOR` title verdict at some point.
        let panes = [
            ("%33", "tasks/pmt2/mr/1061.md", 3303445, true),
            ("%32", "tasks/laptop/laptop.md", 2062923, false),
            ("%430", "tasks/pmt2/tickets/2222.md", 3868191, false),
        ];
        let mut excluded = Vec::new();
        for (_pane, file, pid, replaced) in panes {
            let freshness = classify_pane_supervisor_freshness(Some(pid), Some(replaced), true);
            match column_admission(&freshness, true, false) {
                ColumnAdmission::ExcludeStale => excluded.push(PathBuf::from(file)),
                ColumnAdmission::Admit => assert!(
                    !freshness.log_token().starts_with("stale:"),
                    "an admitted column must never carry supervisor=stale: {}",
                    freshness.log_token()
                ),
                ColumnAdmission::AdmitStaleFocused
                | ColumnAdmission::ExcludeStaleFocusedLiveTurn => unreachable!(),
            }
        }
        assert_eq!(excluded, vec![PathBuf::from("tasks/pmt2/mr/1061.md")]);
        let col_args = vec![
            "tasks/pmt2/mr/1061.md".to_string(),
            "tasks/laptop/laptop.md,tasks/pmt2/tickets/2222.md".to_string(),
        ];
        assert_eq!(
            col_args_without(&col_args, &excluded),
            vec!["tasks/laptop/laptop.md,tasks/pmt2/tickets/2222.md".to_string()],
            "the stale pane's column is never handed to tmux-router"
        );
    }

    #[test]
    fn sync_gates_stale_columns_before_tmux_router_selects_and_realises_them() {
        // GH #121: the #109 audit selected the pane and diagnosed it one second
        // later. The gate's filtered column set must be what the router realises.
        let sync = include_str!("sync.rs");
        let gate = sync
            .find("crate::layout_column_audit::gate_stale_column_panes(")
            .expect("sync must gate stale column panes");
        let router = sync
            .find("tmux_router::sync_with_options(")
            .expect("sync calls tmux-router");
        let rebind = sync
            .find("let col_args: &[String] = &router_col_args;")
            .expect("the router must receive the gated column set");
        assert!(gate < rebind && rebind < router);
    }

    #[test]
    fn col_args_without_matches_canonical_spellings_and_drops_empty_columns() {
        let dir = tempfile::TempDir::new().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        std::fs::write(&a, "").unwrap();
        std::fs::write(&b, "").unwrap();
        let dotted = dir.path().join(".").join("a.md");
        let col_args = vec![
            format!("{},{}", a.display(), b.display()),
            dotted.display().to_string(),
        ];
        assert_eq!(
            col_args_without(&col_args, &[a.clone()]),
            vec![b.display().to_string()]
        );
        assert_eq!(col_args_without(&col_args, &[]), col_args);
    }

    #[test]
    fn prior_request_token_names_an_unconsumed_request() {
        assert_eq!(prior_recycle_request_token(None, 100), "none");
        assert_eq!(
            prior_recycle_request_token(Some(("stale_supervisor_turn_stage", 40)), 8_320),
            "unconsumed:reason=stale_supervisor_turn_stage:age_secs=8280"
        );
    }

    #[test]
    fn stale_title_marker_is_recognised_in_every_title_shape() {
        for (active, stale) in [(true, true), (false, true)] {
            let title = agent_doc_turn::turn_status::pane_title_for_status(active, stale);
            assert!(title_has_stale_supervisor_marker(&title), "{title}");
        }
        for active in [true, false] {
            let title = agent_doc_turn::turn_status::pane_title_for_status(active, false);
            assert!(!title_has_stale_supervisor_marker(&title), "{title}");
        }
        assert!(!title_has_stale_supervisor_marker("custom title"));
    }

    #[test]
    fn issue_109_snapshot_classifies_as_stale_stash_promotion_of_the_columns_own_pane() {
        // The reported evidence: `%66` bound to 1102.md (a retained-layout
        // column), titled `⚠ STALE SUPERVISOR`, supervisor exe inode differs
        // from the installed one, moved from `stash` into `agent-doc`.
        let before = parse_pane_window_snapshot(
            "%66\tstash\t⚠ STALE SUPERVISOR ⟳ agent-doc: turn in progress\n%416\tagent-doc\t⟳ agent-doc: turn in progress\n",
        );
        let after = parse_pane_window_snapshot(
            "%66\tagent-doc\t⚠ STALE SUPERVISOR\n%416\tagent-doc\t⟳ agent-doc: turn in progress\n",
        );
        let origin = before.get("%66").map(|s| s.window_name.as_str());
        let final_window = after.get("%66").map(|s| s.window_name.as_str());
        assert!(promoted_from_stash(origin, final_window));
        let freshness = classify_pane_supervisor_freshness(
            Some(1245655),
            Some(true),
            title_has_stale_supervisor_marker(&after["%66"].title),
        );
        assert!(freshness.is_stale());
        assert_eq!(
            freshness.log_token(),
            "stale:pid=1245655:evidence=binary_replaced"
        );
        assert!(column_selection_is_notable(
            origin,
            final_window,
            &PaneOccupant::Free,
            &freshness
        ));
        // The good pane that stayed put with a fresh supervisor stays quiet.
        let fresh = classify_pane_supervisor_freshness(Some(2926921), Some(false), false);
        assert!(!column_selection_is_notable(
            Some("agent-doc"),
            Some("agent-doc"),
            &PaneOccupant::Free,
            &fresh
        ));
    }

    #[test]
    fn a_pane_bound_to_another_document_is_always_reported_for_the_column() {
        // GH #109 ask 1/2: even a pane that stayed put must be named when the
        // column it fills belongs to a different document.
        let occupant = PaneOccupant::OtherDocument("tasks/pmt2/mr/1102.md".to_string());
        let unknown = PaneSupervisorFreshness::Unknown {
            reason: "not_document_owner",
        };
        assert!(column_selection_is_notable(
            Some("agent-doc"),
            Some("agent-doc"),
            &occupant,
            &unknown
        ));
        assert_eq!(
            column_binding_token(&occupant),
            "other_document:tasks/pmt2/mr/1102.md"
        );
        assert_eq!(
            column_binding_token(&PaneOccupant::ForeignHarness),
            "foreign_harness"
        );
        assert_eq!(column_binding_token(&PaneOccupant::Free), "own");
    }

    #[test]
    fn promotion_requires_leaving_a_stash_window_for_a_visible_one() {
        assert!(promoted_from_stash(Some("stash"), Some("agent-doc")));
        assert!(promoted_from_stash(Some("stash-2"), Some("agent-doc")));
        assert!(!promoted_from_stash(Some("agent-doc"), Some("stash")));
        assert!(!promoted_from_stash(Some("stash"), Some("stash")));
        assert!(!promoted_from_stash(None, Some("agent-doc")));
        assert!(!promoted_from_stash(Some("stash"), None));
    }

    #[test]
    fn column_source_names_why_the_pane_was_chosen() {
        assert_eq!(
            classify_column_pane_source("%66", Some("%66"), Some("%66")),
            ColumnPaneSource::PreResolved
        );
        assert_eq!(
            classify_column_pane_source("%66", None, Some("%66")),
            ColumnPaneSource::Registry
        );
        assert_eq!(
            classify_column_pane_source("%66", Some("%1"), Some("%2")),
            ColumnPaneSource::RouterEphemeral
        );
    }

    #[test]
    fn stale_recycle_is_requested_once_per_window_per_supervisor() {
        let now = Instant::now();
        let window = Duration::from_secs(600);
        assert!(stale_column_recycle_due(None, now, window));
        assert!(!stale_column_recycle_due(Some(now), now, window));
        assert!(stale_column_recycle_due(
            Some(now),
            now + Duration::from_secs(600),
            window
        ));
        // The process-wide ledger applies the same rule.
        let key = "pid:test-gh109-ledger";
        assert!(claim_stale_column_recycle(key, now));
        assert!(!claim_stale_column_recycle(
            key,
            now + Duration::from_secs(1)
        ));
        assert!(claim_stale_column_recycle(
            key,
            now + STALE_COLUMN_RECYCLE_REREQUEST_AFTER
        ));
        assert!(
            claim_stale_column_recycle("pane:%test-gh109", now),
            "a different supervisor/pane has its own window"
        );
    }

    #[test]
    fn pane_snapshot_parser_keeps_titles_with_tabs_and_skips_blank_lines() {
        let parsed = parse_pane_window_snapshot("%1\tagent-doc\ta\tb\n\n%2\tstash\t\n");
        assert_eq!(parsed["%1"].window_name, "agent-doc");
        assert_eq!(parsed["%1"].title, "a\tb");
        assert_eq!(parsed["%2"].window_name, "stash");
        assert_eq!(parsed["%2"].title, "");
        assert_eq!(parsed.len(), 2);
    }
}
