//! Structured diagnostics for tmux and supervisor input delivery.
//!
//! These helpers intentionally avoid logging raw prompt text. Input payloads
//! are represented by byte length and SHA-256 so tests and operators can prove
//! which delivery path ran without leaking typed content into logs.

use sha2::{Digest, Sha256};
use std::path::Path;

const PREFIX: &str = "tmux_input_event";

#[derive(Clone, Copy, Default)]
pub struct KeyEventMeta<'a> {
    pub harness: Option<&'a str>,
    pub detail: Option<&'a str>,
}

fn sanitize_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | ':' | '/' | '%' | '=') {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "none".to_string()
    } else {
        out
    }
}

fn bytes_hash(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex::encode(hasher.finalize())
}

fn emit(file: Option<&Path>, message: String) {
    // Input-delivery diagnostics are debug-level. Writing them to stderr
    // unconditionally bleeds them in front of a full-screen harness TUI (e.g.
    // OpenCode), interleaving with its status line. Keep the durable record in
    // ops.log always, but only surface on stderr when the operator opted into
    // verbose input diagnostics. (#opencode-stdout-bleed)
    if verbose_enabled() {
        eprintln!("[agent-doc] {message}");
    }
    if let Some(file) = file {
        crate::ops_log::log_op(file, &message);
    }
}

pub fn verbose_enabled() -> bool {
    std::env::var_os("AGENT_DOC_TMUX_INPUT_DIAG").is_some()
        || std::env::var_os("AGENT_DOC_DEBUG_STDIN").is_some()
}

pub fn format_key_event(
    source: &str,
    destination: &str,
    transform: &str,
    key: &str,
    bytes: usize,
    harness: Option<&str>,
    detail: Option<&str>,
) -> String {
    let mut message = format!(
        "{PREFIX} source={} destination={} transform={} key={} bytes={}",
        sanitize_field(source),
        sanitize_field(destination),
        sanitize_field(transform),
        sanitize_field(key),
        bytes
    );
    if let Some(harness) = harness {
        message.push_str(&format!(" harness={}", sanitize_field(harness)));
    }
    if let Some(detail) = detail {
        message.push_str(&format!(" detail={}", sanitize_field(detail)));
    }
    message
}

pub fn log_key_event(
    file: Option<&Path>,
    source: &str,
    destination: &str,
    transform: &str,
    key: &str,
    bytes: usize,
    meta: KeyEventMeta<'_>,
) {
    emit(
        file,
        format_key_event(
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
    file: Option<&Path>,
    source: &str,
    destination: &str,
    transform: &str,
    key: &str,
    bytes: usize,
    meta: KeyEventMeta<'_>,
) {
    if verbose_enabled() {
        log_key_event(file, source, destination, transform, key, bytes, meta);
    }
}

pub fn format_payload_event(
    source: &str,
    destination: &str,
    transform: &str,
    key: &str,
    bytes: &[u8],
    harness: Option<&str>,
) -> String {
    let detail = format!("sha256={}", bytes_hash(bytes));
    format_key_event(
        source,
        destination,
        transform,
        key,
        bytes.len(),
        harness,
        Some(&detail),
    )
}

pub fn log_text_submit(
    file: Option<&Path>,
    source: &str,
    destination: &str,
    text: &str,
    harness: Option<&str>,
    transform: &str,
    submit_key: &str,
) {
    emit(
        file,
        format_payload_event(
            source,
            destination,
            transform,
            "text",
            text.as_bytes(),
            harness,
        ),
    );
    log_key_event(
        file,
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

fn key_name(byte: u8) -> &'static str {
    match byte {
        b'\r' | b'\n' => "Enter",
        b'\t' => "Tab",
        0x1b => "Escape",
        0x03 => "Ctrl-C",
        0x04 => "Ctrl-D",
        0x7f => "Backspace",
        0x20..=0x7e => "Printable",
        _ => "Byte",
    }
}

pub fn log_byte_events(
    file: Option<&Path>,
    source: &str,
    destination: &str,
    transform: &str,
    bytes: &[u8],
    harness: Option<&str>,
) {
    for byte in bytes {
        let detail = format!("hex={byte:02x}");
        log_key_event(
            file,
            source,
            destination,
            transform,
            key_name(*byte),
            1,
            KeyEventMeta {
                harness,
                detail: Some(&detail),
            },
        );
    }
}

pub fn log_transform_event(
    file: Option<&Path>,
    source: &str,
    destination: &str,
    transform: &str,
    before: &[u8],
    after: &[u8],
    harness: Option<&str>,
) {
    let detail = format!(
        "before_len={} before_sha256={} after_len={} after_sha256={}",
        before.len(),
        bytes_hash(before),
        after.len(),
        bytes_hash(after)
    );
    log_key_event(
        file,
        source,
        destination,
        transform,
        "transform",
        after.len(),
        KeyEventMeta {
            harness,
            detail: Some(&detail),
        },
    );
}

pub fn log_prompt_detection(
    file: Option<&Path>,
    source: &str,
    destination: &str,
    harness: &str,
    reason: &str,
    state: &str,
) {
    log_key_event(
        file,
        source,
        destination,
        "permission_prompt_detection",
        "permission_prompt",
        0,
        KeyEventMeta {
            harness: Some(harness),
            detail: Some(&format!("state={state}_reason={}", sanitize_field(reason))),
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    struct EnvGuard {
        key: &'static str,
        prior: Option<String>,
        _lock: crate::test_support::ProcessGlobalLockGuard,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::set_var(key, value);
            }
            Self {
                key,
                prior,
                _lock: lock,
            }
        }

        fn remove(key: &'static str) -> Self {
            let lock = crate::test_support::env_lock();
            let prior = std::env::var(key).ok();
            unsafe {
                std::env::remove_var(key);
            }
            Self {
                key,
                prior,
                _lock: lock,
            }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            unsafe {
                match &self.prior {
                    Some(value) => std::env::set_var(self.key, value),
                    None => std::env::remove_var(self.key),
                }
            }
        }
    }

    #[test]
    fn key_event_format_is_structured_and_sanitized() {
        let event = format_key_event(
            "route direct",
            "pane:%42",
            "text+enter",
            "Enter",
            5,
            Some("open code"),
            Some("reason=active permission prompt"),
        );

        assert_eq!(
            event,
            "tmux_input_event source=route_direct destination=pane:%42 transform=text_enter key=Enter bytes=5 harness=open_code detail=reason=active_permission_prompt"
        );
    }

    #[test]
    fn payload_event_hashes_text_without_exposing_it() {
        let event = format_payload_event(
            "queue_dispatch",
            "pane:%7",
            "tmux_submit",
            "text",
            b"/clear",
            Some("codex"),
        );

        assert!(event.contains("tmux_input_event source=queue_dispatch"));
        assert!(event.contains("bytes=6"));
        assert!(event.contains("sha256="));
        assert!(!event.contains("/clear"));
    }

    #[test]
    fn verbose_input_diagnostics_are_opt_in() {
        let _diag_guard = EnvGuard::remove("AGENT_DOC_TMUX_INPUT_DIAG");
        let _stdin_guard = EnvGuard::remove("AGENT_DOC_DEBUG_STDIN");
        assert!(!verbose_enabled());

        let _diag_enabled = EnvGuard::set("AGENT_DOC_TMUX_INPUT_DIAG", "1");
        assert!(verbose_enabled());
    }
}
