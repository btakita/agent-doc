//! Pure tmux command builders and output parsers.
//!
//! This crate builds argv vectors and parses command output. It does not spawn
//! processes or decide turn lifecycle actions.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TmuxCommand {
    args: Vec<String>,
}

impl TmuxCommand {
    pub fn new(args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            args: args.into_iter().map(Into::into).collect(),
        }
    }

    pub fn args(&self) -> &[String] {
        &self.args
    }

    pub fn into_args(self) -> Vec<String> {
        self.args
    }
}

pub fn display_message(target: Option<&str>, format: &str) -> TmuxCommand {
    let mut args = vec!["display-message".to_string(), "-p".to_string()];
    push_optional_target(&mut args, target);
    args.push(format.to_string());
    TmuxCommand::new(args)
}

pub fn display_notification(target: &str, delay_ms: &str, message: &str) -> TmuxCommand {
    TmuxCommand::new(["display-message", "-t", target, "-d", delay_ms, message])
}

pub fn list_panes(target: Option<&str>, format: &str) -> TmuxCommand {
    let mut args = vec!["list-panes".to_string()];
    push_optional_target(&mut args, target);
    args.extend(["-F".to_string(), format.to_string()]);
    TmuxCommand::new(args)
}

pub fn list_panes_all(format: &str) -> TmuxCommand {
    TmuxCommand::new(["list-panes", "-a", "-F", format])
}

pub fn list_windows(target: Option<&str>, format: &str) -> TmuxCommand {
    let mut args = vec!["list-windows".to_string()];
    push_optional_target(&mut args, target);
    args.extend(["-F".to_string(), format.to_string()]);
    TmuxCommand::new(args)
}

pub fn list_windows_all(format: &str) -> TmuxCommand {
    TmuxCommand::new(["list-windows", "-a", "-F", format])
}

pub fn new_window_in_cwd(cwd: &str, name: &str, command: &str) -> TmuxCommand {
    TmuxCommand::new(["new-window", "-c", cwd, "-n", name, command])
}

pub fn kill_pane(target: &str) -> TmuxCommand {
    TmuxCommand::new(["kill-pane", "-t", target])
}

pub fn kill_window(target: &str) -> TmuxCommand {
    TmuxCommand::new(["kill-window", "-t", target])
}

pub fn respawn_pane(target: &str, command: &str) -> TmuxCommand {
    TmuxCommand::new(["respawn-pane", "-k", "-t", target, command])
}

pub fn rename_window(target: &str, name: &str) -> TmuxCommand {
    TmuxCommand::new(["rename-window", "-t", target, name])
}

pub fn resize_window_height(target: &str, height: &str) -> TmuxCommand {
    TmuxCommand::new(["resize-window", "-t", target, "-y", height])
}

pub fn swap_window(source: &str, target: &str) -> TmuxCommand {
    TmuxCommand::new(["swap-window", "-s", source, "-t", target])
}

pub fn capture_pane(target: &str) -> TmuxCommand {
    TmuxCommand::new(["capture-pane", "-p", "-t", target])
}

pub fn capture_pane_with_ansi(target: &str) -> TmuxCommand {
    TmuxCommand::new(["capture-pane", "-t", target, "-p", "-e"])
}

pub fn send_keys_literal(target: &str, text: &str) -> TmuxCommand {
    TmuxCommand::new(["send-keys", "-t", target, "-l", text])
}

pub fn send_key(target: &str, key: &str) -> TmuxCommand {
    TmuxCommand::new(["send-keys", "-t", target, key])
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TmuxSubmitProfile {
    /// When non-zero, send the text and the submit key as separate
    /// `tmux send-keys` calls with this delay between them.
    ///
    /// OpenCode's slash-command palette opens the moment `/` is typed. If Enter
    /// is sent in the same `tmux send-keys` call as the text, the palette can
    /// swallow the Enter instead of submitting the composer. Splitting the send
    /// gives the TUI time to settle. Other harnesses keep the canonical
    /// single-call form.
    split_text_and_submit_delay_ms: u64,
}

impl TmuxSubmitProfile {
    pub const fn new() -> Self {
        Self {
            split_text_and_submit_delay_ms: 0,
        }
    }

    pub const fn with_split_text_submit_delay(delay_ms: u64) -> Self {
        Self {
            split_text_and_submit_delay_ms: delay_ms,
        }
    }

    pub const fn mode(self) -> &'static str {
        "tmux_text_enter"
    }

    pub const fn transform(self) -> &'static str {
        "tmux_text_enter"
    }

    pub const fn submit_key(self) -> &'static str {
        "Enter"
    }

    pub const fn pending_draft_enter_resubmit(self) -> bool {
        true
    }

    pub const fn split_text_and_submit_delay_ms(self) -> u64 {
        self.split_text_and_submit_delay_ms
    }
}

impl Default for TmuxSubmitProfile {
    fn default() -> Self {
        Self::new()
    }
}

/// OpenCode needs the split text+Enter send because its slash-command palette
/// opens on `/` and swallows a same-call Enter. This is `const fn`-safe byte
/// compare because `str` equality is not const.
const fn harness_is_opencode(harness: &str) -> bool {
    let b = harness.as_bytes();
    b.len() == 8
        && b[0] == b'o'
        && b[1] == b'p'
        && b[2] == b'e'
        && b[3] == b'n'
        && b[4] == b'c'
        && b[5] == b'o'
        && b[6] == b'd'
        && b[7] == b'e'
}

pub const fn tmux_submit_profile_for_harness(harness: &str) -> TmuxSubmitProfile {
    if harness_is_opencode(harness) {
        TmuxSubmitProfile::with_split_text_submit_delay(80)
    } else {
        TmuxSubmitProfile::new()
    }
}

pub const fn tmux_submit_mode_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).mode()
}

pub const fn tmux_submit_transform_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).transform()
}

pub const fn tmux_submit_key_for_harness(harness: &str) -> &'static str {
    tmux_submit_profile_for_harness(harness).submit_key()
}

pub fn submitted_text_without_trailing_line_endings(text: &str) -> &str {
    text.trim_end_matches(['\r', '\n'])
}

pub fn text_submit_command(target: &str, text: &str, profile: TmuxSubmitProfile) -> TmuxCommand {
    let text = submitted_text_without_trailing_line_endings(text);
    let mut args = vec![
        "send-keys".to_string(),
        "-t".to_string(),
        target.to_string(),
    ];
    if !text.is_empty() {
        args.push(text.to_string());
    }
    args.push(profile.submit_key().to_string());
    TmuxCommand::new(args)
}

/// Arg list for the text-only half of a split send, same shape as
/// [`text_submit_command`] minus the trailing submit key.
pub fn text_only_command(target: &str, text: &str) -> TmuxCommand {
    let text = submitted_text_without_trailing_line_endings(text);
    let mut args = vec![
        "send-keys".to_string(),
        "-t".to_string(),
        target.to_string(),
    ];
    if !text.is_empty() {
        args.push(text.to_string());
    }
    TmuxCommand::new(args)
}

pub fn parse_nonempty_lines(output: &str) -> Vec<String> {
    output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn push_optional_target(args: &mut Vec<String>, target: Option<&str>) {
    if let Some(target) = target {
        args.extend(["-t".to_string(), target.to_string()]);
    }
}

pub mod input_diag {
    //! Pure structured formatting policy for tmux and supervisor input delivery.
    //!
    //! These helpers intentionally avoid logging raw prompt text. Input payloads
    //! are represented by byte length and SHA-256 so tests and operators can
    //! prove which delivery path ran without leaking typed content into logs.

    use sha2::{Digest, Sha256};

    pub const PREFIX: &str = "tmux_input_event";
    pub const EDITOR_ROUTE_ATTEMPT_ID_ENV: &str = "AGENT_DOC_EDITOR_ROUTE_ATTEMPT_ID";

    #[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
    pub struct KeyEventMeta<'a> {
        pub harness: Option<&'a str>,
        pub detail: Option<&'a str>,
    }

    pub fn sanitize_field(value: &str) -> String {
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

    pub fn sanitize_route_snapshot_field(value: &str) -> String {
        let mut out = String::with_capacity(value.len());
        for ch in value.chars() {
            if ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.') {
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

    pub fn format_route_pane_snapshot_filename(
        timestamp_millis: u128,
        phase: &str,
        harness_binary: &str,
        pane: &str,
        capture_hash: &str,
    ) -> String {
        format!(
            "{}-{}-{}-{}-{}.txt",
            timestamp_millis,
            sanitize_route_snapshot_field(phase),
            sanitize_route_snapshot_field(harness_binary),
            sanitize_route_snapshot_field(pane),
            capture_hash
        )
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct RoutePaneSnapshotFacts<'a> {
        pub file_display: &'a str,
        pub pane: &'a str,
        pub harness_binary: &'a str,
        pub phase: &'a str,
        pub capture_len: usize,
        pub capture_hash: &'a str,
        pub editor_attempt_id: Option<&'a str>,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct RoutePaneSnapshotLogFacts<'a> {
        pub snapshot: RoutePaneSnapshotFacts<'a>,
        pub snapshot_path: &'a str,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct RoutePaneSnapshotFailedLogFacts<'a> {
        pub snapshot: RoutePaneSnapshotFacts<'a>,
        pub error: &'a str,
    }

    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub struct RoutePaneSnapshotHintFacts<'a> {
        pub snapshot: RoutePaneSnapshotFacts<'a>,
        pub snapshot_path: Option<&'a str>,
    }

    fn append_route_pane_snapshot_attempt(message: &mut String, facts: RoutePaneSnapshotFacts<'_>) {
        if let Some(editor_attempt_id) = facts.editor_attempt_id {
            message.push_str(&format!(" editor_attempt_id={editor_attempt_id}"));
        }
    }

    pub fn format_route_pane_snapshot_log(facts: RoutePaneSnapshotLogFacts<'_>) -> String {
        let snapshot = facts.snapshot;
        let mut message = format!(
            "route_pane_snapshot file={} pane={} harness={} phase={} capture_len={} capture_hash={} snapshot_path={}",
            snapshot.file_display,
            snapshot.pane,
            snapshot.harness_binary,
            snapshot.phase,
            snapshot.capture_len,
            snapshot.capture_hash,
            facts.snapshot_path
        );
        append_route_pane_snapshot_attempt(&mut message, snapshot);
        message
    }

    pub fn format_route_pane_snapshot_failed_log(
        facts: RoutePaneSnapshotFailedLogFacts<'_>,
    ) -> String {
        let snapshot = facts.snapshot;
        let mut message = format!(
            "route_pane_snapshot_failed file={} pane={} harness={} phase={} capture_len={} capture_hash={} error={}",
            snapshot.file_display,
            snapshot.pane,
            snapshot.harness_binary,
            snapshot.phase,
            snapshot.capture_len,
            snapshot.capture_hash,
            facts.error.replace(char::is_whitespace, "_")
        );
        append_route_pane_snapshot_attempt(&mut message, snapshot);
        message
    }

    pub fn format_route_pane_snapshot_hint(facts: RoutePaneSnapshotHintFacts<'_>) -> String {
        let snapshot = facts.snapshot;
        let mut message = format!(
            "[route] preserved dispatch-start proof snapshot for {} pane {} harness={} phase={} capture_len={} capture_hash={}",
            snapshot.file_display,
            snapshot.pane,
            snapshot.harness_binary,
            snapshot.phase,
            snapshot.capture_len,
            snapshot.capture_hash
        );
        if let Some(snapshot_path) = facts.snapshot_path {
            message.push_str(&format!(" snapshot_path={snapshot_path}"));
        }
        append_route_pane_snapshot_attempt(&mut message, snapshot);
        message
    }

    fn editor_route_attempt_id() -> Option<String> {
        std::env::var(EDITOR_ROUTE_ATTEMPT_ID_ENV)
            .ok()
            .map(|value| sanitize_field(&value))
            .filter(|value| !value.is_empty())
    }

    pub fn bytes_hash(bytes: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(bytes);
        hex::encode(hasher.finalize())
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
        if let Some(attempt_id) = editor_route_attempt_id() {
            message.push_str(&format!(" editor_attempt_id={attempt_id}"));
        }
        message
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

    pub fn key_name(byte: u8) -> &'static str {
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

    pub fn format_byte_event(
        source: &str,
        destination: &str,
        transform: &str,
        byte: u8,
        harness: Option<&str>,
    ) -> String {
        let detail = format!("hex={byte:02x}");
        format_key_event(
            source,
            destination,
            transform,
            key_name(byte),
            1,
            harness,
            Some(&detail),
        )
    }

    pub fn format_transform_event(
        source: &str,
        destination: &str,
        transform: &str,
        before: &[u8],
        after: &[u8],
        harness: Option<&str>,
    ) -> String {
        let detail = format!(
            "before_len={} before_sha256={} after_len={} after_sha256={}",
            before.len(),
            bytes_hash(before),
            after.len(),
            bytes_hash(after)
        );
        format_key_event(
            source,
            destination,
            transform,
            "transform",
            after.len(),
            harness,
            Some(&detail),
        )
    }

    pub fn format_prompt_detection(
        source: &str,
        destination: &str,
        harness: &str,
        reason: &str,
        state: &str,
    ) -> String {
        let detail = format!("state={state}_reason={}", sanitize_field(reason));
        format_key_event(
            source,
            destination,
            "permission_prompt_detection",
            "permission_prompt",
            0,
            Some(harness),
            Some(&detail),
        )
    }

    #[cfg(test)]
    mod tests {
        use super::*;
        use std::sync::Mutex;

        static ENV_LOCK: Mutex<()> = Mutex::new(());

        struct EnvSnapshot {
            entries: Vec<(&'static str, Option<String>)>,
        }

        impl EnvSnapshot {
            fn capture(keys: &[&'static str]) -> Self {
                Self {
                    entries: keys
                        .iter()
                        .copied()
                        .map(|key| (key, std::env::var(key).ok()))
                        .collect(),
                }
            }
        }

        impl Drop for EnvSnapshot {
            fn drop(&mut self) {
                for (key, prior) in self.entries.iter().rev() {
                    unsafe {
                        match prior {
                            Some(value) => std::env::set_var(key, value),
                            None => std::env::remove_var(key),
                        }
                    }
                }
            }
        }

        fn set_env(key: &'static str, value: &str) {
            unsafe {
                std::env::set_var(key, value);
            }
        }

        fn remove_env(key: &'static str) {
            unsafe {
                std::env::remove_var(key);
            }
        }

        fn route_snapshot_facts() -> RoutePaneSnapshotFacts<'static> {
            RoutePaneSnapshotFacts {
                file_display: "tasks/route.md",
                pane: "%7",
                harness_binary: "codex",
                phase: "direct_pane_acceptance",
                capture_len: 42,
                capture_hash: "abc123",
                editor_attempt_id: Some("attempt_1_2"),
            }
        }

        #[test]
        fn key_event_format_is_structured_and_sanitized() {
            let _lock = ENV_LOCK.lock().unwrap();
            let _env = EnvSnapshot::capture(&[EDITOR_ROUTE_ATTEMPT_ID_ENV]);
            remove_env(EDITOR_ROUTE_ATTEMPT_ID_ENV);

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
            let _lock = ENV_LOCK.lock().unwrap();
            let _env = EnvSnapshot::capture(&[EDITOR_ROUTE_ATTEMPT_ID_ENV]);
            remove_env(EDITOR_ROUTE_ATTEMPT_ID_ENV);

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
            assert!(event.contains(
                "sha256=ddf7839cb8fca09abdd9e9b0b2f498885f382f5bf9fec65d95db793bd0f11832"
            ));
            assert!(!event.contains("/clear"));
            assert_eq!(
                bytes_hash(b"/clear"),
                "ddf7839cb8fca09abdd9e9b0b2f498885f382f5bf9fec65d95db793bd0f11832"
            );
        }

        #[test]
        fn input_events_include_editor_route_attempt_when_present() {
            let _lock = ENV_LOCK.lock().unwrap();
            let _env = EnvSnapshot::capture(&[EDITOR_ROUTE_ATTEMPT_ID_ENV]);
            set_env(EDITOR_ROUTE_ATTEMPT_ID_ENV, "attempt 1/2");

            let event = format_key_event(
                "route.direct_pane_submit",
                "pane:%42",
                "tmux_text_enter",
                "Enter",
                5,
                Some("codex"),
                None,
            );

            assert!(event.contains("editor_attempt_id=attempt_1/2"), "{event}");
        }

        #[test]
        fn route_snapshot_filename_fields_use_strict_filename_sanitization() {
            assert_eq!(
                sanitize_route_snapshot_field("phase/name:%7=ok"),
                "phase_name__7_ok"
            );
            assert_eq!(sanitize_route_snapshot_field(""), "none");
            assert_eq!(
                sanitize_field("phase/name:%7=ok"),
                "phase/name:%7=ok",
                "generic input diagnostics keep log-safe route characters"
            );

            assert_eq!(
                format_route_pane_snapshot_filename(
                    123,
                    "direct/pane acceptance",
                    "codex:dev",
                    "%7",
                    "abc123"
                ),
                "123-direct_pane_acceptance-codex_dev-_7-abc123.txt"
            );
        }

        #[test]
        fn route_snapshot_logs_are_structured_and_keep_attempt_id() {
            let log = format_route_pane_snapshot_log(RoutePaneSnapshotLogFacts {
                snapshot: route_snapshot_facts(),
                snapshot_path: "/tmp/.agent-doc/logs/route-submit/one.txt",
            });
            assert_eq!(
                log,
                "route_pane_snapshot file=tasks/route.md pane=%7 harness=codex phase=direct_pane_acceptance capture_len=42 capture_hash=abc123 snapshot_path=/tmp/.agent-doc/logs/route-submit/one.txt editor_attempt_id=attempt_1_2"
            );

            let failed = format_route_pane_snapshot_failed_log(RoutePaneSnapshotFailedLogFacts {
                snapshot: route_snapshot_facts(),
                error: "write failed\npermission denied",
            });
            assert_eq!(
                failed,
                "route_pane_snapshot_failed file=tasks/route.md pane=%7 harness=codex phase=direct_pane_acceptance capture_len=42 capture_hash=abc123 error=write_failed_permission_denied editor_attempt_id=attempt_1_2"
            );
        }

        #[test]
        fn route_snapshot_hint_formats_optional_snapshot_path() {
            let hint = format_route_pane_snapshot_hint(RoutePaneSnapshotHintFacts {
                snapshot: route_snapshot_facts(),
                snapshot_path: Some("/tmp/.agent-doc/logs/route-submit/one.txt"),
            });
            assert_eq!(
                hint,
                "[route] preserved dispatch-start proof snapshot for tasks/route.md pane %7 harness=codex phase=direct_pane_acceptance capture_len=42 capture_hash=abc123 snapshot_path=/tmp/.agent-doc/logs/route-submit/one.txt editor_attempt_id=attempt_1_2"
            );

            let without_path = format_route_pane_snapshot_hint(RoutePaneSnapshotHintFacts {
                snapshot: RoutePaneSnapshotFacts {
                    editor_attempt_id: None,
                    ..route_snapshot_facts()
                },
                snapshot_path: None,
            });
            assert_eq!(
                without_path,
                "[route] preserved dispatch-start proof snapshot for tasks/route.md pane %7 harness=codex phase=direct_pane_acceptance capture_len=42 capture_hash=abc123"
            );
        }

        #[test]
        fn byte_transform_and_prompt_events_share_structured_policy() {
            let _lock = ENV_LOCK.lock().unwrap();
            let _env = EnvSnapshot::capture(&[EDITOR_ROUTE_ATTEMPT_ID_ENV]);
            remove_env(EDITOR_ROUTE_ATTEMPT_ID_ENV);

            let byte = format_byte_event("supervisor.stdin", "child pty", "raw", 0x03, None);
            assert_eq!(
                byte,
                "tmux_input_event source=supervisor.stdin destination=child_pty transform=raw key=Ctrl-C bytes=1 detail=hex=03"
            );

            let transform = format_transform_event(
                "supervisor.stdin",
                "child_pty",
                "arrow_translation",
                b"\x1b[A",
                b"j",
                Some("opencode"),
            );
            assert!(transform.contains("key=transform bytes=1 harness=opencode"));
            assert!(transform.contains("before_len=3"));
            assert!(transform.contains("after_len=1"));
            assert!(transform.contains("before_sha256="));
            assert!(transform.contains("after_sha256="));

            let prompt = format_prompt_detection(
                "supervisor.stdout",
                "route",
                "codex",
                "active permission prompt",
                "visible",
            );
            assert_eq!(
                prompt,
                "tmux_input_event source=supervisor.stdout destination=route transform=permission_prompt_detection key=permission_prompt bytes=0 harness=codex detail=state=visible_reason=active_permission_prompt"
            );
        }

        #[test]
        fn verbose_input_diagnostics_are_opt_in() {
            let _lock = ENV_LOCK.lock().unwrap();
            let _env =
                EnvSnapshot::capture(&["AGENT_DOC_TMUX_INPUT_DIAG", "AGENT_DOC_DEBUG_STDIN"]);
            remove_env("AGENT_DOC_TMUX_INPUT_DIAG");
            remove_env("AGENT_DOC_DEBUG_STDIN");
            assert!(!verbose_enabled());

            set_env("AGENT_DOC_TMUX_INPUT_DIAG", "1");
            assert!(verbose_enabled());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn literal_send_keys_keeps_user_text_as_one_arg() {
        let command = send_keys_literal("%1", "--hello world");

        assert_eq!(
            command.args(),
            ["send-keys", "-t", "%1", "-l", "--hello world"]
        );
    }

    #[test]
    fn list_panes_places_target_before_format() {
        let command = list_panes(Some(":agent"), "#{pane_id}");

        assert_eq!(
            command.args(),
            ["list-panes", "-t", ":agent", "-F", "#{pane_id}"]
        );
    }

    #[test]
    fn list_panes_all_uses_all_flag_before_format() {
        let command = list_panes_all("#{pane_id}");

        assert_eq!(command.args(), ["list-panes", "-a", "-F", "#{pane_id}"]);
    }

    #[test]
    fn list_windows_places_target_before_format() {
        let command = list_windows(Some("dev:"), "#{window_id}");

        assert_eq!(
            command.args(),
            ["list-windows", "-t", "dev:", "-F", "#{window_id}"]
        );
    }

    #[test]
    fn list_windows_all_uses_all_flag_before_format() {
        let command = list_windows_all("#{window_id}");

        assert_eq!(command.args(), ["list-windows", "-a", "-F", "#{window_id}"]);
    }

    #[test]
    fn new_window_in_cwd_sets_cwd_name_and_command() {
        let command = new_window_in_cwd("/repo", "agent-doc", "agent-doc start plan.md");

        assert_eq!(
            command.args(),
            [
                "new-window",
                "-c",
                "/repo",
                "-n",
                "agent-doc",
                "agent-doc start plan.md"
            ]
        );
    }

    #[test]
    fn kill_pane_targets_requested_pane() {
        let command = kill_pane("%7");

        assert_eq!(command.args(), ["kill-pane", "-t", "%7"]);
    }

    #[test]
    fn kill_window_targets_requested_window() {
        let command = kill_window("@9");

        assert_eq!(command.args(), ["kill-window", "-t", "@9"]);
    }

    #[test]
    fn respawn_pane_kills_existing_process_and_runs_command() {
        let command = respawn_pane("%4", "exec agent-doc start file.md");

        assert_eq!(
            command.args(),
            [
                "respawn-pane",
                "-k",
                "-t",
                "%4",
                "exec agent-doc start file.md"
            ]
        );
    }

    #[test]
    fn rename_window_targets_requested_window() {
        let command = rename_window("@2", "agent-doc");

        assert_eq!(command.args(), ["rename-window", "-t", "@2", "agent-doc"]);
    }

    #[test]
    fn resize_window_height_targets_requested_height() {
        let command = resize_window_height("@2", "1000");

        assert_eq!(command.args(), ["resize-window", "-t", "@2", "-y", "1000"]);
    }

    #[test]
    fn swap_window_targets_requested_source_and_destination() {
        let command = swap_window("@2", "@9");

        assert_eq!(command.args(), ["swap-window", "-s", "@2", "-t", "@9"]);
    }

    #[test]
    fn display_notification_builds_targeted_delay_message_command() {
        assert_eq!(
            display_notification("%1", "3000", "claimed").into_args(),
            vec!["display-message", "-t", "%1", "-d", "3000", "claimed"]
        );
    }

    #[test]
    fn capture_pane_with_ansi_preserves_escape_attributes() {
        assert_eq!(
            capture_pane_with_ansi("%1").into_args(),
            vec!["capture-pane", "-t", "%1", "-p", "-e"]
        );
    }

    #[test]
    fn parser_discards_blank_lines() {
        assert_eq!(
            parse_nonempty_lines("\n%1\n  \n%2  \n"),
            vec!["%1".to_string(), "%2".to_string()]
        );
    }

    #[test]
    fn submit_profiles_keep_harness_submit_policy_in_one_place() {
        for harness in ["codex", "claude", "opencode", "unknown-harness"] {
            assert_eq!(tmux_submit_mode_for_harness(harness), "tmux_text_enter");
            assert_eq!(
                tmux_submit_transform_for_harness(harness),
                "tmux_text_enter"
            );
            assert_eq!(tmux_submit_key_for_harness(harness), "Enter");
            assert_eq!(
                submitted_text_without_trailing_line_endings("agent-doc plan.md\r\n"),
                "agent-doc plan.md"
            );
            assert_eq!(
                text_submit_command(
                    "%7",
                    "agent-doc plan.md\r\n",
                    tmux_submit_profile_for_harness(harness)
                )
                .into_args(),
                ["send-keys", "-t", "%7", "agent-doc plan.md", "Enter"]
                    .into_iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "{harness} must submit tmux text with one named Enter key"
            );
            assert_eq!(
                text_submit_command("%7", "\n", tmux_submit_profile_for_harness(harness))
                    .into_args(),
                ["send-keys", "-t", "%7", "Enter"]
                    .into_iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>(),
                "{harness} empty resubmit must send only the named Enter key"
            );
        }
    }

    #[test]
    fn tmux_submit_profile_splits_text_and_enter_only_for_opencode() {
        let opencode = tmux_submit_profile_for_harness("opencode");
        assert!(
            opencode.split_text_and_submit_delay_ms() > 0,
            "opencode must request a split text+Enter send so the slash-command palette can settle before the Enter arrives"
        );
        assert_eq!(opencode.submit_key(), "Enter");
        assert_eq!(opencode.mode(), "tmux_text_enter");
        assert_eq!(opencode.transform(), "tmux_text_enter");
        assert!(opencode.pending_draft_enter_resubmit());

        for non_opencode in ["codex", "claude", "claude-code", "default", "", "unknown"] {
            let profile = tmux_submit_profile_for_harness(non_opencode);
            assert_eq!(
                profile.split_text_and_submit_delay_ms(),
                0,
                "{non_opencode:?} must keep the single-call text+Enter send (no split)"
            );
        }
    }

    #[test]
    fn text_only_command_omits_submit_key_for_split_send() {
        assert_eq!(
            text_only_command("%7", "/new").into_args(),
            ["send-keys", "-t", "%7", "/new"]
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            "split-send text step must not include the trailing Enter"
        );
        assert_eq!(
            text_only_command("%7", "/new\r\n").into_args(),
            ["send-keys", "-t", "%7", "/new"]
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
            "split-send text step must strip trailing line endings before sending"
        );
        assert_eq!(
            text_only_command("%7", "\n").into_args(),
            ["send-keys", "-t", "%7"]
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        );
    }
}
