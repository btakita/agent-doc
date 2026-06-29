//! Pure supervisor child crash classification and restart policy.
//!
//! Crash classifier, restart history ring buffer, and supervisor state machine.
//!
//! ## Spec
//! See `src/agent-doc/specs/supervisor.md` § Crash Recovery Policy.
//!
//! The supervisor tracks every child exit in a bounded ring buffer and uses the
//! recent-exit frequency to classify each exit as Clean, Transient, or Flapping.
//! The classification drives a state machine (Healthy -> Degraded -> Halted) that
//! determines whether and how the child should be restarted.
//!
//! ## State Machine
//!
//! ```text
//! on exit code c:
//!   append to ring buffer (last 10 exits with timestamps)
//!   classify:
//!     c == 0                          -> Clean
//!     c != 0 AND exits_in_60s < 3    -> Transient
//!     c != 0 AND exits_in_60s >= 3   -> Flapping
//!   transition:
//!     Clean     -> Healthy, reset consecutive counter
//!     Transient -> Healthy
//!     Flapping  -> Degraded, increment consecutive flap counter
//!                  5th consecutive -> Halted
//! ```
//!
//! ## Invariants
//!
//! - Ring buffer is capped at 10 entries. Older entries are evicted on push.
//! - `exits_in_window` only counts non-zero exit codes within the time window.
//! - Halted non-zero exits keep returning `RestartAction::Halt`; callers can
//!   explicitly clear the policy with `CrashPolicy::reset`.
//! - `RestartAction` is a value type — the caller (future `start.rs` wire-up)
//!   matches on it and performs the actual sleep/prompt/halt.

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Maximum entries in the restart history ring buffer.
const RING_BUFFER_CAP: usize = 10;

/// Window for flap detection: exits within this duration count toward the
/// flapping threshold.
const FLAP_WINDOW: Duration = Duration::from_secs(60);

/// Number of non-zero exits within [`FLAP_WINDOW`] that triggers Flapping.
const FLAP_THRESHOLD: usize = 3;

/// Number of consecutive Flapping classifications before Halted.
const HALT_THRESHOLD: usize = 5;

/// Delay before restarting after a Transient exit.
const TRANSIENT_DELAY: Duration = Duration::from_secs(2);

/// Delay before restarting after a Flapping exit.
const FLAPPING_DELAY: Duration = Duration::from_secs(30);

/// Classification of a single child exit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitClass {
    /// Exit code 0 — clean shutdown.
    Clean,
    /// Non-zero exit, but below the flapping threshold in the recent window.
    Transient,
    /// Non-zero exit with >= FLAP_THRESHOLD exits in the last 60s.
    Flapping,
}

/// Supervisor health state, surfaced via IPC `state` method.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorState {
    /// Normal operation. Restarts are immediate or short-delayed.
    Healthy,
    /// Flapping detected. Restarts use longer delays.
    Degraded,
    /// Too many consecutive failures. Supervisor will not restart until
    /// externally resumed.
    Halted,
}

impl SupervisorState {
    /// Stable string tag for IPC responses and logs.
    pub fn as_str(&self) -> &'static str {
        match self {
            SupervisorState::Healthy => "healthy",
            SupervisorState::Degraded => "degraded",
            SupervisorState::Halted => "halted",
        }
    }
}

/// What the caller should do after a child exit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RestartAction {
    /// Prompt the user (Enter to restart fresh, 'q' to quit).
    PromptUser,
    /// Wait `delay`, then restart with the given flags.
    RestartAfter {
        delay: Duration,
        with_continue: bool,
    },
    /// Do not restart. Supervisor is halted.
    Halt,
}

/// Parsed operator response to the supervisor restart/quit prompt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SupervisorPromptDecision {
    RestartFresh,
    Quit,
    QuitEof,
    Invalid,
}

/// Classify raw stdin input for the supervisor restart/quit prompt.
pub fn classify_supervisor_prompt_input(
    bytes_read: usize,
    input: &str,
) -> SupervisorPromptDecision {
    if bytes_read == 0 {
        return SupervisorPromptDecision::QuitEof;
    }
    let trimmed = input.trim();
    if trimmed.eq_ignore_ascii_case("q") {
        return SupervisorPromptDecision::Quit;
    }
    if trimmed.is_empty() {
        return SupervisorPromptDecision::RestartFresh;
    }
    SupervisorPromptDecision::Invalid
}

/// Normalize intentional forwarded Ctrl-C shutdowns onto the clean-exit path.
pub fn supervisor_policy_exit_code(exit_code: i32, ctrl_c_forwarded_interrupt: bool) -> i32 {
    if ctrl_c_forwarded_interrupt {
        0
    } else {
        exit_code
    }
}

/// A single exit event recorded in the ring buffer.
#[derive(Debug, Clone)]
pub struct ExitRecord {
    /// Child exit code.
    pub code: i32,
    /// When the exit was observed.
    pub timestamp: Instant,
}

/// Bounded ring buffer of recent child exits.
#[derive(Debug)]
struct RestartHistory {
    entries: VecDeque<ExitRecord>,
}

impl RestartHistory {
    pub fn new() -> Self {
        Self {
            entries: VecDeque::with_capacity(RING_BUFFER_CAP),
        }
    }

    /// Record an exit. Evicts the oldest entry if at capacity.
    pub fn push(&mut self, record: ExitRecord) {
        if self.entries.len() >= RING_BUFFER_CAP {
            self.entries.pop_front();
        }
        self.entries.push_back(record);
    }

    /// Count non-zero exits within `window` of `now`.
    pub fn exits_in_window(&self, window: Duration, now: Instant) -> usize {
        let cutoff = now.checked_sub(window).unwrap_or(now);
        self.entries
            .iter()
            .filter(|r| r.code != 0 && r.timestamp >= cutoff)
            .count()
    }

    /// Number of entries in the buffer.
    #[allow(dead_code)] // API surface — used by tests
    pub fn len(&self) -> usize {
        self.entries.len()
    }
}

/// Crash policy: classifies exits, tracks state, and recommends restart actions.
#[derive(Debug)]
pub struct CrashPolicy {
    pub state: SupervisorState,
    history: RestartHistory,
    /// Number of consecutive Flapping classifications without an intervening
    /// Clean exit.
    consecutive_flaps: usize,
}

impl CrashPolicy {
    pub fn new() -> Self {
        Self {
            state: SupervisorState::Healthy,
            history: RestartHistory::new(),
            consecutive_flaps: 0,
        }
    }

    /// Classify an exit, record it, transition state, and return the
    /// recommended action.
    ///
    /// This is the main entry point called by the supervisor restart loop.
    pub fn on_exit(&mut self, exit_code: i32) -> RestartAction {
        self.on_exit_at(exit_code, Instant::now())
    }

    /// Like [`on_exit`](Self::on_exit) but with an explicit timestamp for
    /// deterministic testing.
    pub fn on_exit_at(&mut self, exit_code: i32, now: Instant) -> RestartAction {
        let record = ExitRecord {
            code: exit_code,
            timestamp: now,
        };
        self.history.push(record);

        let class = self.classify(exit_code, now);
        self.transition(class);
        self.action(class)
    }

    /// Classify the exit based on code and recent history.
    fn classify(&self, exit_code: i32, now: Instant) -> ExitClass {
        if exit_code == 0 {
            return ExitClass::Clean;
        }
        if self.history.exits_in_window(FLAP_WINDOW, now) >= FLAP_THRESHOLD {
            ExitClass::Flapping
        } else {
            ExitClass::Transient
        }
    }

    /// Update supervisor state based on the exit classification.
    fn transition(&mut self, class: ExitClass) {
        match class {
            ExitClass::Clean => {
                self.state = SupervisorState::Healthy;
                self.consecutive_flaps = 0;
            }
            ExitClass::Transient => {
                // Transient does not escalate — stay in current state or
                // return to Healthy if previously Degraded.
                if self.state == SupervisorState::Degraded {
                    // A transient (non-flapping) exit while Degraded means
                    // the flap storm has subsided.
                    self.state = SupervisorState::Healthy;
                    self.consecutive_flaps = 0;
                }
            }
            ExitClass::Flapping => {
                self.consecutive_flaps += 1;
                if self.consecutive_flaps >= HALT_THRESHOLD {
                    self.state = SupervisorState::Halted;
                } else {
                    self.state = SupervisorState::Degraded;
                }
            }
        }
    }

    /// Map the classification to a concrete restart action.
    fn action(&self, class: ExitClass) -> RestartAction {
        if self.state == SupervisorState::Halted {
            return RestartAction::Halt;
        }
        match class {
            ExitClass::Clean => RestartAction::PromptUser,
            ExitClass::Transient => RestartAction::RestartAfter {
                delay: TRANSIENT_DELAY,
                with_continue: true,
            },
            ExitClass::Flapping => RestartAction::RestartAfter {
                delay: FLAPPING_DELAY,
                with_continue: true,
            },
        }
    }

    /// Reset the policy to Healthy with empty history. Called by
    /// `supervisor resume` to un-halt.
    #[allow(dead_code)] // API surface — consumed by IPC resume handler (future)
    pub fn reset(&mut self) {
        self.state = SupervisorState::Healthy;
        self.history = RestartHistory::new();
        self.consecutive_flaps = 0;
    }

    /// Current number of consecutive flapping exits.
    #[allow(dead_code)] // API surface — used by tests and IPC state response (future)
    pub fn consecutive_flaps(&self) -> usize {
        self.consecutive_flaps
    }
}

impl Default for CrashPolicy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Create an Instant offset by `secs` from a base.
    fn offset(base: Instant, secs: u64) -> Instant {
        base + Duration::from_secs(secs)
    }

    #[test]
    fn classify_clean_exit() {
        let mut policy = CrashPolicy::new();
        let action = policy.on_exit_at(0, Instant::now());
        assert_eq!(action, RestartAction::PromptUser);
        assert_eq!(policy.state, SupervisorState::Healthy);
    }

    #[test]
    fn classify_supervisor_prompt_input_quits_on_q() {
        assert_eq!(
            classify_supervisor_prompt_input(2, "q\n"),
            SupervisorPromptDecision::Quit
        );
        assert_eq!(
            classify_supervisor_prompt_input(2, "Q\n"),
            SupervisorPromptDecision::Quit
        );
    }

    #[test]
    fn classify_supervisor_prompt_input_restarts_on_blank_line() {
        assert_eq!(
            classify_supervisor_prompt_input(1, "\n"),
            SupervisorPromptDecision::RestartFresh
        );
    }

    #[test]
    fn classify_supervisor_prompt_input_quits_on_eof() {
        assert_eq!(
            classify_supervisor_prompt_input(0, ""),
            SupervisorPromptDecision::QuitEof
        );
    }

    #[test]
    fn classify_supervisor_prompt_input_rejects_unrecognized_input() {
        assert_eq!(
            classify_supervisor_prompt_input(4, "yes\n"),
            SupervisorPromptDecision::Invalid
        );
    }

    #[test]
    fn forwarded_ctrl_c_uses_clean_exit_code_for_policy() {
        assert_eq!(supervisor_policy_exit_code(1, true), 0);
        assert_eq!(supervisor_policy_exit_code(130, true), 0);
        assert_eq!(supervisor_policy_exit_code(1, false), 1);
    }

    #[test]
    fn classify_single_nonzero_is_transient() {
        let mut policy = CrashPolicy::new();
        let action = policy.on_exit_at(1, Instant::now());
        assert_eq!(
            action,
            RestartAction::RestartAfter {
                delay: TRANSIENT_DELAY,
                with_continue: true,
            }
        );
        assert_eq!(policy.state, SupervisorState::Healthy);
    }

    #[test]
    fn classify_three_in_60s_is_flapping() {
        let mut policy = CrashPolicy::new();
        let base = Instant::now();
        policy.on_exit_at(1, offset(base, 0));
        policy.on_exit_at(1, offset(base, 10));
        let action = policy.on_exit_at(1, offset(base, 20));
        assert_eq!(
            action,
            RestartAction::RestartAfter {
                delay: FLAPPING_DELAY,
                with_continue: true,
            }
        );
        assert_eq!(policy.state, SupervisorState::Degraded);
    }

    #[test]
    fn ring_buffer_caps_at_10() {
        let mut history = RestartHistory::new();
        let base = Instant::now();
        for i in 0..15 {
            history.push(ExitRecord {
                code: 1,
                timestamp: offset(base, i),
            });
        }
        assert_eq!(history.len(), RING_BUFFER_CAP);
    }

    #[test]
    fn exits_in_window_excludes_old() {
        let mut history = RestartHistory::new();
        let base = Instant::now();
        // Old exit — 120s ago
        history.push(ExitRecord {
            code: 1,
            timestamp: base,
        });
        // Recent exit — 10s ago
        let now = offset(base, 120);
        history.push(ExitRecord {
            code: 1,
            timestamp: offset(base, 110),
        });
        assert_eq!(
            history.exits_in_window(FLAP_WINDOW, now),
            1,
            "only the recent exit should count"
        );
    }

    #[test]
    fn flapping_transitions_to_degraded() {
        let mut policy = CrashPolicy::new();
        let base = Instant::now();
        // Push 3 exits within 60s
        policy.on_exit_at(1, offset(base, 0));
        policy.on_exit_at(1, offset(base, 1));
        policy.on_exit_at(1, offset(base, 2));
        assert_eq!(policy.state, SupervisorState::Degraded);
        assert_eq!(policy.consecutive_flaps(), 1);
    }

    #[test]
    fn five_consecutive_flaps_halts() {
        let mut policy = CrashPolicy::new();
        let base = Instant::now();
        // Each batch of 3 exits in quick succession triggers Flapping.
        // We need 5 Flapping classifications to halt.
        // After the 3rd exit in each batch, classification becomes Flapping
        // because there are >= 3 non-zero exits in the 60s window.
        // Since all exits are within 60s, every exit after the 2nd is Flapping.
        for i in 0..7 {
            policy.on_exit_at(1, offset(base, i));
        }
        // Exits: #1 Transient, #2 Transient, #3 Flapping, #4 Flapping,
        //        #5 Flapping, #6 Flapping, #7 Flapping = 5 flaps → Halted
        assert_eq!(policy.state, SupervisorState::Halted);
    }

    #[test]
    fn nonzero_exit_while_halted_returns_halt() {
        let mut policy = CrashPolicy::new();
        let base = Instant::now();
        // Drive to Halted
        for i in 0..7 {
            policy.on_exit_at(1, offset(base, i));
        }
        assert_eq!(policy.state, SupervisorState::Halted);

        // The non-zero path stays halted until reset.
        let action = policy.on_exit_at(1, offset(base, 8));
        assert_eq!(action, RestartAction::Halt);
    }

    #[test]
    fn clean_exit_resets_consecutive_flaps() {
        let mut policy = CrashPolicy::new();
        let base = Instant::now();
        // Drive to Degraded (3 quick failures)
        policy.on_exit_at(1, offset(base, 0));
        policy.on_exit_at(1, offset(base, 1));
        policy.on_exit_at(1, offset(base, 2));
        assert_eq!(policy.state, SupervisorState::Degraded);
        assert!(policy.consecutive_flaps() > 0);

        // Clean exit resets
        policy.on_exit_at(0, offset(base, 3));
        assert_eq!(policy.state, SupervisorState::Healthy);
        assert_eq!(policy.consecutive_flaps(), 0);
    }

    #[test]
    fn reset_clears_everything() {
        let mut policy = CrashPolicy::new();
        let base = Instant::now();
        for i in 0..7 {
            policy.on_exit_at(1, offset(base, i));
        }
        assert_eq!(policy.state, SupervisorState::Halted);

        policy.reset();
        assert_eq!(policy.state, SupervisorState::Healthy);
        assert_eq!(policy.consecutive_flaps(), 0);
        assert_eq!(policy.history.len(), 0);
    }

    #[test]
    fn state_tag_strings_are_stable() {
        assert_eq!(SupervisorState::Healthy.as_str(), "healthy");
        assert_eq!(SupervisorState::Degraded.as_str(), "degraded");
        assert_eq!(SupervisorState::Halted.as_str(), "halted");
    }

    #[test]
    fn transient_while_degraded_recovers_to_healthy() {
        let mut policy = CrashPolicy::new();
        let base = Instant::now();
        // Drive to Degraded
        policy.on_exit_at(1, offset(base, 0));
        policy.on_exit_at(1, offset(base, 1));
        policy.on_exit_at(1, offset(base, 2));
        assert_eq!(policy.state, SupervisorState::Degraded);

        // Wait long enough that the old exits fall outside the 60s window,
        // then a single failure is Transient (< 3 in window).
        let later = offset(base, 120);
        let action = policy.on_exit_at(1, later);
        assert_eq!(
            action,
            RestartAction::RestartAfter {
                delay: TRANSIENT_DELAY,
                with_continue: true,
            }
        );
        assert_eq!(policy.state, SupervisorState::Healthy);
    }
}
