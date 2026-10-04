//! Pure route-owned supervisor lifecycle and completion policy.
//!
//! This module owns queue-control admission for route-owned startup and decides
//! whether a route-owned supervisor pane should reap itself after a committed
//! document cycle. It does not read documents, inspect panes, spawn processes,
//! or write logs; callers provide the lifecycle and liveness facts gathered from
//! their effectful adapters.

use std::{fmt, str::FromStr};

use agent_doc_prompt_lines::text_line_looks_like_prompt_target;
use agent_doc_state_scope::LocalProcessScope;
use lazily::{Computed, Source};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteOwnedReapPolicy {
    Auto,
    ReapAfterCommit,
    KeepAlive,
}

impl RouteOwnedReapPolicy {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::ReapAfterCommit => "reap_after_commit",
            Self::KeepAlive => "keep_alive",
        }
    }
}

impl fmt::Display for RouteOwnedReapPolicy {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Auto => "auto",
            Self::ReapAfterCommit => "reap-after-commit",
            Self::KeepAlive => "keep-alive",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRouteOwnedReapPolicyError {
    value: String,
}

impl fmt::Display for ParseRouteOwnedReapPolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid route-owned reap policy {:?}; expected auto, reap-after-commit, or keep-alive",
            self.value
        )
    }
}

impl std::error::Error for ParseRouteOwnedReapPolicyError {}

impl FromStr for RouteOwnedReapPolicy {
    type Err = ParseRouteOwnedReapPolicyError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "auto" => Ok(Self::Auto),
            "reap-after-commit" | "reap_after_commit" => Ok(Self::ReapAfterCommit),
            "keep-alive" | "keep_alive" => Ok(Self::KeepAlive),
            _ => Err(ParseRouteOwnedReapPolicyError {
                value: value.to_string(),
            }),
        }
    }
}

/// Why a new route-owned supervisor lifecycle is being started.
///
/// Layout provisioning may create an idle harness owner so an editor projection
/// can converge, but it never carries a prompt dispatch. Queue control therefore
/// fences `Dispatch` while allowing `LayoutProvision`; the idle supervisor's
/// queue watcher continues to honor the same pause before any later dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RouteOwnedStartPurpose {
    #[default]
    Dispatch,
    LayoutProvision,
}

impl RouteOwnedStartPurpose {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Dispatch => "dispatch",
            Self::LayoutProvision => "layout_provision",
        }
    }
}

impl fmt::Display for RouteOwnedStartPurpose {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Dispatch => "dispatch",
            Self::LayoutProvision => "layout-provision",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseRouteOwnedStartPurposeError {
    value: String,
}

impl fmt::Display for ParseRouteOwnedStartPurposeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "invalid route-owned start purpose {:?}; expected dispatch or layout-provision",
            self.value
        )
    }
}

impl std::error::Error for ParseRouteOwnedStartPurposeError {}

impl FromStr for RouteOwnedStartPurpose {
    type Err = ParseRouteOwnedStartPurposeError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        match value {
            "dispatch" => Ok(Self::Dispatch),
            "layout-provision" | "layout_provision" => Ok(Self::LayoutProvision),
            _ => Err(ParseRouteOwnedStartPurposeError {
                value: value.to_string(),
            }),
        }
    }
}

/// Typed lifecycle admission presented to durable queue-control policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteOwnedStartAdmission {
    NewSession(RouteOwnedStartPurpose),
    SupervisorReentry,
}

/// Whether effective queue control blocks this route-owned lifecycle edge.
///
/// This is the single policy owner shared by route provisioning and the spawned
/// `agent-doc start` process, closing their pause-vs-start race without making a
/// layout projection retry forever.
pub const fn route_owned_start_blocked_by_queue_control(
    route_owned: bool,
    admission: RouteOwnedStartAdmission,
) -> bool {
    route_owned
        && matches!(
            admission,
            RouteOwnedStartAdmission::NewSession(RouteOwnedStartPurpose::Dispatch)
        )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOwnedLivenessReason {
    BacklogNonEmpty,
    QueueNonEmpty,
    PostCommitUserFollowUp,
    ExchangeTailUnresolvedPrompt,
    DocumentDirtyAfterCommit,
    AdapterFailure(String),
}

impl RouteOwnedLivenessReason {
    pub fn as_str(&self) -> &str {
        match self {
            Self::BacklogNonEmpty => "backlog_non_empty",
            Self::QueueNonEmpty => "queue_non_empty",
            Self::PostCommitUserFollowUp => "post_commit_user_follow_up",
            Self::ExchangeTailUnresolvedPrompt => "exchange_tail_unresolved_prompt",
            Self::DocumentDirtyAfterCommit => "document_dirty_after_commit",
            Self::AdapterFailure(reason) => reason,
        }
    }
}

impl fmt::Display for RouteOwnedLivenessReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteOwnedReapDecision {
    pub reap: bool,
    pub reason: String,
}

/// One fully-derived route-owned completion effect.
///
/// Polling observes these facts; only a change in this value licenses another
/// diagnostic/effect receipt. This keeps liveness state in Lazily instead of
/// turning every timer tick into a command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteOwnedReapEffect {
    pub policy: RouteOwnedReapPolicy,
    pub decision: RouteOwnedReapDecision,
    pub cycle_id: String,
    pub cycle_event: String,
    pub suppression: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
struct RouteOwnedReapProjection {
    observed: Option<RouteOwnedReapEffect>,
    consumed: Option<RouteOwnedReapEffect>,
}

impl RouteOwnedReapProjection {
    fn pending(&self) -> Option<RouteOwnedReapEffect> {
        (self.observed != self.consumed)
            .then(|| self.observed.clone())
            .flatten()
    }
}

/// Lazily-owned edge detector for route-owned completion effects.
pub struct RouteOwnedReapEffects {
    scope: LocalProcessScope,
    projection: Source<RouteOwnedReapProjection>,
    pending: Computed<Option<RouteOwnedReapEffect>>,
}

impl Default for RouteOwnedReapEffects {
    fn default() -> Self {
        Self::new()
    }
}

impl RouteOwnedReapEffects {
    pub fn new() -> Self {
        let scope = LocalProcessScope::new();
        let projection = scope.ctx().source(RouteOwnedReapProjection::default());
        let projection_for_pending = projection;
        let pending = scope
            .ctx()
            .computed(move |ctx| ctx.get(&projection_for_pending).pending());
        Self {
            scope,
            projection,
            pending,
        }
    }

    /// Observe state and consume at most one effect for this exact transition.
    pub fn observe_and_take(
        &self,
        observation: RouteOwnedReapEffect,
    ) -> Option<RouteOwnedReapEffect> {
        let mut projection = self.scope.ctx().get(&self.projection);
        if projection.observed.as_ref() != Some(&observation) {
            projection.observed = Some(observation);
            self.scope.ctx().set(&self.projection, projection);
        }

        let effect = self.scope.ctx().get(&self.pending)?;
        let mut projection = self.scope.ctx().get(&self.projection);
        projection.consumed = Some(effect.clone());
        self.scope.ctx().set(&self.projection, projection);
        Some(effect)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RouteOwnedCyclePhase {
    Open,
    Committed,
    Closed,
}

impl RouteOwnedCyclePhase {
    pub const fn is_open(self) -> bool {
        matches!(self, Self::Open)
    }

    pub const fn is_committed(self) -> bool {
        matches!(self, Self::Committed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RouteOwnedCycleFacts {
    pub cycle_id: String,
    pub phase: RouteOwnedCyclePhase,
    pub updated_at: u64,
    pub last_event: String,
    pub committed_file_hash: Option<String>,
}

pub fn route_owned_cycle_changed_since_start(
    current: &RouteOwnedCycleFacts,
    baseline: Option<&RouteOwnedCycleFacts>,
) -> bool {
    match baseline {
        None => true,
        Some(previous) if previous.phase.is_open() => {
            current.cycle_id != previous.cycle_id
                || current.updated_at != previous.updated_at
                || current.phase != previous.phase
                || current.last_event != previous.last_event
        }
        Some(previous) => current.cycle_id != previous.cycle_id,
    }
}

pub fn route_owned_cycle_committed_since_start(
    current: &RouteOwnedCycleFacts,
    baseline: Option<&RouteOwnedCycleFacts>,
) -> bool {
    route_owned_cycle_changed_since_start(current, baseline) && current.phase.is_committed()
}

pub fn route_owned_reap_decision(
    policy: RouteOwnedReapPolicy,
    liveness_reason: Option<RouteOwnedLivenessReason>,
) -> RouteOwnedReapDecision {
    match policy {
        RouteOwnedReapPolicy::KeepAlive => RouteOwnedReapDecision {
            reap: false,
            reason: "explicit_keep_alive".to_string(),
        },
        RouteOwnedReapPolicy::ReapAfterCommit => RouteOwnedReapDecision {
            reap: true,
            reason: "explicit_reap_after_commit".to_string(),
        },
        RouteOwnedReapPolicy::Auto => {
            if let Some(reason) = liveness_reason {
                RouteOwnedReapDecision {
                    reap: false,
                    reason: reason.to_string(),
                }
            } else {
                RouteOwnedReapDecision {
                    reap: true,
                    reason: "no_liveness_signals".to_string(),
                }
            }
        }
    }
}

/// Whether a liveness signal is *pending interaction* a stashed
/// layout-provision pane must stay alive for.
///
/// `#stashpaneunbounded`: `BacklogNonEmpty` is deliberately NOT one. Every task
/// document carries a backlog as its steady state, so treating it as liveness
/// is exactly what made layout-provision panes unbounded — the signal is true
/// forever and for every document. The remaining reasons all describe work the
/// operator is mid-way through (a queued prompt, an unanswered exchange tail, a
/// post-commit follow-up, uncommitted drift) or an adapter the reaper must not
/// second-guess.
pub fn route_owned_liveness_blocks_layout_provision_reap(
    reason: Option<&RouteOwnedLivenessReason>,
) -> bool {
    match reason {
        None | Some(RouteOwnedLivenessReason::BacklogNonEmpty) => false,
        Some(_) => true,
    }
}

/// Purpose-aware reap decision.
///
/// `#stashpaneunbounded`: `KeepAlive` means "do not reap merely because a cycle
/// committed" — it was never meant to mean "never exit, ever," but that is what
/// it did: [`route_owned_reap_decision`] answers `explicit_keep_alive`
/// unconditionally, discarding the liveness reason before it is even read.
/// Measured on the dogfood project: 35 `explicit_keep_alive` decisions across
/// six documents and not one reap, one permanent pane per document ever visited
/// in the editor.
///
/// A layout-provision supervisor exists to hold a pane for an editor column and
/// nothing else — queue control fences `Dispatch` and lets `LayoutProvision`
/// through precisely because it never carries a prompt. Once its pane is
/// **stashed** it is holding no visible column, so it is doing the one job it
/// has nowhere at all. That is the orphan condition, and it is proven from the
/// supervisor's own start purpose and its own pane — never from a working
/// directory or a command line, and never about a pane another session owns.
///
/// Callers must evaluate this only when the document's cycle is not open, and
/// must let a live-pane-busy reason short-circuit to `reap: false` first, so an
/// active harness turn in that pane is never reaped.
pub fn route_owned_reap_decision_for_purpose(
    policy: RouteOwnedReapPolicy,
    purpose: RouteOwnedStartPurpose,
    liveness_reason: Option<RouteOwnedLivenessReason>,
    owned_pane_stashed: bool,
) -> RouteOwnedReapDecision {
    if policy == RouteOwnedReapPolicy::KeepAlive
        && purpose == RouteOwnedStartPurpose::LayoutProvision
        && owned_pane_stashed
        && !route_owned_liveness_blocks_layout_provision_reap(liveness_reason.as_ref())
    {
        return RouteOwnedReapDecision {
            reap: true,
            reason: "layout_provision_stashed_orphan".to_string(),
        };
    }
    route_owned_reap_decision(policy, liveness_reason)
}

/// Reap reason published when a route-owned supervisor proves the controller's
/// authoritative actor binding for its document names a different, live pane.
pub const ROUTE_OWNED_SUPERSEDED_BINDING_ORPHAN: &str = "superseded_binding_orphan";

/// Durable facts about the authoritative actor binding for this supervisor's
/// document, observed from the supervisor's own side (GH #133).
///
/// `own_pane_id` is the pane this supervisor generation registered with the
/// controller. `authoritative_pane_id` is the pane named by the durable actor
/// record for the same document (`None` when no record exists or it could not
/// be read). `authoritative_pane_alive` must be a positive tmux observation of
/// exactly that pane.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RouteOwnedBindingFacts<'a> {
    pub own_pane_id: &'a str,
    pub authoritative_pane_id: Option<&'a str>,
    pub authoritative_pane_alive: bool,
}

/// What a route-owned supervisor can prove about its own binding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RouteOwnedBindingObservation {
    /// The authoritative actor record still names this supervisor's pane.
    Owned,
    /// The authoritative record names a different pane, and that pane is live:
    /// the controller already rejects every transition this generation makes.
    Superseded { current_pane: String },
    /// Neither could be proven (no record, unreadable record, a dead or empty
    /// superseding pane, or this supervisor owns no tmux pane). Never reaps.
    Unproven,
}

/// `#routeownedsupersededreap` (GH #133): classify this supervisor's binding.
///
/// A route-owned supervisor only learned it had been superseded when one of its
/// own actor transitions was rejected (`ActorGenerationLease::observe_failure`).
/// An idle supervisor parked in a `stash` window never transitions, so it never
/// learned, and it lived as long as the tmux server — one permanent pane and
/// ~7% CPU per superseded generation. This is the read-side proof of the same
/// fact the controller's `mark_lifecycle` rejection encodes: the authoritative
/// record names another pane.
///
/// Strict: only a *different, live* pane is supersession. A missing record, a
/// dead superseding pane, or a supervisor without a tmux pane answers
/// [`RouteOwnedBindingObservation::Unproven`], which never reaps.
pub fn route_owned_binding_observation(
    facts: RouteOwnedBindingFacts<'_>,
) -> RouteOwnedBindingObservation {
    let own = facts.own_pane_id.trim();
    if !own.starts_with('%') {
        return RouteOwnedBindingObservation::Unproven;
    }
    let Some(current) = facts.authoritative_pane_id.map(str::trim) else {
        return RouteOwnedBindingObservation::Unproven;
    };
    if current == own {
        return RouteOwnedBindingObservation::Owned;
    }
    if current.starts_with('%') && facts.authoritative_pane_alive {
        return RouteOwnedBindingObservation::Superseded {
            current_pane: current.to_string(),
        };
    }
    RouteOwnedBindingObservation::Unproven
}

/// `#routeownedsupersededreap` (GH #133): reap decision for a superseded
/// route-owned supervisor, independent of reap policy and start purpose.
///
/// `explicit_keep_alive` protects the document's *owner*; a superseded
/// generation is not the owner, so no policy keeps it. Returns `Some(reap)` only
/// when the binding is proven [`RouteOwnedBindingObservation::Superseded`], the
/// document cycle is not open, and direct child-output observation shows no
/// active turn in this supervisor's own pane. Every other shape returns `None`
/// (leave the supervisor alone).
pub fn route_owned_superseded_orphan_decision(
    binding: &RouteOwnedBindingObservation,
    cycle_open: bool,
    observed_live_pane_busy: Option<&str>,
) -> Option<RouteOwnedReapDecision> {
    if cycle_open || observed_live_pane_busy.is_some() {
        return None;
    }
    matches!(binding, RouteOwnedBindingObservation::Superseded { .. }).then(|| {
        RouteOwnedReapDecision {
            reap: true,
            reason: ROUTE_OWNED_SUPERSEDED_BINDING_ORPHAN.to_string(),
        }
    })
}

/// Keep-alive reason for an `auto` route-owned pane that currently fills a
/// visible editor column in the layout window.
pub const ROUTE_OWNED_VISIBLE_COLUMN_KEEP_ALIVE: &str = "owned_pane_holds_visible_layout_column";

/// `#routeownedvisiblereap`: an `auto` dispatch pane with no pending work used
/// to be reaped right after its commit even while it held a visible editor
/// column. Nothing re-provisioned the column, so the layout window collapsed
/// (observed 2026-09-29: haiven `infra.md` pane `%27` reaped with
/// `no_liveness_signals`, leaving `agent-doc` with one pane while `infra.md`
/// stayed open in the editor). A visible pane is doing the job a layout pane
/// exists for, so it stays alive. Once the layout stashes it, it becomes a
/// stashed orphan and [`route_owned_visible_column_stashed_orphan_decision`]
/// reaps it.
pub fn route_owned_keep_visible_column(
    policy: RouteOwnedReapPolicy,
    decision: RouteOwnedReapDecision,
    owned_pane_visible: bool,
) -> RouteOwnedReapDecision {
    if policy == RouteOwnedReapPolicy::Auto && decision.reap && owned_pane_visible {
        return RouteOwnedReapDecision {
            reap: false,
            reason: ROUTE_OWNED_VISIBLE_COLUMN_KEEP_ALIVE.to_string(),
        };
    }
    decision
}

/// Reap decision for a pane that was kept alive only because it held a visible
/// column and has since been stashed. Uses the same pending-interaction rule as
/// layout-provision orphans (a steady-state backlog is not liveness).
pub fn route_owned_visible_column_stashed_orphan_decision(
    liveness_reason: Option<RouteOwnedLivenessReason>,
) -> RouteOwnedReapDecision {
    match liveness_reason {
        Some(reason) if route_owned_liveness_blocks_layout_provision_reap(Some(&reason)) => {
            RouteOwnedReapDecision {
                reap: false,
                reason: reason.to_string(),
            }
        }
        _ => RouteOwnedReapDecision {
            reap: true,
            reason: "visible_column_pane_stashed_orphan".to_string(),
        },
    }
}

pub fn route_owned_file_dirty_after_commit(
    content: &str,
    committed_file_hash: Option<&str>,
) -> bool {
    committed_file_hash.is_some_and(|hash| agent_doc_hash::content_hash(content) != hash)
}

pub fn route_owned_liveness_reason_for_content(
    content: &str,
    committed_file_hash: Option<&str>,
) -> Option<RouteOwnedLivenessReason> {
    let dirty_after_commit = route_owned_file_dirty_after_commit(content, committed_file_hash);
    if dirty_after_commit && route_owned_exchange_tail_has_unresolved_prompt(content) {
        return Some(RouteOwnedLivenessReason::PostCommitUserFollowUp);
    }

    let components = match agent_doc_element::element::parse(content) {
        Ok(components) => components,
        Err(err) => {
            return Some(if dirty_after_commit {
                RouteOwnedLivenessReason::DocumentDirtyAfterCommit
            } else {
                RouteOwnedLivenessReason::AdapterFailure(format!("component_parse_failed:{err}"))
            });
        }
    };

    for component in &components {
        let body = component.content(content);
        if agent_doc_element::element::is_backlog_component(&component.name)
            && route_owned_backlog_has_live_items(body)
        {
            return Some(RouteOwnedLivenessReason::BacklogNonEmpty);
        }
        if component.name == "queue" && route_owned_queue_has_prompts(body) {
            return Some(RouteOwnedLivenessReason::QueueNonEmpty);
        }
        if component.name == "exchange" && route_owned_exchange_tail_has_unresolved_prompt(body) {
            return Some(if dirty_after_commit {
                RouteOwnedLivenessReason::PostCommitUserFollowUp
            } else {
                RouteOwnedLivenessReason::ExchangeTailUnresolvedPrompt
            });
        }
    }

    if dirty_after_commit {
        return Some(RouteOwnedLivenessReason::DocumentDirtyAfterCommit);
    }

    None
}

pub fn route_owned_backlog_has_live_items(body: &str) -> bool {
    let (_, items, _) = agent_doc_element_backlog::backlog::parse_items(body);
    items
        .iter()
        .any(|item| item.state != agent_doc_element_backlog::backlog::PendingState::Done)
}

pub fn route_owned_queue_has_prompts(body: &str) -> bool {
    match agent_doc_queue::document_queue::parse(body) {
        Ok(entries) => !agent_doc_queue::document_queue::prompts(&entries).is_empty(),
        Err(_) => !body.trim().is_empty(),
    }
}

pub fn route_owned_exchange_tail_has_unresolved_prompt(body: &str) -> bool {
    let mut tail_start = 0usize;
    let mut line_start = 0usize;
    for line in body.split_inclusive('\n') {
        if route_owned_line_is_response_heading(line.trim()) {
            tail_start = line_start + line.len();
        }
        line_start += line.len();
    }
    if line_start < body.len() && route_owned_line_is_response_heading(body[line_start..].trim()) {
        tail_start = body.len();
    }

    body[tail_start..]
        .lines()
        .any(text_line_looks_like_prompt_target)
}

fn route_owned_line_is_response_heading(line: &str) -> bool {
    line == "## Assistant"
        || line.starts_with("### Re:")
        || line.starts_with("#### Re:")
        || line.starts_with("##### Re:")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_pane_holding_a_visible_column_survives_commit_then_reaps_once_stashed() {
        let reap = route_owned_reap_decision(RouteOwnedReapPolicy::Auto, None);
        assert!(reap.reap);

        let kept = route_owned_keep_visible_column(RouteOwnedReapPolicy::Auto, reap.clone(), true);
        assert!(!kept.reap);
        assert_eq!(kept.reason, ROUTE_OWNED_VISIBLE_COLUMN_KEEP_ALIVE);

        assert_eq!(
            route_owned_keep_visible_column(RouteOwnedReapPolicy::Auto, reap.clone(), false),
            reap,
            "a stashed or vanished pane keeps the one-shot reap"
        );
        let explicit = route_owned_reap_decision(RouteOwnedReapPolicy::ReapAfterCommit, None);
        assert_eq!(
            route_owned_keep_visible_column(
                RouteOwnedReapPolicy::ReapAfterCommit,
                explicit.clone(),
                true
            ),
            explicit,
            "an explicit one-shot reap is never overridden"
        );

        let orphan = route_owned_visible_column_stashed_orphan_decision(Some(
            RouteOwnedLivenessReason::BacklogNonEmpty,
        ));
        assert!(orphan.reap, "a steady-state backlog must not pin a stashed pane");
        assert_eq!(orphan.reason, "visible_column_pane_stashed_orphan");
        let pending = route_owned_visible_column_stashed_orphan_decision(Some(
            RouteOwnedLivenessReason::QueueNonEmpty,
        ));
        assert!(!pending.reap);
    }

    #[test]
    fn queue_control_blocks_dispatch_but_not_layout_provision_or_reentry() {
        assert!(route_owned_start_blocked_by_queue_control(
            true,
            RouteOwnedStartAdmission::NewSession(RouteOwnedStartPurpose::Dispatch),
        ));
        assert!(!route_owned_start_blocked_by_queue_control(
            true,
            RouteOwnedStartAdmission::NewSession(RouteOwnedStartPurpose::LayoutProvision),
        ));
        assert!(!route_owned_start_blocked_by_queue_control(
            true,
            RouteOwnedStartAdmission::SupervisorReentry,
        ));
        assert!(!route_owned_start_blocked_by_queue_control(
            false,
            RouteOwnedStartAdmission::NewSession(RouteOwnedStartPurpose::Dispatch),
        ));
    }

    #[test]
    fn route_owned_start_purpose_round_trips_cli_values() {
        assert_eq!(
            "dispatch".parse::<RouteOwnedStartPurpose>().unwrap(),
            RouteOwnedStartPurpose::Dispatch
        );
        assert_eq!(
            "layout-provision"
                .parse::<RouteOwnedStartPurpose>()
                .unwrap(),
            RouteOwnedStartPurpose::LayoutProvision
        );
        assert_eq!(
            RouteOwnedStartPurpose::LayoutProvision.to_string(),
            "layout-provision"
        );
    }

    fn reap_effect(cycle: &str, reason: &str) -> RouteOwnedReapEffect {
        RouteOwnedReapEffect {
            policy: RouteOwnedReapPolicy::Auto,
            decision: RouteOwnedReapDecision {
                reap: false,
                reason: reason.to_string(),
            },
            cycle_id: cycle.to_string(),
            cycle_event: "commit_success".to_string(),
            suppression: None,
        }
    }

    #[test]
    fn route_owned_reap_effect_is_edge_triggered_by_lazily_state() {
        let effects = RouteOwnedReapEffects::new();
        let first = reap_effect("cycle-a", "queue_non_empty");

        assert_eq!(effects.observe_and_take(first.clone()), Some(first.clone()));
        assert_eq!(
            effects.observe_and_take(first),
            None,
            "an unchanged poll must not emit another log or completion effect"
        );

        let changed = reap_effect("cycle-b", "document_dirty_after_commit");
        assert_eq!(
            effects.observe_and_take(changed.clone()),
            Some(changed),
            "a new lifecycle transition must emit exactly once"
        );
    }

    /// `#stashpaneunbounded`: the measured defect. `keep-alive` answered
    /// `explicit_keep_alive` no matter what, so a layout-provision pane never
    /// exited — 35 such decisions across six documents on the dogfood project
    /// and not one reap.
    #[test]
    fn keep_alive_still_never_reaps_a_visible_layout_provision_pane() {
        assert_eq!(
            route_owned_reap_decision_for_purpose(
                RouteOwnedReapPolicy::KeepAlive,
                RouteOwnedStartPurpose::LayoutProvision,
                None,
                false,
            ),
            RouteOwnedReapDecision {
                reap: false,
                reason: "explicit_keep_alive".to_string()
            },
            "a layout-provision pane holding a visible column is doing its job"
        );
    }

    #[test]
    fn keep_alive_reaps_a_stashed_layout_provision_pane_with_nothing_to_do() {
        assert_eq!(
            route_owned_reap_decision_for_purpose(
                RouteOwnedReapPolicy::KeepAlive,
                RouteOwnedStartPurpose::LayoutProvision,
                None,
                true,
            ),
            RouteOwnedReapDecision {
                reap: true,
                reason: "layout_provision_stashed_orphan".to_string()
            }
        );
    }

    /// A backlog is the steady state of every task document. Counting it as
    /// liveness is what made the population unbounded, so a stashed
    /// layout-provision pane is reaped despite one.
    #[test]
    fn a_backlog_alone_does_not_keep_a_stashed_layout_provision_pane_alive() {
        assert_eq!(
            route_owned_reap_decision_for_purpose(
                RouteOwnedReapPolicy::KeepAlive,
                RouteOwnedStartPurpose::LayoutProvision,
                Some(RouteOwnedLivenessReason::BacklogNonEmpty),
                true,
            ),
            RouteOwnedReapDecision {
                reap: true,
                reason: "layout_provision_stashed_orphan".to_string()
            }
        );
    }

    /// Pending interaction still wins. Each of these means the operator is
    /// mid-way through something on that document.
    #[test]
    fn pending_interaction_keeps_a_stashed_layout_provision_pane_alive() {
        for reason in [
            RouteOwnedLivenessReason::QueueNonEmpty,
            RouteOwnedLivenessReason::PostCommitUserFollowUp,
            RouteOwnedLivenessReason::ExchangeTailUnresolvedPrompt,
            RouteOwnedLivenessReason::DocumentDirtyAfterCommit,
            RouteOwnedLivenessReason::AdapterFailure("read_failed:boom".to_string()),
        ] {
            assert_eq!(
                route_owned_reap_decision_for_purpose(
                    RouteOwnedReapPolicy::KeepAlive,
                    RouteOwnedStartPurpose::LayoutProvision,
                    Some(reason.clone()),
                    true,
                ),
                RouteOwnedReapDecision {
                    reap: false,
                    reason: "explicit_keep_alive".to_string()
                },
                "{reason:?} is pending interaction, not steady state"
            );
        }
    }

    /// The new leg is scoped to layout provision. A dispatch owner that was
    /// explicitly told `keep-alive` keeps its pre-existing behaviour even when
    /// stashed with nothing to do.
    #[test]
    fn a_stashed_dispatch_owner_is_not_reaped_by_the_layout_provision_leg() {
        assert_eq!(
            route_owned_reap_decision_for_purpose(
                RouteOwnedReapPolicy::KeepAlive,
                RouteOwnedStartPurpose::Dispatch,
                None,
                true,
            ),
            RouteOwnedReapDecision {
                reap: false,
                reason: "explicit_keep_alive".to_string()
            }
        );
    }

    fn binding(own: &str, current: Option<&str>, alive: bool) -> RouteOwnedBindingObservation {
        route_owned_binding_observation(RouteOwnedBindingFacts {
            own_pane_id: own,
            authoritative_pane_id: current,
            authoritative_pane_alive: alive,
        })
    }

    /// GH #133: a generation whose document binding moved to another live pane
    /// is reaped regardless of `keep-alive` or start purpose.
    #[test]
    fn superseded_route_owned_supervisor_is_reaped() {
        let observed = binding("%28", Some("%41"), true);
        assert_eq!(
            observed,
            RouteOwnedBindingObservation::Superseded {
                current_pane: "%41".to_string()
            }
        );
        assert_eq!(
            route_owned_superseded_orphan_decision(&observed, false, None),
            Some(RouteOwnedReapDecision {
                reap: true,
                reason: ROUTE_OWNED_SUPERSEDED_BINDING_ORPHAN.to_string(),
            })
        );
    }

    /// GH #133: the live owner (record names this pane) is never reaped by the
    /// supersession leg, whatever its other state.
    #[test]
    fn live_owned_supervisor_is_kept_by_the_supersession_leg() {
        let observed = binding("%28", Some("%28"), true);
        assert_eq!(observed, RouteOwnedBindingObservation::Owned);
        for cycle_open in [false, true] {
            for busy in [None, Some("live_pane_busy_blocked_prompt")] {
                assert_eq!(
                    route_owned_superseded_orphan_decision(&observed, cycle_open, busy),
                    None
                );
            }
        }
    }

    /// Strict proof: no record, a dead or non-tmux superseding pane, or a
    /// supervisor without its own tmux pane is unproven and never reaps.
    #[test]
    fn unproven_binding_never_reaps() {
        for observed in [
            binding("%28", None, true),
            binding("%28", Some("%41"), false),
            binding("%28", Some(""), true),
            binding("<pty>", Some("%41"), true),
            binding("", Some("%41"), true),
        ] {
            assert_eq!(observed, RouteOwnedBindingObservation::Unproven);
            assert_eq!(
                route_owned_superseded_orphan_decision(&observed, false, None),
                None
            );
        }
    }

    /// A superseded pane still running a turn, or a document with an open
    /// cycle, waits for a quiet tick instead of being torn down mid-turn.
    #[test]
    fn superseded_supervisor_with_active_turn_or_open_cycle_waits() {
        let observed = binding("%28", Some("%41"), true);
        assert_eq!(
            route_owned_superseded_orphan_decision(&observed, true, None),
            None
        );
        assert_eq!(
            route_owned_superseded_orphan_decision(
                &observed,
                false,
                Some("live_pane_busy_blocked_prompt reason=active claude turn"),
            ),
            None
        );
    }

    /// The other two policies are untouched by purpose or stash state.
    #[test]
    fn auto_and_reap_after_commit_are_unchanged_by_the_purpose_aware_wrapper() {
        for purpose in [
            RouteOwnedStartPurpose::Dispatch,
            RouteOwnedStartPurpose::LayoutProvision,
        ] {
            for stashed in [false, true] {
                for policy in [
                    RouteOwnedReapPolicy::Auto,
                    RouteOwnedReapPolicy::ReapAfterCommit,
                ] {
                    for liveness in [None, Some(RouteOwnedLivenessReason::BacklogNonEmpty)] {
                        assert_eq!(
                            route_owned_reap_decision_for_purpose(
                                policy,
                                purpose,
                                liveness.clone(),
                                stashed,
                            ),
                            route_owned_reap_decision(policy, liveness.clone()),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn auto_reaps_without_liveness_signals() {
        assert_eq!(
            route_owned_reap_decision(RouteOwnedReapPolicy::Auto, None),
            RouteOwnedReapDecision {
                reap: true,
                reason: "no_liveness_signals".to_string()
            }
        );
    }

    #[test]
    fn auto_keeps_alive_for_each_liveness_reason() {
        for reason in [
            RouteOwnedLivenessReason::BacklogNonEmpty,
            RouteOwnedLivenessReason::QueueNonEmpty,
            RouteOwnedLivenessReason::PostCommitUserFollowUp,
            RouteOwnedLivenessReason::ExchangeTailUnresolvedPrompt,
            RouteOwnedLivenessReason::DocumentDirtyAfterCommit,
        ] {
            assert_eq!(
                route_owned_reap_decision(RouteOwnedReapPolicy::Auto, Some(reason.clone())),
                RouteOwnedReapDecision {
                    reap: false,
                    reason: reason.as_str().to_string()
                }
            );
        }
    }

    #[test]
    fn auto_keeps_alive_with_adapter_failure_reason() {
        assert_eq!(
            route_owned_reap_decision(
                RouteOwnedReapPolicy::Auto,
                Some(RouteOwnedLivenessReason::AdapterFailure(
                    "read_failed:permission denied".to_string()
                ))
            ),
            RouteOwnedReapDecision {
                reap: false,
                reason: "read_failed:permission denied".to_string()
            }
        );
    }

    #[test]
    fn explicit_reap_overrides_liveness() {
        assert_eq!(
            route_owned_reap_decision(
                RouteOwnedReapPolicy::ReapAfterCommit,
                Some(RouteOwnedLivenessReason::BacklogNonEmpty)
            ),
            RouteOwnedReapDecision {
                reap: true,
                reason: "explicit_reap_after_commit".to_string()
            }
        );
    }

    #[test]
    fn explicit_keep_alive_overrides_missing_liveness() {
        assert_eq!(
            route_owned_reap_decision(RouteOwnedReapPolicy::KeepAlive, None),
            RouteOwnedReapDecision {
                reap: false,
                reason: "explicit_keep_alive".to_string()
            }
        );
    }

    fn cycle(id: &str, phase: RouteOwnedCyclePhase, updated_at: u64) -> RouteOwnedCycleFacts {
        RouteOwnedCycleFacts {
            cycle_id: id.to_string(),
            phase,
            updated_at,
            last_event: format!("{phase:?}"),
            committed_file_hash: None,
        }
    }

    #[test]
    fn route_owned_cycle_policy_treats_missing_baseline_as_changed() {
        let current = cycle("cycle-1", RouteOwnedCyclePhase::Committed, 10);

        assert!(route_owned_cycle_changed_since_start(&current, None));
        assert!(route_owned_cycle_committed_since_start(&current, None));
    }

    #[test]
    fn route_owned_cycle_policy_ignores_unchanged_committed_baseline() {
        let baseline = cycle("cycle-1", RouteOwnedCyclePhase::Committed, 10);
        let current = baseline.clone();

        assert!(!route_owned_cycle_changed_since_start(
            &current,
            Some(&baseline)
        ));
        assert!(!route_owned_cycle_committed_since_start(
            &current,
            Some(&baseline)
        ));
    }

    #[test]
    fn route_owned_cycle_policy_detects_new_committed_cycle() {
        let baseline = cycle("cycle-1", RouteOwnedCyclePhase::Committed, 10);
        let current = cycle("cycle-2", RouteOwnedCyclePhase::Committed, 20);

        assert!(route_owned_cycle_changed_since_start(
            &current,
            Some(&baseline)
        ));
        assert!(route_owned_cycle_committed_since_start(
            &current,
            Some(&baseline)
        ));
    }

    #[test]
    fn route_owned_cycle_policy_waits_while_new_cycle_is_open() {
        let baseline = cycle("cycle-1", RouteOwnedCyclePhase::Committed, 10);
        let current = cycle("cycle-2", RouteOwnedCyclePhase::Open, 20);

        assert!(route_owned_cycle_changed_since_start(
            &current,
            Some(&baseline)
        ));
        assert!(!route_owned_cycle_committed_since_start(
            &current,
            Some(&baseline)
        ));
    }

    #[test]
    fn route_owned_cycle_policy_tracks_open_baseline_updates() {
        let baseline = cycle("cycle-1", RouteOwnedCyclePhase::Open, 10);
        let current = cycle("cycle-1", RouteOwnedCyclePhase::Open, 11);

        assert!(route_owned_cycle_changed_since_start(
            &current,
            Some(&baseline)
        ));
        assert!(!route_owned_cycle_committed_since_start(
            &current,
            Some(&baseline)
        ));
    }

    #[test]
    fn parse_reap_policy_accepts_cli_and_log_spellings() {
        assert_eq!("auto".parse(), Ok(RouteOwnedReapPolicy::Auto));
        assert_eq!(
            "reap-after-commit".parse(),
            Ok(RouteOwnedReapPolicy::ReapAfterCommit)
        );
        assert_eq!(
            "reap_after_commit".parse(),
            Ok(RouteOwnedReapPolicy::ReapAfterCommit)
        );
        assert_eq!("keep-alive".parse(), Ok(RouteOwnedReapPolicy::KeepAlive));
        assert_eq!("keep_alive".parse(), Ok(RouteOwnedReapPolicy::KeepAlive));
        assert!("never".parse::<RouteOwnedReapPolicy>().is_err());
    }

    #[test]
    fn route_owned_body_liveness_detects_backlog_queue_and_exchange_tail() {
        assert!(route_owned_backlog_has_live_items(
            "- [ ] [#next] Continue\n- [x] [#done] Finished\n"
        ));
        assert!(!route_owned_backlog_has_live_items(
            "- [x] [#done] Finished\n"
        ));

        assert!(route_owned_queue_has_prompts("- do #next\n"));
        assert!(!route_owned_queue_has_prompts("<!-- empty -->\n"));

        let body = "\
### Re: done — gpt-5
Done.

do #next
";
        assert!(route_owned_exchange_tail_has_unresolved_prompt(body));
    }

    #[test]
    fn route_owned_exchange_tail_ignores_prompt_text_before_latest_response() {
        let body = "\
### Re: earlier — gpt-5
Do #old after this.

### Re: latest — gpt-5
Done.
";

        assert!(!route_owned_exchange_tail_has_unresolved_prompt(body));
    }

    fn committed_hash(content: &str) -> String {
        agent_doc_hash::content_hash(content)
    }

    #[test]
    fn route_owned_content_liveness_detects_live_backlog() {
        let content = "\
<!-- agent:exchange -->
### Re: prior — gpt-5
Done.
<!-- /agent:exchange -->

<!-- agent:backlog -->
- [ ] [#next] Continue the session
<!-- /agent:backlog -->
";

        assert_eq!(
            route_owned_liveness_reason_for_content(content, Some(&committed_hash(content))),
            Some(RouteOwnedLivenessReason::BacklogNonEmpty)
        );
    }

    #[test]
    fn route_owned_content_liveness_detects_queue_prompt() {
        let content = "\
<!-- agent:queue -->
- do #next
<!-- /agent:queue -->
";

        assert_eq!(
            route_owned_liveness_reason_for_content(content, Some(&committed_hash(content))),
            Some(RouteOwnedLivenessReason::QueueNonEmpty)
        );
    }

    #[test]
    fn route_owned_content_liveness_names_post_commit_user_follow_up() {
        let committed =
            "<!-- agent:exchange -->\n### Re: done — gpt-5\nDone.\n<!-- /agent:exchange -->\n";
        let edited = format!("{committed}\nnew prompt?\n");

        assert_eq!(
            route_owned_liveness_reason_for_content(&edited, Some(&committed_hash(committed))),
            Some(RouteOwnedLivenessReason::PostCommitUserFollowUp)
        );
    }

    #[test]
    fn route_owned_content_liveness_keeps_non_prompt_dirty_doc_alive() {
        let committed =
            "<!-- agent:exchange -->\n### Re: done — gpt-5\nDone.\n<!-- /agent:exchange -->\n";
        let edited = format!("{committed}\n<!-- local note -->\n");

        assert_eq!(
            route_owned_liveness_reason_for_content(&edited, Some(&committed_hash(committed))),
            Some(RouteOwnedLivenessReason::DocumentDirtyAfterCommit)
        );
    }

    #[test]
    fn route_owned_content_liveness_is_empty_when_no_signals() {
        let content = "\
<!-- agent:exchange -->
### Re: done — gpt-5
Done.
<!-- /agent:exchange -->

<!-- agent:backlog -->
<!-- /agent:backlog -->
";

        assert_eq!(
            route_owned_liveness_reason_for_content(content, Some(&committed_hash(content))),
            None
        );
    }
}
