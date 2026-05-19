use super::types::{FlowEvent, FlowName, FlowOutcome, FlowStage};
use std::path::Path;

pub(crate) fn flow_event_log_message(file: &Path, event: &FlowEvent) -> String {
    let mut message = format!(
        "flow_event file={} flow={} stage={} outcome={}",
        file.display(),
        event.flow.as_str(),
        event.stage.as_str(),
        event.outcome.as_str()
    );
    if let Some(reason) = &event.reason {
        message.push_str(" reason=");
        message.push_str(&sanitize_field_value(reason));
    }
    message
}

pub(crate) fn log_flow_event(file: &Path, event: FlowEvent) {
    crate::ops_log::log_op(file, &flow_event_log_message(file, &event));
}

pub(crate) fn flow_event(
    flow: FlowName,
    stage: FlowStage,
    outcome: FlowOutcome,
    reason: impl Into<Option<String>>,
) -> FlowEvent {
    let mut event = FlowEvent::new(flow, stage, outcome);
    if let Some(reason) = reason.into() {
        event = event.with_reason(reason);
    }
    event
}

fn sanitize_field_value(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/') {
                ch
            } else {
                '_'
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::flow::types::{FlowName, FlowOutcome, FlowStage};

    #[test]
    fn flow_event_log_message_is_field_parseable() {
        let event = FlowEvent::new(
            FlowName::RoutedReopen,
            FlowStage::PromptReadyBarrier,
            FlowOutcome::FailedClosed,
        )
        .with_reason("starting actor not ready");

        let message = flow_event_log_message(Path::new("tasks/a.md"), &event);

        assert!(message.contains("flow=routed_reopen"));
        assert!(message.contains("stage=prompt_ready_barrier"));
        assert!(message.contains("outcome=failed_closed"));
        assert!(message.contains("reason=starting_actor_not_ready"));
    }
}
