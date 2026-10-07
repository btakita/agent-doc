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
//! 4. GH #136: the focus exception is bounded. It exists "on the strength of"
//!    a safe-boundary recycle that will make the pane fresh, so a stale pane
//!    still parked in the stash is promoted only while that request is
//!    [`StaleRecycleRequestState::Pending`] and only when promoting it does not
//!    widen the target window. An idle supervisor that left its request
//!    unconsumed past [`STALE_RECYCLE_CONSUME_BOUND_SECS`] is `Overdue`, and its
//!    stash pane stays in the stash. That elapsed bound controls layout admission
//!    only; it never authorises replacement. See [`stale_focus_admission`] for
//!    the transition function and `formal/tla/StaleColumnRecycle.tla` for the
//!    model.
//! 5. The stale-supervisor consequence is owned by the controller. While a turn
//!    is active the durable safe-boundary request remains the only action. Once
//!    the same pane is proven idle, positive `/proc/<pid>/exe` unlinked evidence
//!    authorises exactly one forced continuation replacement for that PID. The
//!    layout gate never kills or reaps a process directly, and no wall-clock age
//!    is an input to the replacement decision.

use crate::sync::{pane_occupant_for_document, PaneOccupant};
use agent_doc_controller::dispatch::is_stash_window_name;
use agent_doc_controller::supervisor_replacement::{
    decide_stale_idle_supervisor_recovery, StaleIdleSupervisorFacts, StaleIdleSupervisorRecovery,
};
pub use agent_doc_supervisor::recycle_request::{
    StaleRecycleRequestState, STALE_RECYCLE_CONSUME_BOUND_SECS,
};
use agent_doc_turn::turn_status::STALE_SUPERVISOR_PANE_MARKER;
use std::collections::{HashMap, HashSet};
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

fn stale_idle_supervisor_replacements() -> &'static Mutex<HashSet<u32>> {
    static REPLACEMENTS: std::sync::OnceLock<Mutex<HashSet<u32>>> = std::sync::OnceLock::new();
    REPLACEMENTS.get_or_init(|| Mutex::new(HashSet::new()))
}

/// GH #136(c): old supervisors cannot consume the durable recycle request that
/// was added after they started. Once `/proc/<pid>/exe` positively proves the
/// binary was replaced and the harness-owned turn marker proves the pane idle,
/// ask the project controller to replace that exact supervisor once. There is
/// intentionally no elapsed-time input to this authorization.
fn request_stale_idle_supervisor_replacement(
    file: &Path,
    pane: &str,
    freshness: &PaneSupervisorFreshness,
    turn_active: bool,
) -> Option<String> {
    // Revalidate the exact process witness at the idle boundary. The decision is
    // deliberately clock-free: time may bound layout admission, never process
    // replacement. The controller remains the sole owner of the lifecycle
    // transition; this path only submits one request for this exact stale PID.
    let PaneSupervisorFreshness::Stale {
        supervisor_pid: Some(pid),
        evidence,
    } = freshness
    else {
        return None;
    };
    let binary_unlinked =
        *evidence == "binary_replaced" && supervisor_binary_replaced(*pid) == Some(true);
    let mut claims = stale_idle_supervisor_replacements()
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    match decide_stale_idle_supervisor_recovery(StaleIdleSupervisorFacts {
        binary_unlinked,
        turn_active,
        replacement_claimed: claims.contains(pid),
    }) {
        StaleIdleSupervisorRecovery::NotStale => {
            Some("controller_replacement_skipped_unproven_stale".to_string())
        }
        StaleIdleSupervisorRecovery::DeferTurnActive => None,
        StaleIdleSupervisorRecovery::AlreadyClaimed => {
            Some("controller_replacement_already_requested".to_string())
        }
        StaleIdleSupervisorRecovery::ReplaceOnce => {
            claims.insert(*pid);
            drop(claims);
            let Some(project_root) = agent_doc_project_root_io::project_root_containing(file)
            else {
                stale_idle_supervisor_replacements()
                    .lock()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .remove(pid);
                return Some("controller_replacement_skipped_no_project_root".to_string());
            };
            let request =
                agent_doc_controller_io::project_controller::SupervisorReplacementRequest {
                    file: file.to_path_buf(),
                    mode: "continue".to_string(),
                    force: true,
                };
            match agent_doc_controller_io::project_controller::request_supervisor_replacement(
                &project_root,
                request,
            ) {
                Ok(receipt) => Some(format!(
                    "controller_replacement_requested:pid={pid}:pane={pane}:receipt={}",
                    receipt.operator_receipt.receipt_id
                )),
                Err(err) => {
                    stale_idle_supervisor_replacements()
                        .lock()
                        .unwrap_or_else(|poisoned| poisoned.into_inner())
                        .remove(pid);
                    Some(format!(
                        "controller_replacement_failed:pid={pid}:error={}",
                        format!("{err:#}").replace('\n', "\\n")
                    ))
                }
            }
        }
    }
}

/// GH #121: what the pre-selection gate does with one column's candidate pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColumnAdmission {
    /// The pane may realise the column.
    Admit,
    /// The pane's supervisor runs replaced bytes, but the pane already realises
    /// this desired column in the target window. Preserve it in place while its
    /// safe-boundary recycle remains pending; excluding it would make the
    /// layout effect stash a live visible session merely because an install
    /// happened during its turn.
    AdmitStaleVisible,
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
    /// GH #136: the stale focused pane is still in the stash and its recycle
    /// request is [`StaleRecycleRequestState::Overdue`]: the supervisor sat idle
    /// past the consumption bound without recycling. The exception was granted
    /// on the strength of that recycle, so it no longer applies; the pane is
    /// not promoted.
    ExcludeStaleFocusedRecycleOverdue,
    /// GH #136: promoting the stale focused pane out of the stash would realise
    /// more columns than the target window holds — the `columns =
    /// observed_panes + 1` flap. A stale stash pane may take the place of a
    /// column, never add one.
    ExcludeStaleFocusedWouldWiden,
}

impl ColumnAdmission {
    /// Whether the column is removed from what tmux-router realises.
    pub fn excludes(self) -> bool {
        !matches!(
            self,
            Self::Admit | Self::AdmitStaleVisible | Self::AdmitStaleFocused
        )
    }
}

/// GH #136: the facts the bounded focus exception decides on, for one stale
/// focused column. Pure data so the transition function can be enumerated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StaleFocusFacts {
    /// The pane is not (provably) in the layout's target window, so admitting
    /// it MOVES it there: a stash pane, or (GH #136 follow-up e) a pane in any
    /// other window.
    pub outside_target_window: bool,
    /// The recycle request is overdue (idle past the consumption bound).
    pub recycle_overdue: bool,
    /// Columns the plan would realise if this pane were admitted.
    pub realised_columns: usize,
    /// Panes currently in the target window, when observed.
    pub window_panes: Option<usize>,
    /// Some live-turn pane in the target window would be stashed for it.
    pub displaces_live_turn: bool,
}

/// GH #136: the bounded focus exception, as a total transition function from
/// the observed facts to an admission. Only ever called for a column whose
/// base admission is [`ColumnAdmission::AdmitStaleFocused`].
///
/// Invariants (enumerated exhaustively in the tests and model-checked in
/// `formal/tla/StaleColumnRecycle.tla`):
///
/// * a pane outside the target window (stash or any other window) is never
///   moved in while its recycle is overdue;
/// * such a pane is never moved in when that widens the target window past
///   `max(window_panes, 1)` (unknown counts as empty);
/// * a live turn is never displaced (GH #124);
/// * a pane already in the target window is never moved by this rule — only
///   moving a pane IN is bounded, so a visible stale pane cannot flap.
pub fn stale_focus_admission(facts: StaleFocusFacts) -> ColumnAdmission {
    if facts.displaces_live_turn {
        return ColumnAdmission::ExcludeStaleFocusedLiveTurn;
    }
    if !facts.outside_target_window {
        return ColumnAdmission::AdmitStaleFocused;
    }
    if facts.recycle_overdue {
        return ColumnAdmission::ExcludeStaleFocusedRecycleOverdue;
    }
    if facts.realised_columns > facts.window_panes.unwrap_or(0).max(1) {
        return ColumnAdmission::ExcludeStaleFocusedWouldWiden;
    }
    ColumnAdmission::AdmitStaleFocused
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
    /// GH #136: the pane sits outside the target window (a stash window, or —
    /// follow-up e — any other window), so admitting it moves it in.
    pub outside_target_window: bool,
    /// GH #136: the recycle request's lifecycle state (meaningful only for a
    /// stale own pane; `NotRequested` otherwise).
    pub recycle: StaleRecycleRequestState,
    /// A fresh harness-owned marker proves this pane is still mid-turn.
    pub turn_active: bool,
}

/// Pure gate plan: one admission (plus the live-turn panes it would have
/// displaced) per column fact, in order.
///
/// GH #124: the focused-document exception is decided LAST, against the panes
/// the layout will actually realise. A stale focused pane is admitted only when
/// no pane in the target window running a live turn would be stashed for it —
/// a live agent pane is never stashed in favour of a stale-supervisor pane.
/// `live_turn_window_panes` is consulted only when a stale focused column exists.
///
/// GH #136: the exception is further bounded by [`stale_focus_admission`].
/// `window_pane_count` (the target window's current pane count) is likewise
/// consulted only when a stale focused column exists, and at most once.
pub fn plan_column_admissions(
    facts: &[ColumnGateFacts],
    window_pane_count: &dyn Fn() -> Option<usize>,
    live_turn_window_panes: &dyn Fn() -> Vec<String>,
) -> Vec<(ColumnAdmission, Vec<String>)> {
    let base: Vec<ColumnAdmission> = facts
        .iter()
        .map(|fact| {
            let admission = column_admission(&fact.freshness, fact.own_pane, fact.is_focus);
            if admission == ColumnAdmission::ExcludeStale && !fact.outside_target_window {
                // A desired pane already in the target window needs no stale
                // promotion exception: preserving it is the zero-movement
                // fixed point. Removing it from the router input would itself
                // move the live session to stash and turn a safe-boundary
                // recycle into an operator-visible layout regression.
                ColumnAdmission::AdmitStaleVisible
            } else {
                admission
            }
        })
        .collect();
    if !base.contains(&ColumnAdmission::AdmitStaleFocused) {
        return base
            .into_iter()
            .map(|admission| (admission, Vec::new()))
            .collect();
    }
    let live = live_turn_window_panes();
    let window_panes = window_pane_count();
    let realised: Vec<String> = facts
        .iter()
        .zip(&base)
        .filter(|(_, admission)| {
            matches!(
                admission,
                ColumnAdmission::Admit
                    | ColumnAdmission::AdmitStaleVisible
                    | ColumnAdmission::AdmitStaleFocused
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
            let admission = stale_focus_admission(StaleFocusFacts {
                outside_target_window: fact.outside_target_window,
                recycle_overdue: fact.recycle.is_overdue(),
                realised_columns: realised.len(),
                window_panes,
                displaces_live_turn: !displaced.is_empty(),
            });
            (admission, displaced)
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

/// The lifecycle state of `file`'s outstanding recycle request, for a
/// supervisor just observed stale (GH #136). One ledger read.
pub fn observe_stale_recycle_request(
    file: &Path,
    consumer_turn_active: bool,
) -> StaleRecycleRequestState {
    let outstanding = agent_doc_supervisor_io::recycle_request::read_outstanding_recycle_request(
        &file.to_string_lossy(),
    );
    agent_doc_supervisor::recycle_request::classify_stale_recycle_request(
        outstanding.as_ref(),
        now_epoch_secs(),
        consumer_turn_active,
        agent_doc_supervisor::recycle_request::stale_recycle_consume_bound_secs(),
    )
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
    prior: Option<&StaleRecycleRequestState>,
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
    // GH #136: the gate already classified the request; the post-router audit
    // has no turn evidence, so it reports the idle classification.
    let prior_token = match prior {
        Some(prior) => prior.log_token(),
        None => observe_stale_recycle_request(file, false).log_token(),
    };
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
    /// The resolved layout target window (`@N`), when known.
    pub target_window: Option<&'a str>,
    /// Panes sync proved and is about to hand to tmux-router.
    pub pre_resolved: &'a HashMap<PathBuf, String>,
    /// Durable-registry pane per file, as tmux-router would look it up.
    pub registry_pane: &'a dyn Fn(&Path) -> Option<String>,
    /// Pane → window/title before tmux-router runs.
    pub before: &'a HashMap<String, PaneWindowSnapshot>,
    /// GH #124: panes in the target window running a live turn (fresh
    /// turn-active lease). Evaluated only when a stale focused column needs it.
    pub live_turn_window_panes: &'a dyn Fn() -> Vec<String>,
    /// GH #136: pane count of the target window. Evaluated only when a stale
    /// focused column needs it.
    pub window_pane_count: &'a dyn Fn() -> Option<usize>,
    /// GH #136: whether one document pane is in an active interaction (its
    /// recycle is then deferred to the interaction boundary, never overdue).
    /// The document is included because its owning controller may live under a
    /// nested project root rather than the sync caller's root. Evaluated only
    /// for a stale own pane.
    pub pane_interaction_active: &'a dyn Fn(&Path, &str) -> bool,
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
/// Returns the column arguments tmux-router should realise, and the documents
/// it excluded (GH #136: the effect's acknowledgement of what it will not
/// build). The gate never directly moves, kills, or reaps a pane; when stale
/// idle recovery is authorized it delegates the lifecycle transition to the
/// project controller.
pub fn gate_stale_column_panes(
    tmux: &Tmux,
    input: &StaleColumnGateInput<'_>,
) -> StaleColumnGateOutcome {
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
            let title = input
                .before
                .get(&pane)
                .map(|snapshot| snapshot.title.as_str());
            pane_supervisor_freshness(tmux, &pane, &file, title)
        } else {
            PaneSupervisorFreshness::Unknown {
                reason: "not_document_owner",
            }
        };
        let is_focus = focus.as_ref() == Some(&path_identity(&file));
        // GH #136 follow-up (e): bounded wherever the pane is parked, not
        // only in a `stash` window.
        let outside_target_window =
            pane_outside_target_window(input.before.get(&pane), input.target_window);
        // GH #136: one ledger read per stale own pane, before this pass makes
        // any request, so the classification reflects what the consumer had.
        let turn_active =
            own_pane && freshness.is_stale() && (input.pane_interaction_active)(&file, &pane);
        let recycle = if own_pane && freshness.is_stale() {
            observe_stale_recycle_request(&file, turn_active)
        } else {
            StaleRecycleRequestState::NotRequested
        };
        facts.push(ColumnGateFacts {
            file,
            pane,
            freshness,
            own_pane,
            is_focus,
            outside_target_window,
            recycle,
            turn_active,
        });
        sources.push(pre_resolved);
    }
    let plan = plan_column_admissions(
        &facts,
        input.window_pane_count,
        input.live_turn_window_panes,
    );
    for ((fact, pre_resolved), (admission, displaced)) in facts.iter().zip(sources).zip(plan) {
        let (file, pane, freshness) = (&fact.file, &fact.pane, &fact.freshness);
        let before = input.before.get(pane);
        let (record, admission_token) = match admission {
            ColumnAdmission::Admit => continue,
            ColumnAdmission::AdmitStaleVisible => (
                "layout_column_pane_stale_visible_preserved",
                "admitted_visible_document".to_string(),
            ),
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
            ColumnAdmission::ExcludeStaleFocusedRecycleOverdue => {
                excluded.push(file.clone());
                (
                    "layout_column_pane_excluded",
                    "excluded_focused_recycle_overdue".to_string(),
                )
            }
            ColumnAdmission::ExcludeStaleFocusedWouldWiden => {
                excluded.push(file.clone());
                (
                    "layout_column_pane_excluded",
                    "excluded_focused_stash_would_widen".to_string(),
                )
            }
        };
        let source = if pre_resolved.as_deref() == Some(pane.as_str()) {
            ColumnPaneSource::PreResolved
        } else {
            ColumnPaneSource::Registry
        };
        let action =
            request_stale_idle_supervisor_replacement(file, pane, freshness, fact.turn_active)
                .or_else(|| {
                    request_stale_column_recycle(
                        file,
                        pane,
                        freshness,
                        "layout_column_gate",
                        Some(&fact.recycle),
                    )
                })
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
        if action.starts_with("safe_boundary_recycle_requested")
            || action.starts_with("controller_replacement_requested")
            || action.starts_with("controller_replacement_failed")
        {
            eprintln!("[sync] warning: {line}");
            agent_doc_ops_log_io::log_op(file, &line);
        }
    }
    StaleColumnGateOutcome {
        col_args: col_args_without(input.col_args, &excluded),
        excluded,
    }
}

/// What the pre-selection gate decided for one sync pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StaleColumnGateOutcome {
    /// The `--col` arguments tmux-router should realise.
    pub col_args: Vec<String>,
    /// The documents removed from them.
    pub excluded: Vec<PathBuf>,
}

/// One tmux pane's window and title, captured in a single `list-panes -a`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PaneWindowSnapshot {
    /// tmux window id (`@N`), the identity the layout target is resolved to.
    pub window_id: String,
    pub window_name: String,
    pub title: String,
}

const PANE_SNAPSHOT_FORMAT: &str = "#{pane_id}\t#{window_id}\t#{window_name}\t#{pane_title}";

/// Parse `list-panes -a -F '#{pane_id}\t#{window_id}\t#{window_name}\t#{pane_title}'`.
pub fn parse_pane_window_snapshot(output: &str) -> HashMap<String, PaneWindowSnapshot> {
    output
        .lines()
        .filter_map(|line| {
            let mut fields = line.splitn(4, '\t');
            let pane = fields.next()?.trim();
            if pane.is_empty() {
                return None;
            }
            let window_id = fields.next().unwrap_or_default().trim().to_string();
            let window_name = fields.next().unwrap_or_default().to_string();
            let title = fields.next().unwrap_or_default().to_string();
            Some((
                pane.to_string(),
                PaneWindowSnapshot {
                    window_id,
                    window_name,
                    title,
                },
            ))
        })
        .collect()
}

/// GH #136 follow-up (e): whether admitting a pane as a column MOVES it into
/// the layout's target window — a stash pane, or a pane in any other window
/// (another session's window, a second agent-doc window, a detached scratch
/// window). The GH #136 bound used to apply to `stash` windows only, so a stale
/// pane parked anywhere else was promoted unbounded, widening the window or
/// riding an overdue request.
///
/// Total and conservative: a pane whose origin is unknown (absent from the
/// snapshot) is NOT proven to be in the target window, so it counts as
/// outside. With no resolved target window only a stash window is known to be
/// elsewhere.
pub fn pane_outside_target_window(
    snapshot: Option<&PaneWindowSnapshot>,
    target_window: Option<&str>,
) -> bool {
    let Some(snapshot) = snapshot else {
        return true;
    };
    if is_stash_window_name(&snapshot.window_name) {
        return true;
    }
    match target_window
        .map(str::trim)
        .filter(|target| !target.is_empty())
    {
        Some(target) if target.starts_with('@') && !snapshot.window_id.is_empty() => {
            snapshot.window_id != target
        }
        Some(target) => snapshot.window_id != target && snapshot.window_name != target,
        None => false,
    }
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
            request_stale_column_recycle(file, pane, &freshness, "layout_column_selection", None);
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

    #[test]
    fn stale_nonfocused_desired_pane_already_in_target_window_is_preserved() {
        // Live repro, 2026-10-05: an install made the supervisor stale while
        // its open-turn pane remained visible. A passive two-column editor
        // observation carried no focus, so excluding the stale column reduced
        // the router input to one pane and stashed the live session.
        let stale = classify_pane_supervisor_freshness(Some(2062336), Some(true), true);
        let fresh = classify_pane_supervisor_freshness(Some(3935160), Some(false), false);
        let facts = vec![
            ColumnGateFacts {
                file: PathBuf::from("tasks/agent-doc/agent-doc-bugs.md"),
                pane: "%2".to_string(),
                freshness: stale,
                own_pane: true,
                is_focus: false,
                outside_target_window: false,
                recycle: StaleRecycleRequestState::Pending {
                    reason: "install_fanout".to_string(),
                    age_secs: 4_820,
                    deferred_by_turn: true,
                },
                turn_active: true,
            },
            ColumnGateFacts {
                file: PathBuf::from("tasks/devops.md"),
                pane: "%125".to_string(),
                freshness: fresh,
                own_pane: true,
                is_focus: false,
                outside_target_window: false,
                recycle: StaleRecycleRequestState::NotRequested,
                turn_active: false,
            },
        ];

        let plan = plan_column_admissions(&facts, &|| Some(2), &|| vec!["%2".to_string()]);

        assert_eq!(plan[0].0, ColumnAdmission::AdmitStaleVisible);
        assert!(!plan[0].0.excludes());
        assert_eq!(plan[1].0, ColumnAdmission::Admit);
        assert_eq!(
            facts
                .iter()
                .zip(&plan)
                .filter(|(_, (admission, _))| !admission.excludes())
                .map(|(fact, _)| fact.pane.as_str())
                .collect::<Vec<_>>(),
            vec!["%2", "%125"],
            "the existing two-pane visible layout must remain a two-pane router input"
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
                        outside_target_window: !self.window.contains(&pane),
                        // A request made this pass: inside its bound.
                        recycle: if replaced {
                            StaleRecycleRequestState::Pending {
                                reason: "stale_supervisor_turn_stage".to_string(),
                                age_secs: 0,
                                deferred_by_turn: false,
                            }
                        } else {
                            StaleRecycleRequestState::NotRequested
                        },
                        turn_active: self.live_turn.contains(&pane),
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
            let window_panes = || Some(self.window.len());
            let plan = plan_column_admissions(&facts, &window_panes, &live);
            let excluded: Vec<PathBuf> = facts
                .iter()
                .zip(&plan)
                .filter(|(_, (admission, _))| admission.excludes())
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
            self.panes
                .iter()
                .any(|(p, _, replaced)| *p == pane && *replaced)
        }
    }

    #[test]
    fn gh124_live_turn_pane_is_never_stashed_in_favour_of_a_stale_focused_pane() {
        let world = Gh124World::issue_124();
        // The operator focuses 1061.md, whose only pane runs a stale supervisor.
        let outcome = world.sync(
            &["tasks/pmt2/mr/1061.md".to_string()],
            "tasks/pmt2/mr/1061.md",
        );
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
            !outcome
                .active
                .as_deref()
                .is_some_and(|pane| world.stale(pane)),
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
        let outcome = world.sync(
            &["tasks/pmt2/mr/1061.md".to_string()],
            "tasks/pmt2/mr/1061.md",
        );
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
        let outcome = world.sync(
            &["tasks/pmt2/mr/1061.md".to_string()],
            "tasks/pmt2/mr/1061.md",
        );
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
        let outcome = world.sync(
            &["tasks/pmt2/mr/1061.md".to_string()],
            "tasks/pmt2/mr/1061.md",
        );
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
            .find("if gated_layout_decision(")
            .expect("sync must consult the typed gated-layout decision");
        let router = sync.find("tmux_router::sync_with_options(").unwrap();
        assert!(guard < rebind && rebind < router);
        assert!(sync[guard..rebind].contains("== GatedLayoutDecision::PreserveCurrent"));
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
                ColumnAdmission::AdmitStaleVisible
                | ColumnAdmission::AdmitStaleFocused
                | ColumnAdmission::ExcludeStaleFocusedLiveTurn
                | ColumnAdmission::ExcludeStaleFocusedRecycleOverdue
                | ColumnAdmission::ExcludeStaleFocusedWouldWiden => unreachable!(),
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
            col_args_without(&col_args, std::slice::from_ref(&a)),
            vec![b.display().to_string()]
        );
        assert_eq!(col_args_without(&col_args, &[]), col_args);
    }

    // ---------------------------------------------------------------------
    // GH #136: the bounded focus exception.
    // ---------------------------------------------------------------------

    /// Exhaustive transition table: every combination of the facts the
    /// bounded exception decides on, checked against each invariant.
    #[test]
    fn gh136_stale_focus_admission_transition_table_is_exhaustive() {
        let mut cases = 0usize;
        for outside_target_window in [false, true] {
            for recycle_overdue in [false, true] {
                for displaces_live_turn in [false, true] {
                    for realised_columns in 1..=5usize {
                        for window_panes in [None, Some(0), Some(1), Some(2), Some(3), Some(4)] {
                            cases += 1;
                            let facts = StaleFocusFacts {
                                outside_target_window,
                                recycle_overdue,
                                realised_columns,
                                window_panes,
                                displaces_live_turn,
                            };
                            let admission = stale_focus_admission(facts);
                            let admitted = admission == ColumnAdmission::AdmitStaleFocused;
                            // The function only ever answers within the
                            // focused-stale family.
                            assert!(
                                matches!(
                                    admission,
                                    ColumnAdmission::AdmitStaleFocused
                                        | ColumnAdmission::ExcludeStaleFocusedLiveTurn
                                        | ColumnAdmission::ExcludeStaleFocusedRecycleOverdue
                                        | ColumnAdmission::ExcludeStaleFocusedWouldWiden
                                ),
                                "{facts:?}"
                            );
                            // I1: a live turn is never displaced.
                            assert!(!(admitted && displaces_live_turn), "{facts:?}");
                            // I2: never promoted out of the stash while overdue.
                            assert!(
                                !(admitted && outside_target_window && recycle_overdue),
                                "{facts:?}"
                            );
                            // I3: a stash promotion never widens the window.
                            assert!(
                                !(admitted
                                    && outside_target_window
                                    && realised_columns > window_panes.unwrap_or(0).max(1)),
                                "{facts:?}"
                            );
                            // I4: a visible pane is never moved by this rule
                            // (no flap): only the live-turn guard can exclude it.
                            if !outside_target_window && !displaces_live_turn {
                                assert!(admitted, "{facts:?}");
                            }
                            // Liveness of the exception: a stash pane with a
                            // pending request that replaces a column is admitted.
                            if outside_target_window
                                && !recycle_overdue
                                && !displaces_live_turn
                                && realised_columns <= window_panes.unwrap_or(0).max(1)
                            {
                                assert!(admitted, "{facts:?}");
                            }
                        }
                    }
                }
            }
        }
        assert_eq!(cases, 2 * 2 * 2 * 5 * 6);
    }

    /// GH #136 SimWorld: tmux panes, the gate plan, and tmux-router realising
    /// the gated columns, as in [`Gh124World`], plus the recycle request
    /// lifecycle driven by a clock and a consumer.
    struct Gh136World {
        /// pane -> document
        panes: Vec<(&'static str, &'static str)>,
        /// Panes in `0:agent-doc`, in order.
        window: Vec<&'static str>,
        /// The supervisor of this pane runs replaced bytes.
        stale_pane: &'static str,
        stale: bool,
        /// Seconds since the FIRST unconsumed recycle request (None: none).
        request_age: Option<u64>,
        stale_pane_interaction_active: bool,
        window_count_queries: std::cell::Cell<usize>,
    }

    impl Gh136World {
        /// The measured shape: `0:agent-doc` holds 2 panes, `%41` (1099.md)
        /// is a stale supervisor in `stash`.
        fn issue_136() -> Self {
            Self {
                panes: vec![
                    ("%434", "tasks/agent-doc/agent-doc.ad.md"),
                    ("%430", "tasks/pmt2/tickets/2222.md"),
                    ("%41", "tasks/pmt2/mr/1099.md"),
                ],
                window: vec!["%434", "%430"],
                stale_pane: "%41",
                stale: true,
                request_age: Some(31_622),
                stale_pane_interaction_active: false,
                window_count_queries: std::cell::Cell::new(0),
            }
        }

        fn pane_for(&self, doc: &str) -> &'static str {
            self.panes.iter().find(|(_, d)| *d == doc).unwrap().0
        }

        fn recycle_state(&self) -> StaleRecycleRequestState {
            let outstanding = self.request_age.map(|age| {
                agent_doc_supervisor::recycle_request::OutstandingRecycleRequest {
                    latest: agent_doc_supervisor::recycle_request::recycle_request(
                        "stale_supervisor_turn_stage",
                        1_000_000,
                    ),
                    first_requested_secs: 1_000_000 - age,
                }
            });
            agent_doc_supervisor::recycle_request::classify_stale_recycle_request(
                outstanding.as_ref(),
                1_000_000,
                self.stale_pane_interaction_active,
                STALE_RECYCLE_CONSUME_BOUND_SECS,
            )
        }

        /// One sync pass; returns the realised window (or the preserved one
        /// when every column was gated out, as sync.rs does).
        fn sync(&self, col_args: &[String], focus: &str) -> Vec<String> {
            let facts: Vec<ColumnGateFacts> = col_args
                .iter()
                .flat_map(|arg| arg.split(','))
                .map(|doc| {
                    let pane = self.pane_for(doc);
                    let stale = self.stale && pane == self.stale_pane;
                    ColumnGateFacts {
                        file: PathBuf::from(doc),
                        pane: pane.to_string(),
                        freshness: classify_pane_supervisor_freshness(
                            Some(20488),
                            Some(stale),
                            stale,
                        ),
                        own_pane: true,
                        is_focus: doc == focus,
                        outside_target_window: !self.window.contains(&pane),
                        recycle: if stale {
                            self.recycle_state()
                        } else {
                            StaleRecycleRequestState::NotRequested
                        },
                        turn_active: stale && self.stale_pane_interaction_active,
                    }
                })
                .collect();
            let window_panes = || {
                self.window_count_queries
                    .set(self.window_count_queries.get() + 1);
                Some(self.window.len())
            };
            let plan = plan_column_admissions(&facts, &window_panes, &Vec::new);
            let excluded: Vec<PathBuf> = facts
                .iter()
                .zip(&plan)
                .filter(|(_, (admission, _))| admission.excludes())
                .map(|(fact, _)| fact.file.clone())
                .collect();
            let gated = col_args_without(col_args, &excluded);
            if gated.is_empty() {
                return self.window.iter().map(|p| p.to_string()).collect();
            }
            gated
                .iter()
                .flat_map(|arg| arg.split(','))
                .map(|doc| self.pane_for(doc).to_string())
                .collect()
        }
    }

    fn cols(docs: &[&str]) -> Vec<String> {
        docs.iter().map(|doc| doc.to_string()).collect()
    }

    #[test]
    fn gh136_stale_stash_pane_never_widens_the_window() {
        // The editor asks for three columns with 1099.md focused; the window
        // holds two panes. Promoting `%41` was the `columns = observed + 1` flap.
        let mut world = Gh136World::issue_136();
        world.request_age = Some(0); // even a fresh request may not widen
        let three = cols(&[
            "tasks/pmt2/mr/1099.md",
            "tasks/agent-doc/agent-doc.ad.md",
            "tasks/pmt2/tickets/2222.md",
        ]);
        let window = world.sync(&three, "tasks/pmt2/mr/1099.md");
        assert!(!window.contains(&"%41".to_string()), "{window:?}");
        assert!(window.len() <= world.window.len(), "{window:?}");
        // Replacing a column is still allowed while the request is pending.
        let two = cols(&["tasks/pmt2/mr/1099.md", "tasks/agent-doc/agent-doc.ad.md"]);
        assert_eq!(
            world.sync(&two, "tasks/pmt2/mr/1099.md"),
            vec!["%41".to_string(), "%434".to_string()]
        );
        assert_eq!(
            world.window_count_queries.get(),
            2,
            "the window is counted at most once per pass"
        );
    }

    #[test]
    fn gh136_overdue_recycle_stops_the_stash_promotion() {
        // The recorded admission: `%41` stale, request unconsumed for 31,622s
        // while idle. It may no longer be promoted on the strength of it.
        let world = Gh136World::issue_136();
        assert!(world.recycle_state().is_overdue());
        let two = cols(&["tasks/pmt2/mr/1099.md", "tasks/agent-doc/agent-doc.ad.md"]);
        let window = world.sync(&two, "tasks/pmt2/mr/1099.md");
        assert_eq!(window, vec!["%434".to_string()]);
        // An active interaction in the stale pane (including an operator
        // permission prompt after its short turn marker expires) defers its
        // recycle: not overdue, admitted.
        let mut busy = Gh136World::issue_136();
        busy.stale_pane_interaction_active = true;
        assert!(!busy.recycle_state().is_overdue());
        assert_eq!(
            busy.sync(&two, "tasks/pmt2/mr/1099.md"),
            vec!["%41".to_string(), "%434".to_string()]
        );
        // Already visible: never moved by the bound (no flap).
        let mut visible = Gh136World::issue_136();
        visible.window = vec!["%434", "%41"];
        assert_eq!(
            visible.sync(&two, "tasks/pmt2/mr/1099.md"),
            vec!["%41".to_string(), "%434".to_string()]
        );
    }

    /// GH #136 time evolution: drive the request lifecycle with a clock, an
    /// operator switching tabs between 1099.md and another document every 50s
    /// (so `%41` keeps returning to the stash), and a consumer that either
    /// consumes at a given tick or never does (an old binary stuck deferring).
    /// At every tick:
    ///
    /// * `%41` is promoted out of the stash while stale only when its FIRST
    ///   unconsumed request is within the bound — refreshes do not extend it;
    /// * no pass realises more columns than the editor asked for or the window
    ///   held (no widening from the stash);
    /// * once a promotion is refused as overdue, no later promotion happens
    ///   while the supervisor is still stale (no flap);
    /// * after consumption the pane is promoted as a plain fresh column.
    #[test]
    fn gh136_request_lifecycle_has_no_flap_and_a_bounded_admission_window() {
        let with_1099 = cols(&["tasks/pmt2/mr/1099.md", "tasks/agent-doc/agent-doc.ad.md"]);
        let without = cols(&[
            "tasks/pmt2/tickets/2222.md",
            "tasks/agent-doc/agent-doc.ad.md",
        ]);
        let bound = STALE_RECYCLE_CONSUME_BOUND_SECS;
        for consume_at in [
            Some(30u64),
            Some(120),
            Some(121),
            Some(160),
            Some(5_000),
            None,
        ] {
            let mut world = Gh136World::issue_136();
            world.request_age = None;
            let mut first_request: Option<u64> = None;
            let mut refused_overdue_at: Option<u64> = None;
            let mut fresh_promotions = 0usize;
            for now in 0..=3_600u64 {
                let focus_1099 = (now / 50) % 2 == 0;
                let (col_args, focus) = if focus_1099 {
                    (&with_1099, "tasks/pmt2/mr/1099.md")
                } else {
                    (&without, "tasks/pmt2/tickets/2222.md")
                };
                if consume_at == Some(now) {
                    world.stale = false;
                    first_request = None;
                }
                if world.stale && focus_1099 {
                    // The gate requests on the first stale pass; later passes
                    // refresh it but never move the first-unconsumed time.
                    first_request.get_or_insert(now);
                }
                world.request_age = first_request.map(|first| now - first);
                let was_stashed = !world.window.contains(&"%41");
                let window = world.sync(col_args, focus);
                assert!(
                    window.len() <= col_args.len().max(world.window.len()),
                    "widened at {now}: {window:?}"
                );
                let promoted = was_stashed && window.contains(&"%41".to_string());
                if world.stale && promoted {
                    let age = world.request_age.unwrap();
                    assert!(age <= bound, "stale stash pane promoted at age {age}");
                    assert!(refused_overdue_at.is_none(), "flap at {now}");
                }
                if world.stale && focus_1099 && was_stashed && !promoted {
                    refused_overdue_at.get_or_insert(now);
                }
                if !world.stale && promoted {
                    fresh_promotions += 1;
                }
                world.window = window
                    .iter()
                    .map(|pane| match pane.as_str() {
                        "%41" => "%41",
                        "%434" => "%434",
                        _ => "%430",
                    })
                    .collect();
            }
            match consume_at {
                // Consumed before the first refocus that would read it overdue.
                Some(at) if at <= 100 + 1 => {
                    assert!(refused_overdue_at.is_none(), "consume_at={at}")
                }
                // The first refocus after the bound (t=100 is inside it; the
                // next is t=200) is refused and nothing promotes it again.
                Some(at) if at <= 200 => {
                    assert!(refused_overdue_at.is_none(), "consume_at={at}")
                }
                _ => assert_eq!(refused_overdue_at, Some(200), "consume_at={consume_at:?}"),
            }
            if consume_at.is_some_and(|at| at < 3_600) {
                assert!(fresh_promotions > 0, "consume_at={consume_at:?}");
            }
        }
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
            "%66\t@2\tstash\t⚠ STALE SUPERVISOR ⟳ agent-doc: turn in progress\n%416\t@1\tagent-doc\t⟳ agent-doc: turn in progress\n",
        );
        let after = parse_pane_window_snapshot(
            "%66\t@1\tagent-doc\t⚠ STALE SUPERVISOR\n%416\t@1\tagent-doc\t⟳ agent-doc: turn in progress\n",
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
        let parsed = parse_pane_window_snapshot("%1\t@1\tagent-doc\ta\tb\n\n%2\t@2\tstash\t\n");
        assert_eq!(parsed["%1"].window_id, "@1");
        assert_eq!(parsed["%1"].window_name, "agent-doc");
        assert_eq!(parsed["%1"].title, "a\tb");
        assert_eq!(parsed["%2"].window_name, "stash");
        assert_eq!(parsed["%2"].title, "");
        assert_eq!(parsed.len(), 2);
    }

    // ---------------------------------------------------------------------
    // GH #136 follow-up (e): stale panes in non-`stash` windows.
    // ---------------------------------------------------------------------

    fn snapshot(window_id: &str, window_name: &str) -> PaneWindowSnapshot {
        PaneWindowSnapshot {
            window_id: window_id.to_string(),
            window_name: window_name.to_string(),
            title: String::new(),
        }
    }

    #[test]
    fn gh136e_every_window_but_the_target_counts_as_outside() {
        let target = Some("@1");
        // The target window itself: admitting the pane moves nothing.
        assert!(!pane_outside_target_window(
            Some(&snapshot("@1", "agent-doc")),
            target
        ));
        // A stash window, whatever the target.
        assert!(pane_outside_target_window(
            Some(&snapshot("@2", "stash")),
            target
        ));
        assert!(pane_outside_target_window(
            Some(&snapshot("@2", "stash")),
            None
        ));
        // Any OTHER non-stash window — the case GH #136 left unbounded.
        assert!(pane_outside_target_window(
            Some(&snapshot("@7", "scratch")),
            target
        ));
        assert!(pane_outside_target_window(
            Some(&snapshot("@9", "agent-doc")),
            target
        ));
        // Unknown origin is not proof of visibility.
        assert!(pane_outside_target_window(None, target));
        // A name-form target still matches by name.
        assert!(!pane_outside_target_window(
            Some(&snapshot("@1", "agent-doc")),
            Some("agent-doc")
        ));
        // No resolved target: only a stash window is known to be elsewhere.
        assert!(!pane_outside_target_window(
            Some(&snapshot("@7", "scratch")),
            None
        ));
    }

    /// The GH #136 world with the stale `%41` parked in a non-stash window
    /// (`@7 scratch`) instead of `stash`. The bound must be identical: no
    /// widening, no promotion on an overdue request, and a pane already in
    /// the target window is never moved by it.
    #[test]
    fn gh136e_stale_pane_in_a_non_stash_window_is_bounded_like_a_stash_pane() {
        let before: HashMap<String, PaneWindowSnapshot> = [
            ("%434".to_string(), snapshot("@1", "agent-doc")),
            ("%430".to_string(), snapshot("@1", "agent-doc")),
            ("%41".to_string(), snapshot("@7", "scratch")),
        ]
        .into_iter()
        .collect();
        let target = Some("@1");
        let window_panes = || Some(2usize);
        let fact = |doc: &str, pane: &str, stale: bool, recycle| ColumnGateFacts {
            file: PathBuf::from(doc),
            pane: pane.to_string(),
            freshness: classify_pane_supervisor_freshness(Some(20488), Some(stale), false),
            own_pane: true,
            is_focus: doc == "tasks/pmt2/mr/1099.md",
            outside_target_window: pane_outside_target_window(before.get(pane), target),
            recycle,
            turn_active: false,
        };
        let pending = StaleRecycleRequestState::Pending {
            reason: "stale_supervisor_turn_stage".to_string(),
            age_secs: 5,
            deferred_by_turn: false,
        };
        let overdue = StaleRecycleRequestState::Overdue {
            reason: "install_fanout".to_string(),
            age_secs: 80_071,
        };
        let plan = |facts: &[ColumnGateFacts]| -> Vec<ColumnAdmission> {
            plan_column_admissions(facts, &window_panes, &Vec::new)
                .into_iter()
                .map(|(admission, _)| admission)
                .collect()
        };

        // Widening from a non-stash window: refused even while pending.
        let widen = [
            fact("tasks/pmt2/mr/1099.md", "%41", true, pending.clone()),
            fact(
                "tasks/agent-doc/agent-doc.ad.md",
                "%434",
                false,
                StaleRecycleRequestState::NotRequested,
            ),
            fact(
                "tasks/pmt2/tickets/2222.md",
                "%430",
                false,
                StaleRecycleRequestState::NotRequested,
            ),
        ];
        assert_eq!(
            plan(&widen)[0],
            ColumnAdmission::ExcludeStaleFocusedWouldWiden
        );

        // Replacing a column while pending: admitted (the GH #136 exception).
        let replace = [
            fact("tasks/pmt2/mr/1099.md", "%41", true, pending),
            fact(
                "tasks/agent-doc/agent-doc.ad.md",
                "%434",
                false,
                StaleRecycleRequestState::NotRequested,
            ),
        ];
        assert_eq!(plan(&replace)[0], ColumnAdmission::AdmitStaleFocused);

        // Overdue: never moved in from a non-stash window either.
        let overdue_replace = [
            fact("tasks/pmt2/mr/1099.md", "%41", true, overdue.clone()),
            fact(
                "tasks/agent-doc/agent-doc.ad.md",
                "%434",
                false,
                StaleRecycleRequestState::NotRequested,
            ),
        ];
        assert_eq!(
            plan(&overdue_replace)[0],
            ColumnAdmission::ExcludeStaleFocusedRecycleOverdue
        );

        // Already in the target window: never moved by the bound (no flap),
        // even overdue and even when the plan is wider than the window.
        let mut visible_before = before.clone();
        visible_before.insert("%41".to_string(), snapshot("@1", "agent-doc"));
        let visible = ColumnGateFacts {
            outside_target_window: pane_outside_target_window(visible_before.get("%41"), target),
            ..fact("tasks/pmt2/mr/1099.md", "%41", true, overdue)
        };
        assert!(!visible.outside_target_window);
        assert_eq!(
            plan(&[
                visible,
                fact(
                    "tasks/agent-doc/agent-doc.ad.md",
                    "%434",
                    false,
                    StaleRecycleRequestState::NotRequested
                ),
                fact(
                    "tasks/pmt2/tickets/2222.md",
                    "%430",
                    false,
                    StaleRecycleRequestState::NotRequested
                ),
            ])[0],
            ColumnAdmission::AdmitStaleFocused
        );
    }
}
