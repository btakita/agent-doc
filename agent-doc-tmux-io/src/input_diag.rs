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
