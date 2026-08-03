//! Live supervisor output detection adapters.

use agent_doc_controller::dispatch::dispatch_payload_pending_in_current_input;
use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor::detection as supervisor_detection;
use portable_pty::PtySize;

pub trait SupervisorDetectionState {
    fn record_recent_output(&self, bytes: &[u8]);
    fn record_terminal_screen(&self, bytes: &[u8]);
    fn reset_terminal_screen(&self, size: PtySize);
    fn child_output_for_detection(&self) -> String;
    fn mark_prompt_visible_once(&self) -> bool;
    fn actor_known_non_ready(&self) -> bool;
    fn actor_ready(&self) -> bool;
    fn actor_busy_or_starting(&self) -> bool;
    fn owned_pane_id(&self) -> Option<String>;
    fn with_recent_output<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R;
}

impl<T> SupervisorDetectionState for std::sync::Arc<T>
where
    T: SupervisorDetectionState + ?Sized,
{
    fn record_recent_output(&self, bytes: &[u8]) {
        self.as_ref().record_recent_output(bytes);
    }

    fn record_terminal_screen(&self, bytes: &[u8]) {
        self.as_ref().record_terminal_screen(bytes);
    }

    fn reset_terminal_screen(&self, size: PtySize) {
        self.as_ref().reset_terminal_screen(size);
    }

    fn child_output_for_detection(&self) -> String {
        self.as_ref().child_output_for_detection()
    }

    fn mark_prompt_visible_once(&self) -> bool {
        self.as_ref().mark_prompt_visible_once()
    }

    fn actor_known_non_ready(&self) -> bool {
        self.as_ref().actor_known_non_ready()
    }

    fn actor_ready(&self) -> bool {
        self.as_ref().actor_ready()
    }

    fn actor_busy_or_starting(&self) -> bool {
        self.as_ref().actor_busy_or_starting()
    }

    fn owned_pane_id(&self) -> Option<String> {
        self.as_ref().owned_pane_id()
    }

    fn with_recent_output<R, F>(&self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        self.as_ref().with_recent_output(f)
    }
}

pub fn record_recent_output<S>(state: &S, bytes: &[u8])
where
    S: SupervisorDetectionState + ?Sized,
{
    state.record_recent_output(bytes);
}

pub fn record_terminal_screen<S>(state: &S, bytes: &[u8])
where
    S: SupervisorDetectionState + ?Sized,
{
    state.record_terminal_screen(bytes);
}

pub fn reset_terminal_screen<S>(state: &S, size: PtySize)
where
    S: SupervisorDetectionState + ?Sized,
{
    state.reset_terminal_screen(size);
}

pub fn child_output_for_detection<S>(state: &S) -> String
where
    S: SupervisorDetectionState + ?Sized,
{
    state.child_output_for_detection()
}

pub fn prompt_visible_requires_ready_transition<S>(state: &S) -> bool
where
    S: SupervisorDetectionState + ?Sized,
{
    supervisor_detection::prompt_visible_requires_ready_transition(
        state.mark_prompt_visible_once(),
        state.actor_known_non_ready(),
    )
}

pub fn current_child_prompt_visible<S>(state: &S, harness: &HarnessConfig) -> bool
where
    S: SupervisorDetectionState + ?Sized,
{
    harness.output_prompt_visible(&child_output_for_detection(state))
}

/// True only after the current child generation has emitted filtered PTY output.
/// The generation buffer is cleared immediately before every fresh spawn.
pub fn current_child_output_observed<S>(state: &S) -> bool
where
    S: SupervisorDetectionState + ?Sized,
{
    !child_output_for_detection(state).trim().is_empty()
}

pub fn idle_queue_prompt_visible<S>(state: &S, harness: &HarnessConfig) -> bool
where
    S: SupervisorDetectionState + ?Sized,
{
    let output = child_output_for_detection(state);
    match supervisor_detection::idle_queue_prompt_visibility(&output, harness, state.actor_ready())
    {
        supervisor_detection::IdleQueuePromptVisibility::Visible => true,
        supervisor_detection::IdleQueuePromptVisibility::Hidden => false,
        // Fresh tmux-pane evidence avoids treating a stale edge-triggered PTY
        // buffer as enough proof to drain while a restarted child redraws.
        supervisor_detection::IdleQueuePromptVisibility::NeedsLivePaneDispatchReady => {
            supervisor_detection::idle_queue_prompt_visible_after_live_pane_dispatch_ready(
                supervisor_pane_dispatch_ready(state, harness),
            )
        }
    }
}

pub fn actor_state_is_ready<S>(state: &S) -> bool
where
    S: SupervisorDetectionState + ?Sized,
{
    state.actor_ready()
}

pub fn ready_busy_blocker_reason<S>(state: &S, harness: &HarnessConfig) -> Option<String>
where
    S: SupervisorDetectionState + ?Sized,
{
    supervisor_detection::ready_busy_blocker_reason(&child_output_for_detection(state), harness)
}

pub fn supervisor_pane_dispatch_ready<S>(state: &S, harness: &HarnessConfig) -> Option<bool>
where
    S: SupervisorDetectionState + ?Sized,
{
    let pane = state.owned_pane_id()?;
    let tmux = tmux_router::Tmux::default_server();
    let content = agent_doc_tmux_io::capture_pane_with_ansi(&tmux, &pane).ok()?;
    Some(supervisor_detection::pane_dispatch_ready_at_cursor(
        &content,
        harness,
        agent_doc_tmux_io::pane_cursor_y(&tmux, &pane),
    ))
}

pub fn actor_state_is_busy_or_starting<S>(state: &S) -> bool
where
    S: SupervisorDetectionState + ?Sized,
{
    state.actor_busy_or_starting()
}

pub fn supervisor_pane_has_busy_cue<S>(state: &S, harness: &HarnessConfig) -> Option<bool>
where
    S: SupervisorDetectionState + ?Sized,
{
    let content = live_pane_content(state)?;
    Some(supervisor_detection::pane_has_busy_cue(&content, harness))
}

/// One live-pane observation used to decide whether an idle-queue payload is
/// already drafted or whether the composer is safe for a fresh dispatch.
///
/// Keeping the captured content and both classifications in one value prevents
/// diagnostics and policy from accidentally describing different tmux samples.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SupervisorPanePayloadObservation {
    pub pane_id: String,
    pub content: String,
    pub cursor_y: Option<usize>,
    pub payload_already_pending: bool,
    pub dispatch_ready: bool,
}

pub fn supervisor_pane_payload_observation<S>(
    state: &S,
    payload: &str,
    harness: &HarnessConfig,
) -> Option<SupervisorPanePayloadObservation>
where
    S: SupervisorDetectionState + ?Sized,
{
    let pane_id = state.owned_pane_id()?;
    let tmux = tmux_router::Tmux::default_server();
    let content = agent_doc_tmux_io::capture_pane_with_ansi(&tmux, &pane_id).ok()?;
    let cursor_y = agent_doc_tmux_io::pane_cursor_y(&tmux, &pane_id);
    let payload_already_pending = dispatch_payload_pending_in_current_input(
        &content,
        payload,
        |line| harness.is_dispatch_ready_prompt_line(line),
        |line| harness.is_prompt_line(line),
    );
    let dispatch_ready =
        supervisor_detection::pane_dispatch_ready_at_cursor(&content, harness, cursor_y);
    Some(SupervisorPanePayloadObservation {
        pane_id,
        content,
        cursor_y,
        payload_already_pending,
        dispatch_ready,
    })
}

pub fn supervisor_pane_payload_already_pending<S>(
    state: &S,
    payload: &str,
    harness: &HarnessConfig,
) -> Option<bool>
where
    S: SupervisorDetectionState + ?Sized,
{
    let content = live_pane_content(state)?;
    Some(dispatch_payload_pending_in_current_input(
        &content,
        payload,
        |line| harness.is_dispatch_ready_prompt_line(line),
        |line| harness.is_prompt_line(line),
    ))
}

pub fn normalize_stdin_for_harness_permission_prompt<S>(
    state: &S,
    harness: &HarnessConfig,
    data: &[u8],
) -> Option<Vec<u8>>
where
    S: SupervisorDetectionState + ?Sized,
{
    if harness.binary != "opencode" {
        return None;
    }
    let output = child_output_for_detection(state);
    state.with_recent_output(|raw| {
        agent_doc_turn_executor_tmux::prompt::normalize_opencode_permission_stdin(
            &output, raw, data,
        )
    })
}

pub fn is_help_screen_visible<S>(state: &S, harness: &HarnessConfig) -> bool
where
    S: SupervisorDetectionState + ?Sized,
{
    supervisor_detection::help_screen_visible(&child_output_for_detection(state), harness)
}

fn live_pane_content<S>(state: &S) -> Option<String>
where
    S: SupervisorDetectionState + ?Sized,
{
    let pane = state.owned_pane_id()?;
    let tmux = tmux_router::Tmux::default_server();
    agent_doc_tmux_io::capture_pane_with_ansi(&tmux, &pane).ok()
}

#[cfg(test)]
mod tests {
    #[test]
    fn dim_sensitive_live_pane_probes_preserve_ansi() {
        let source = include_str!("detection.rs")
            .split_once("\n#[cfg(test)]")
            .map(|(production, _)| production)
            .expect("tests must remain behind cfg(test)");
        assert!(
            !source.contains("agent_doc_tmux_io::capture_pane(&tmux"),
            "supervisor live-pane readiness must not strip the SGR-2 evidence that distinguishes a Codex autosuggest placeholder from operator input"
        );
        assert_eq!(
            source
                .matches("agent_doc_tmux_io::capture_pane_with_ansi(&tmux")
                .count(),
            3,
            "every supervisor live-pane observation must use the ANSI-preserving capture"
        );
    }
}
