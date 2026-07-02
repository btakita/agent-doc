//! Flow event logging adapters.
//!
//! Pure flow vocabulary and log-line formatting live in `agent-doc-flow`.
//! This crate owns the tiny IO adapter that sends a formatted [`FlowEvent`]
//! through an injected best-effort logger, keeping orchestration from retaining
//! a flow-proof facade.

use agent_doc_flow::types::{FlowEvent, flow_event_log_message};
use std::path::Path;

/// Best-effort logger shape used by orchestration's ops log.
pub type FlowEventLogger = fn(&Path, &str);

/// Render the canonical flow-event ops-log line for `file`.
pub fn flow_event_message(file: &Path, event: &FlowEvent) -> String {
    flow_event_log_message(&file.display().to_string(), event)
}

/// Log a typed flow event through an injected sink.
pub fn log_flow_event(file: &Path, event: FlowEvent, mut logger: impl FnMut(&Path, &str)) {
    let message = flow_event_message(file, &event);
    logger(file, &message);
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_flow::types::{FlowName, FlowOutcome, FlowStage};
    use std::cell::RefCell;

    #[test]
    fn log_flow_event_uses_canonical_message_and_injected_sink() {
        let file = Path::new("tasks/a.md");
        let event = FlowEvent::new(
            FlowName::DocumentMutation,
            FlowStage::PreWriteGuard,
            FlowOutcome::Blocked,
        )
        .with_reason("visible write changed");
        let logged = RefCell::new(Vec::new());

        log_flow_event(file, event, |path, message| {
            logged
                .borrow_mut()
                .push((path.display().to_string(), message.to_string()));
        });

        let logged = logged.borrow();
        assert_eq!(logged.len(), 1);
        assert_eq!(logged[0].0, "tasks/a.md");
        assert!(logged[0].1.contains("flow=document_mutation"));
        assert!(logged[0].1.contains("stage=pre_write_guard"));
        assert!(logged[0].1.contains("reason=visible_write_changed"));
    }
}
