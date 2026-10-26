//! Effectful diagnostics adapter for tmux and supervisor input delivery.
//!
//! Pure structured formatting policy lives in `agent-doc-tmux-commands`.
//! This module only mirrors opted-in diagnostics to stderr and writes ops.log.

use agent_doc_tmux_commands::input_diag::{self, KeyEventMeta};
use std::path::Path;

pub type OpsLogFn = fn(&Path, &str);

#[derive(Clone, Copy)]
pub struct InputDiagSink<'a> {
    file: Option<&'a Path>,
    log_op: OpsLogFn,
}

impl<'a> InputDiagSink<'a> {
    pub fn new(file: Option<&'a Path>, log_op: OpsLogFn) -> Self {
        Self { file, log_op }
    }
}

fn emit(sink: InputDiagSink<'_>, message: String) {
    // Input-delivery diagnostics are debug-level. Writing them to stderr
    // unconditionally bleeds them in front of a full-screen harness TUI (e.g.
    // OpenCode), interleaving with its status line. Keep the durable record in
    // ops.log always, but only surface on stderr when the operator opted into
    // verbose input diagnostics. (#opencode-stdout-bleed)
    if input_diag::verbose_enabled() {
        eprintln!("[agent-doc] {message}");
    }
    if let Some(file) = sink.file {
        (sink.log_op)(file, &message);
    }
}

pub fn log_key_event(
    sink: InputDiagSink<'_>,
    source: &str,
    destination: &str,
    transform: &str,
    key: &str,
    bytes: usize,
    meta: KeyEventMeta<'_>,
) {
    emit(
        sink,
        input_diag::format_key_event(
            source,
            destination,
            transform,
            key,
            bytes,
            meta.harness,
            meta.detail,
        ),
    );
}

pub fn log_key_event_verbose(
    sink: InputDiagSink<'_>,
    source: &str,
    destination: &str,
    transform: &str,
    key: &str,
    bytes: usize,
    meta: KeyEventMeta<'_>,
) {
    if input_diag::verbose_enabled() {
        log_key_event(sink, source, destination, transform, key, bytes, meta);
    }
}

pub fn log_text_submit(
    sink: InputDiagSink<'_>,
    source: &str,
    destination: &str,
    text: &str,
    harness: Option<&str>,
    transform: &str,
    submit_key: &str,
) {
    emit(
        sink,
        input_diag::format_payload_event(
            source,
            destination,
            transform,
            "text",
            text.as_bytes(),
            harness,
        ),
    );
    log_key_event(
        sink,
        source,
        destination,
        transform,
        submit_key,
        submit_key.len(),
        KeyEventMeta {
            harness,
            detail: None,
        },
    );
}

/// Record a command launched as a pane's own process
/// ([`crate::launch_command_in_pane_at`]). Like [`log_text_submit`] it logs
/// only the command's length and SHA-256, never its text, under
/// `key=launch`; unlike it, there is no submit-key event, because nothing is
/// typed into the pane.
pub fn log_pane_launch(
    sink: InputDiagSink<'_>,
    source: &str,
    destination: &str,
    command: &str,
    harness: Option<&str>,
    transform: &str,
) {
    emit(
        sink,
        input_diag::format_payload_event(
            source,
            destination,
            transform,
            "launch",
            command.as_bytes(),
            harness,
        ),
    );
}

pub fn log_byte_events(
    sink: InputDiagSink<'_>,
    source: &str,
    destination: &str,
    transform: &str,
    bytes: &[u8],
    harness: Option<&str>,
) {
    for byte in bytes {
        emit(
            sink,
            input_diag::format_byte_event(source, destination, transform, *byte, harness),
        );
    }
}

pub fn log_transform_event(
    sink: InputDiagSink<'_>,
    source: &str,
    destination: &str,
    transform: &str,
    before: &[u8],
    after: &[u8],
    harness: Option<&str>,
) {
    emit(
        sink,
        input_diag::format_transform_event(source, destination, transform, before, after, harness),
    );
}

pub fn log_prompt_detection(
    sink: InputDiagSink<'_>,
    source: &str,
    destination: &str,
    harness: &str,
    reason: &str,
    state: &str,
) {
    emit(
        sink,
        input_diag::format_prompt_detection(source, destination, harness, reason, state),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;

    thread_local! {
        static LOGGED: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    }

    fn record(_file: &Path, message: &str) {
        LOGGED.with(|logged| logged.borrow_mut().push(message.to_string()));
    }

    #[test]
    fn pane_launch_logs_one_hashed_launch_event_and_no_submit_key() {
        let command = "export K=\"$(passage show k)\" && claude -p";

        log_pane_launch(
            InputDiagSink::new(Some(Path::new("doc.md")), record),
            "route.auto_start",
            "pane:%3",
            command,
            Some("claude"),
            "route_owned_start_launch",
        );

        let logged = LOGGED.with(|logged| logged.borrow().clone());
        assert_eq!(logged.len(), 1, "{logged:?}");
        let event = &logged[0];
        assert!(
            event.starts_with(
                "tmux_input_event source=route.auto_start destination=pane:%3 transform=route_owned_start_launch key=launch "
            ),
            "{event}"
        );
        assert!(
            event.contains(&format!("bytes={}", command.len())),
            "{event}"
        );
        assert!(
            event.contains(&format!(
                "sha256={}",
                input_diag::bytes_hash(command.as_bytes())
            )),
            "{event}"
        );
        assert!(event.contains("harness=claude"), "{event}");
        assert!(
            !event.contains("passage") && !event.contains("Enter"),
            "the command text and a submit key never reach the log: {event}"
        );
    }
}
