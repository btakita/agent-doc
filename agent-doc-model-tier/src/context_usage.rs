//! Harness transcript token and context-window policy.
//!
//! This module owns the pure half of pre-emptive context clearing: parse token
//! usage from harness transcript content, map model names to context windows,
//! compute context percentage, and decide whether an opted-in caller should
//! request a destructive context clear. Filesystem transcript discovery and
//! reads stay in orchestration adapters.

use std::path::{Path, PathBuf};

/// Harness whose session transcript token usage can be interpreted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    OpenCode,
}

impl Harness {
    /// Parse a harness token. Unknown harnesses return `None` so callers can
    /// fail safe.
    pub fn parse(s: &str) -> Option<Harness> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(Harness::Claude),
            "codex" => Some(Harness::Codex),
            "opencode" => Some(Harness::OpenCode),
            _ => None,
        }
    }
}

/// Cumulative token usage read from a harness session transcript.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsedTokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl UsedTokens {
    /// Total tokens occupying the context window.
    pub fn total(self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_creation)
    }
}

/// Default context window in tokens for the current Claude model family.
pub const CLAUDE_CONTEXT_WINDOW: u64 = 200_000;

/// Claude Code encodes a project directory into its transcript project
/// subdirectory by replacing every `/` and `.` with `-`.
pub fn claude_project_hash(project_dir: &Path) -> String {
    project_dir.to_string_lossy().replace(['/', '.'], "-")
}

/// Locate a Claude Code session transcript by known session id.
pub fn claude_transcript_path(home: &Path, project_dir: &Path, session_id: &str) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(claude_project_hash(project_dir))
        .join(format!("{session_id}.jsonl"))
}

/// Compose the `~/.claude/projects/<project-hash>/` transcript directory.
pub fn claude_projects_subdir(home: &Path, project_dir: &Path) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(claude_project_hash(project_dir))
}

/// Parse cumulative token usage from Claude Code JSONL transcript content.
///
/// Each line is one JSON record. Assistant records nest the usage block under
/// `message.usage`; a few record shapes carry top-level `usage`. The latest
/// nonzero usage record wins. Non-JSON and partial trailing lines are ignored.
pub fn parse_claude_jsonl_used_tokens(content: &str) -> Option<UsedTokens> {
    let mut latest: Option<UsedTokens> = None;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let usage = value
            .get("message")
            .and_then(|m| m.get("usage"))
            .or_else(|| value.get("usage"));
        let Some(usage) = usage else { continue };
        let field = |k: &str| {
            usage
                .get(k)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(0)
        };
        let used = UsedTokens {
            input: field("input_tokens"),
            output: field("output_tokens"),
            cache_read: field("cache_read_input_tokens"),
            cache_creation: field("cache_creation_input_tokens"),
        };
        if used.total() > 0 {
            latest = Some(used);
        }
    }
    latest
}

/// Map a resolved model id to its context window in tokens.
pub fn context_window_for_model(model: &str) -> Option<u64> {
    let m = model.to_ascii_lowercase();
    if m.contains("opus")
        || m.contains("sonnet")
        || m.contains("haiku")
        || m.contains("fable")
        || m.starts_with("claude")
    {
        return Some(CLAUDE_CONTEXT_WINDOW);
    }
    None
}

/// Compute context-usage percentage for `used` tokens against `model`'s window,
/// clamped to `[0, 100]`.
pub fn context_pct(used: u64, model: &str) -> Option<f64> {
    let window = context_window_for_model(model)?;
    context_pct_for_window(used, window)
}

fn json_u64_at(value: &serde_json::Value, path: &[&str]) -> u64 {
    let mut cursor = value;
    for key in path {
        let Some(next) = cursor.get(*key) else {
            return 0;
        };
        cursor = next;
    }
    cursor.as_u64().unwrap_or(0)
}

fn context_pct_for_window(used: u64, window: u64) -> Option<f64> {
    if window == 0 {
        return None;
    }
    Some(((used as f64) / (window as f64) * 100.0).clamp(0.0, 100.0))
}

/// Parse Codex TUI session JSONL and compute context usage from the latest
/// `token_count` event.
pub fn parse_codex_jsonl_context_pct(content: &str) -> Option<f64> {
    let mut latest: Option<(u64, u64)> = None;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let value: serde_json::Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        if value
            .get("payload")
            .and_then(|p| p.get("type"))
            .and_then(serde_json::Value::as_str)
            != Some("token_count")
        {
            continue;
        }
        let window = json_u64_at(&value, &["payload", "info", "model_context_window"]);
        let input = json_u64_at(
            &value,
            &["payload", "info", "last_token_usage", "input_tokens"],
        );
        let cached = json_u64_at(
            &value,
            &["payload", "info", "last_token_usage", "cached_input_tokens"],
        );
        let output = json_u64_at(
            &value,
            &["payload", "info", "last_token_usage", "output_tokens"],
        );
        let used = input.saturating_add(cached).saturating_add(output);
        if window > 0 && used > 0 {
            latest = Some((used, window));
        }
    }
    let (used, window) = latest?;
    context_pct_for_window(used, window)
}

/// Parse the `payload.cwd` from a Codex TUI `session_meta` record.
///
/// Codex writes this near the start of each session transcript. Only the first
/// 20 JSONL records are scanned so callers can cheaply test candidate transcript
/// files during recursive filesystem discovery.
pub fn parse_codex_jsonl_session_meta_cwd(content: &str) -> Option<PathBuf> {
    for line in content.lines().take(20) {
        let value: serde_json::Value = match serde_json::from_str(line.trim()) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if value.get("type").and_then(serde_json::Value::as_str) != Some("session_meta") {
            continue;
        }
        let cwd = value
            .get("payload")
            .and_then(|p| p.get("cwd"))
            .and_then(serde_json::Value::as_str)?;
        return Some(PathBuf::from(cwd));
    }
    None
}

/// Outcome of the dispatch-time pre-emptive clear gate.
#[derive(Clone, Debug, PartialEq)]
pub struct ClearDecision {
    pub clear: bool,
    pub diagnostic: String,
}

/// Decide whether an opted-in caller should pre-emptively clear context before
/// dispatching a queue head.
pub fn clear_decision(opted_in: bool, pct: Option<f64>, threshold: u8) -> ClearDecision {
    let clear = opted_in && pct.is_some_and(|p| p >= f64::from(threshold));
    let pct_field = match pct {
        Some(p) => format!("{p:.1}"),
        None => "none".to_string(),
    };
    let diagnostic = format!(
        "[s760] clear-decision optIn={opted_in} threshold={threshold} pct={pct_field} clear={clear}"
    );
    ClearDecision { clear, diagnostic }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FIXTURE: &str = r#"{"type":"user","message":{"role":"user","content":"hi"}}
{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}}
{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":2,"output_tokens":4205,"cache_read_input_tokens":243320,"cache_creation_input_tokens":2232,"server_tool_use":{"web_search_requests":0}}}}
"#;

    #[test]
    fn parse_jsonl_returns_latest_usage() {
        let used = parse_claude_jsonl_used_tokens(FIXTURE).expect("latest usage");
        assert_eq!(used.input, 2);
        assert_eq!(used.output, 4205);
        assert_eq!(used.cache_read, 243320);
        assert_eq!(used.cache_creation, 2232);
        assert_eq!(used.total(), 2 + 4205 + 243320 + 2232);
    }

    #[test]
    fn parse_jsonl_tolerates_non_json_and_empty() {
        let content = "not json at all\n\n{\"message\":{\"usage\":{\"input_tokens\":7}}}\n{partial";
        let used = parse_claude_jsonl_used_tokens(content).expect("usage from the one valid line");
        assert_eq!(used.input, 7);
        assert_eq!(used.total(), 7);
    }

    #[test]
    fn parse_jsonl_none_when_no_usage() {
        assert!(parse_claude_jsonl_used_tokens("").is_none());
        assert!(
            parse_claude_jsonl_used_tokens("{\"type\":\"user\",\"message\":{\"content\":\"x\"}}")
                .is_none()
        );
        assert!(
            parse_claude_jsonl_used_tokens("{\"message\":{\"usage\":{\"input_tokens\":0}}}")
                .is_none()
        );
    }

    #[test]
    fn claude_project_hash_replaces_slash_and_dot() {
        assert_eq!(
            claude_project_hash(Path::new("/home/brian/work/btakita/agent-loop")),
            "-home-brian-work-btakita-agent-loop"
        );
        assert_eq!(
            claude_project_hash(Path::new("/home/u/.claude-mem")),
            "-home-u--claude-mem"
        );
    }

    #[test]
    fn claude_transcript_path_composes() {
        let p = claude_transcript_path(
            Path::new("/home/brian"),
            Path::new("/home/brian/work/btakita/agent-loop"),
            "74bb0c6d-4f39",
        );
        assert_eq!(
            p,
            Path::new(
                "/home/brian/.claude/projects/-home-brian-work-btakita-agent-loop/74bb0c6d-4f39.jsonl"
            )
        );
    }

    #[test]
    fn claude_projects_subdir_composes() {
        assert_eq!(
            claude_projects_subdir(
                Path::new("/home/brian"),
                Path::new("/home/brian/work/btakita/agent-loop"),
            ),
            Path::new("/home/brian/.claude/projects/-home-brian-work-btakita-agent-loop")
        );
    }

    #[test]
    fn context_window_known_families_200k() {
        for m in [
            "claude-opus-4-8",
            "claude-sonnet-4-6",
            "claude-haiku-4-5-20251001",
            "claude-fable-5",
            "opus",
            "Sonnet",
        ] {
            assert_eq!(
                context_window_for_model(m),
                Some(CLAUDE_CONTEXT_WINDOW),
                "{m}"
            );
        }
    }

    #[test]
    fn context_window_unknown_is_none() {
        assert!(context_window_for_model("gpt-5").is_none());
        assert!(context_window_for_model("llama-3").is_none());
    }

    #[test]
    fn context_pct_computes_and_clamps() {
        assert_eq!(context_pct(100_000, "claude-opus-4-8"), Some(50.0));
        assert_eq!(context_pct(500_000, "opus"), Some(100.0));
        assert_eq!(context_pct(0, "sonnet"), Some(0.0));
    }

    #[test]
    fn context_pct_unknown_model_is_none() {
        assert!(context_pct(100_000, "gpt-5").is_none());
    }

    #[test]
    fn parse_codex_jsonl_context_pct_uses_latest_token_count() {
        let content = r#"{"type":"session_meta","payload":{"cwd":"/tmp/project"}}
{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":10000,"cached_input_tokens":5000,"output_tokens":1000},"model_context_window":100000}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20000,"cached_input_tokens":10000,"output_tokens":5000},"model_context_window":100000}}}
"#;
        let pct = parse_codex_jsonl_context_pct(content).expect("codex pct");
        assert_eq!(pct, 35.0);
    }

    #[test]
    fn parse_codex_jsonl_context_pct_clamps_and_ignores_missing_window() {
        let content = r#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":1,"cached_input_tokens":1,"output_tokens":1}}}}
{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":90000,"cached_input_tokens":20000,"output_tokens":1000},"model_context_window":100000}}}
"#;
        let pct = parse_codex_jsonl_context_pct(content).expect("codex pct");
        assert_eq!(pct, 100.0);
    }

    #[test]
    fn parse_codex_jsonl_session_meta_cwd_scans_early_records_only() {
        let content = r#"not json
{"type":"event_msg","payload":{"type":"token_count"}}
{"type":"session_meta","payload":{"cwd":"/tmp/project"}}
"#;

        assert_eq!(
            parse_codex_jsonl_session_meta_cwd(content),
            Some(PathBuf::from("/tmp/project"))
        );

        let late = (0..20)
            .map(|idx| format!(r#"{{"type":"event_msg","payload":{{"idx":{idx}}}}}"#))
            .chain(std::iter::once(
                r#"{"type":"session_meta","payload":{"cwd":"/tmp/late"}}"#.to_string(),
            ))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(parse_codex_jsonl_session_meta_cwd(&late).is_none());
    }

    #[test]
    fn harness_parse_known_and_unknown() {
        assert_eq!(Harness::parse("claude"), Some(Harness::Claude));
        assert_eq!(Harness::parse("Claude-Code"), Some(Harness::Claude));
        assert_eq!(Harness::parse("codex"), Some(Harness::Codex));
        assert_eq!(Harness::parse("opencode"), Some(Harness::OpenCode));
        assert!(Harness::parse("junie").is_none());
    }

    #[test]
    fn clear_decision_clears_only_when_opted_in_and_at_or_above_threshold() {
        let d = clear_decision(true, Some(50.0), 50);
        assert!(d.clear);
        assert_eq!(
            d.diagnostic,
            "[s760] clear-decision optIn=true threshold=50 pct=50.0 clear=true"
        );
        assert!(clear_decision(true, Some(83.4), 50).clear);
        let below = clear_decision(true, Some(49.9), 50);
        assert!(!below.clear);
        assert_eq!(
            below.diagnostic,
            "[s760] clear-decision optIn=true threshold=50 pct=49.9 clear=false"
        );
    }

    #[test]
    fn clear_decision_fails_safe_on_unknown_pct_and_disabled_opt_in() {
        let unknown = clear_decision(true, None, 50);
        assert!(!unknown.clear);
        assert_eq!(
            unknown.diagnostic,
            "[s760] clear-decision optIn=true threshold=50 pct=none clear=false"
        );
        let off = clear_decision(false, Some(100.0), 50);
        assert!(!off.clear);
        assert_eq!(
            off.diagnostic,
            "[s760] clear-decision optIn=false threshold=50 pct=100.0 clear=false"
        );
    }
}
