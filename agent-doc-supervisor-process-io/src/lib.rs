//! Supervisor process observer adapters.

use agent_doc_harness::HarnessConfig;
use agent_doc_supervisor_io::detection::{
    SupervisorDetectionState, current_child_prompt_visible,
    normalize_stdin_for_harness_permission_prompt, prompt_visible_requires_ready_transition,
    record_recent_output, record_terminal_screen,
};
use agent_doc_supervisor_process::io_threads::{PtyReaderObserver, StdinForwardObserver};

pub trait SupervisorProcessIoState: SupervisorDetectionState + Send + Sync + 'static {
    fn transition_actor_ready_for_prompt(&self);
    fn clear_suppress_stale_ctrl_d_until_prompt(&self);
    fn suppress_stale_ctrl_d_until_prompt(&self) -> bool;
    fn prompt_visible_once(&self) -> bool;
}

impl<T> SupervisorProcessIoState for std::sync::Arc<T>
where
    T: SupervisorProcessIoState + ?Sized,
{
    fn transition_actor_ready_for_prompt(&self) {
        self.as_ref().transition_actor_ready_for_prompt();
    }

    fn clear_suppress_stale_ctrl_d_until_prompt(&self) {
        self.as_ref().clear_suppress_stale_ctrl_d_until_prompt();
    }

    fn suppress_stale_ctrl_d_until_prompt(&self) -> bool {
        self.as_ref().suppress_stale_ctrl_d_until_prompt()
    }

    fn prompt_visible_once(&self) -> bool {
        self.as_ref().prompt_visible_once()
    }
}

pub struct SupervisorProcessIoObserver<S> {
    state: S,
}

impl<S> SupervisorProcessIoObserver<S> {
    pub fn new(state: S) -> Self {
        Self { state }
    }
}

impl<S> PtyReaderObserver for SupervisorProcessIoObserver<S>
where
    S: SupervisorProcessIoState,
{
    fn on_filtered_pty_output(&self, harness: &HarnessConfig, bytes: &[u8]) {
        observe_filtered_pty_output(&self.state, harness, bytes);
    }
}

impl<S> StdinForwardObserver for SupervisorProcessIoObserver<S>
where
    S: SupervisorProcessIoState,
{
    fn suppress_stale_ctrl_d_until_prompt(&self) -> bool {
        self.state.suppress_stale_ctrl_d_until_prompt()
    }

    fn prompt_visible_once(&self) -> bool {
        self.state.prompt_visible_once()
    }

    fn normalize_permission_prompt_input(
        &self,
        harness: &HarnessConfig,
        data: &[u8],
    ) -> Option<Vec<u8>> {
        normalize_stdin_for_harness_permission_prompt(&self.state, harness, data)
    }
}

pub fn observe_filtered_pty_output<S>(state: &S, harness: &HarnessConfig, bytes: &[u8])
where
    S: SupervisorProcessIoState + ?Sized,
{
    record_terminal_screen(state, bytes);
    record_recent_output(state, bytes);
    if current_child_prompt_visible(state, harness) {
        if prompt_visible_requires_ready_transition(state) {
            state.transition_actor_ready_for_prompt();
        }
        state.clear_suppress_stale_ctrl_d_until_prompt();
    }
}
