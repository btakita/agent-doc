//! Pure supervisor realtime state model.
//!
//! This crate owns supervisor state and decisions. It does not spawn, kill,
//! unlink sockets, run tmux commands, mutate documents, or write commits.

use lazily::{ThreadSafeContext, ThreadSafeStateMachine};
use serde::{Deserialize, Serialize};

pub mod agent_change;
pub mod auto_install_stdio;
pub mod auto_trigger;
pub mod claim_binding;
pub mod config;
pub mod crash_policy;
pub mod detection;
pub mod handoff;
pub mod heartbeat;
pub mod idle_reconcile;
pub mod idle_revision;
pub mod idle_watch;
pub mod input;
pub mod ipc_protocol;
pub mod lifecycle;
pub mod recycle_inflight;
pub mod recycle_request;
pub mod recycle_yield;
pub mod reexec;
pub mod route_owned;
pub mod route_runtime;
pub mod run_loop;
pub mod selfkill;
pub mod session_owner;
pub mod startup_miss;
pub mod terminal_filter;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorState {
    Starting,
    Ready,
    Busy,
    Blocked,
    Stale,
    Dead,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorEvent {
    Spawned,
    ReadyObserved,
    TurnStarted,
    TurnFinished,
    BlockedObserved,
    HeartbeatStale,
    ProcessDead,
    UserClosed,
    Restarted,
}

pub fn transition_supervisor(
    current: &SupervisorState,
    event: &SupervisorEvent,
) -> Option<SupervisorState> {
    use SupervisorEvent::*;
    use SupervisorState::*;

    match (*current, *event) {
        (Starting, ReadyObserved) | (Stale, Restarted) | (Dead, Restarted) => Some(Ready),
        (Ready, TurnStarted) => Some(Busy),
        (Busy, TurnFinished) => Some(Ready),
        (Ready | Busy | Starting | Stale, BlockedObserved) => Some(Blocked),
        (Ready | Busy | Starting | Blocked, HeartbeatStale) => Some(Stale),
        (Ready | Busy | Starting | Blocked | Stale, ProcessDead) => Some(Dead),
        (_, UserClosed) => Some(Closed),
        (Closed, _) => None,
        _ => None,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SupervisorBinding {
    pub pane_id: String,
    pub generation: u64,
    pub supervisor_instance_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct OwnershipGeneration {
    pub prior_generation: u64,
    pub new_generation: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnershipTransitionEvent<'a> {
    pub caller: &'a str,
    pub reason: &'a str,
    pub prior_generation: u64,
    pub new_generation: u64,
    pub old_pane: Option<&'a str>,
    pub new_pane: &'a str,
    pub old_window: Option<&'a str>,
    pub new_window: Option<&'a str>,
}

fn extract_event(raw_line: &str) -> &str {
    raw_line
        .split_once("] ")
        .map(|(_, event)| event)
        .unwrap_or(raw_line)
        .trim()
}

fn parse_u64_field(event: &str, name: &str) -> Option<u64> {
    event.split_whitespace().find_map(|part| {
        part.strip_prefix(name)
            .and_then(|value| value.parse::<u64>().ok())
    })
}

fn render_field<'a>(value: Option<&'a str>, fallback: &'a str) -> &'a str {
    value.filter(|value| !value.is_empty()).unwrap_or(fallback)
}

pub fn infer_latest_generation_from_content(content: &str) -> u64 {
    let mut latest_explicit = 0u64;
    let mut legacy_session_starts = 0u64;

    for raw_line in content.lines() {
        let event = extract_event(raw_line);
        if event.is_empty() {
            continue;
        }
        if event.starts_with("session_start ") {
            legacy_session_starts += 1;
            if let Some(generation) = parse_u64_field(event, "generation=") {
                latest_explicit = latest_explicit.max(generation);
            }
            continue;
        }
        if event.starts_with("ownership_transition ")
            && let Some(generation) = parse_u64_field(event, "new_generation=")
        {
            latest_explicit = latest_explicit.max(generation);
        }
    }

    if latest_explicit > 0 {
        latest_explicit
    } else {
        legacy_session_starts
    }
}

pub fn format_transition_event(event: OwnershipTransitionEvent<'_>) -> String {
    format!(
        "ownership_transition caller={} reason={} prior_generation={} new_generation={} old_pane={} new_pane={} old_window={} new_window={}",
        event.caller,
        event.reason,
        event.prior_generation,
        event.new_generation,
        render_field(event.old_pane, "none"),
        render_field(Some(event.new_pane), "none"),
        render_field(event.old_window, "none"),
        render_field(event.new_window, "unknown"),
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SupervisorRecoveryAction {
    None,
    RestartSamePane,
    CloseStaleActor,
    RefuseDispatch,
}

pub fn recovery_action(state: SupervisorState, pane_alive: bool) -> SupervisorRecoveryAction {
    match (state, pane_alive) {
        (SupervisorState::Ready | SupervisorState::Busy, _) => SupervisorRecoveryAction::None,
        (SupervisorState::Stale | SupervisorState::Dead, true) => {
            SupervisorRecoveryAction::RestartSamePane
        }
        (SupervisorState::Stale | SupervisorState::Dead, false) => {
            SupervisorRecoveryAction::CloseStaleActor
        }
        (SupervisorState::Closed, _) => SupervisorRecoveryAction::CloseStaleActor,
        _ => SupervisorRecoveryAction::RefuseDispatch,
    }
}

pub struct SupervisorMachine {
    ctx: ThreadSafeContext,
    machine: ThreadSafeStateMachine<SupervisorState, SupervisorEvent>,
}

impl SupervisorMachine {
    /// Join this machine to `scope`'s graph (`#stategraphjoin`).
    ///
    /// A supervisor actor is bound to one document's session; a new document gets a new actor, and dispatch/recovery decisions must not leak between documents.
    ///
    /// The scope owns the context, so dropping the scope drops this machine's cells —
    /// teardown is the scope's lifetime, not a separate deregistration step.
    pub fn new_in(scope: &agent_doc_state_scope::DocumentScope, initial: SupervisorState) -> Self {
        let ctx = scope.ctx().clone();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_supervisor);
        Self { ctx, machine }
    }

    /// Standalone machine in a private context — a pure-transition helper for unit
    /// tests only.
    ///
    /// A long-lived owner must use [`Self::new_in`] instead: nothing outside a private
    /// context can derive from its cells and invalidation never crosses it, so a
    /// `Computed` built over one is Computed in name only.
    pub fn new(initial: SupervisorState) -> Self {
        // #stategraphjoin-allow: standalone pure-transition helper kept beside `new_in` for unit tests; no long-lived owner holds it.
        let ctx = ThreadSafeContext::new();
        let machine = ThreadSafeStateMachine::new(&ctx, initial, transition_supervisor);
        Self { ctx, machine }
    }

    pub fn send(&self, event: SupervisorEvent) -> bool {
        self.machine.send(&self.ctx, event)
    }

    pub fn state(&self) -> SupervisorState {
        self.machine.state(&self.ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stale_supervisor_restarts_same_live_pane() {
        assert_eq!(
            recovery_action(SupervisorState::Stale, true),
            SupervisorRecoveryAction::RestartSamePane
        );
    }

    #[test]
    fn busy_turn_returns_to_ready() {
        let machine = SupervisorMachine::new(SupervisorState::Ready);
        assert!(machine.send(SupervisorEvent::TurnStarted));
        assert_eq!(machine.state(), SupervisorState::Busy);
        assert!(machine.send(SupervisorEvent::TurnFinished));
        assert_eq!(machine.state(), SupervisorState::Ready);
    }

    #[test]
    fn infer_latest_generation_counts_legacy_session_starts() {
        let content = concat!(
            "[1] session_start file=doc.md pane=%41 session=session-1\n",
            "[2] codex_start mode=fresh restart_count=0\n",
            "[10] session_start file=doc.md pane=%52 session=session-1\n",
        );

        assert_eq!(infer_latest_generation_from_content(content), 2);
    }

    #[test]
    fn infer_latest_generation_prefers_explicit_generation_markers() {
        let content = concat!(
            "[1] ownership_transition caller=start reason=session_start prior_generation=0 new_generation=1 old_pane=none new_pane=%41 old_window=none new_window=@1\n",
            "[2] session_start file=doc.md pane=%41 session=session-1 generation=1\n",
            "[10] ownership_transition caller=route reason=registry_rebind prior_generation=1 new_generation=2 old_pane=%41 new_pane=%52 old_window=@1 new_window=@2\n",
        );

        assert_eq!(infer_latest_generation_from_content(content), 2);
    }

    #[test]
    fn format_transition_event_uses_stable_placeholders() {
        let rendered = format_transition_event(OwnershipTransitionEvent {
            caller: "start",
            reason: "session_start",
            prior_generation: 0,
            new_generation: 1,
            old_pane: None,
            new_pane: "%41",
            old_window: None,
            new_window: Some("@1"),
        });

        assert_eq!(
            rendered,
            "ownership_transition caller=start reason=session_start prior_generation=0 new_generation=1 old_pane=none new_pane=%41 old_window=none new_window=@1"
        );
    }
}
