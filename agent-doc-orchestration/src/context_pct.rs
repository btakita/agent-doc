//! # Module: context_pct (`#s760a` / `#s760b`)
//!
//! ## Spec (`#s760wire` — transcript-token context-% source)
//! The pure *source* half of the pre-emptive queue `/clear` decision: derive the
//! real context-usage % from the harness session transcript's cumulative token
//! usage — **not** exchange size, **not** a TUI-footer scrape (footers vary by
//! harness/config, so scraping is wrong for the shipped product). See
//! `tasks/agent-doc/plan-s760-transcript-ctx-clear.md`.
//!
//! This module owns two phases:
//! - **`#s760a`** — the harness-aware transcript locator + token reader. For
//!   Claude Code, locate `~/.claude/projects/<project-hash>/<session-id>.jsonl`
//!   and read the latest entry's cumulative `usage`. For Codex, locate the
//!   newest `~/.codex/sessions/**/rollout-*.jsonl` for the current project and
//!   read the latest `token_count` event's `last_token_usage` plus
//!   `model_context_window`. OpenCode transcript stores are not yet confirmed,
//!   so they return `None` (unsupported, skip) rather than guess.
//! - **`#s760b`** — the model → context-window table and the ctx% compute
//!   (`pct = used / window * 100`, clamped).
//!
//! The route-dispatch gate (`#s760c`, in `route.rs`) and operator live-verify
//! (`#s760d`) consume this source; they are intentionally **not** in this module.
//!
//! ## Safety
//! Sending `/clear` wipes the agent's context, so this source fails safe at every
//! boundary: an unknown model, a missing/empty/unreadable transcript, or an
//! unsupported harness all yield `None`, and the caller never clears on `None`
//! (per the `plan-s760` safety invariants). The destructive `/clear` also stays
//! behind the existing default-off `agent_doc_queue_context_reset` opt-in at the
//! `route.rs` gate.
//!
//! ## Evals
//! - `parse_jsonl_returns_latest_usage`
//! - `parse_jsonl_tolerates_non_json_and_empty`
//! - `parse_jsonl_none_when_no_usage`
//! - `claude_project_hash_replaces_slash_and_dot`
//! - `claude_transcript_path_composes`
//! - `context_window_known_families_200k`
//! - `context_window_unknown_is_none`
//! - `context_pct_computes_and_clamps`
//! - `context_pct_unknown_model_is_none`
//! - `read_used_tokens_unsupported_harness_is_none`
//! - `parse_codex_jsonl_context_pct_uses_latest_token_count`
//! - `transcript_context_pct_end_to_end`

use std::path::{Path, PathBuf};

/// Harness whose session transcript token usage we can read (`#s760a`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Harness {
    Claude,
    Codex,
    OpenCode,
}

impl Harness {
    /// Parse a harness token (the value of `agent:`/`--agent`). Returns `None`
    /// for anything we do not recognize so the caller can fail safe.
    pub fn parse(s: &str) -> Option<Harness> {
        match s.trim().to_ascii_lowercase().as_str() {
            "claude" | "claude-code" => Some(Harness::Claude),
            "codex" => Some(Harness::Codex),
            "opencode" => Some(Harness::OpenCode),
            _ => None,
        }
    }
}

/// Cumulative token usage read from a harness session transcript (`#s760a`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UsedTokens {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub cache_creation: u64,
}

impl UsedTokens {
    /// Total tokens occupying the context window: input + output + both cache
    /// classes (cache-read and cache-creation both consume window space).
    pub fn total(self) -> u64 {
        self.input
            .saturating_add(self.output)
            .saturating_add(self.cache_read)
            .saturating_add(self.cache_creation)
    }
}

/// Default context window (tokens) for the current Claude model family.
pub const CLAUDE_CONTEXT_WINDOW: u64 = 200_000;

/// Claude Code encodes a project directory into its `~/.claude/projects/`
/// subdirectory name by replacing every `/` and `.` with `-`
/// (e.g. `/home/u/.cfg/p` → `-home-u--cfg-p`). Pure string transform.
pub fn claude_project_hash(project_dir: &Path) -> String {
    project_dir.to_string_lossy().replace(['/', '.'], "-")
}

/// Locate a Claude Code session transcript:
/// `<home>/.claude/projects/<project-hash>/<session-id>.jsonl` (`#s760a`).
pub fn claude_transcript_path(home: &Path, project_dir: &Path, session_id: &str) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(claude_project_hash(project_dir))
        .join(format!("{session_id}.jsonl"))
}

/// Parse cumulative token usage from Claude Code JSONL transcript content
/// (`#s760a`). Each line is one JSON record; assistant records nest the usage
/// block under `message.usage` (a few record shapes carry a top-level `usage`).
/// Returns the LATEST record's usage, or `None` if none carries usage. Pure over
/// the content (no filesystem) so it is unit-testable against fixtures, and
/// tolerant of non-JSON / partial trailing lines (an active transcript may be
/// mid-write).
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

/// Read cumulative token usage from a transcript file for the given harness
/// (`#s760a`). Claude -> parse JSONL; OpenCode -> `None` (unsupported until its
/// transcript store is confirmed live, so the caller fails safe and never
/// clears). Codex usage needs the event-provided context window, so callers use
/// [`parse_codex_jsonl_context_pct`] through [`transcript_context_pct`] instead
/// of this raw token helper. A missing/unreadable file is `None`.
pub fn read_used_tokens(harness: Harness, transcript: &Path) -> Option<UsedTokens> {
    match harness {
        Harness::Claude => {
            let content = std::fs::read_to_string(transcript).ok()?;
            parse_claude_jsonl_used_tokens(&content)
        }
        Harness::Codex | Harness::OpenCode => {
            eprintln!(
                "[s760] raw transcript token reading unsupported for {harness:?}; ctx% None (never clears)"
            );
            None
        }
    }
}

/// Map a resolved model id to its context window in tokens (`#s760b`). Known
/// Claude families (Opus/Sonnet/Haiku/Fable, or any `claude-*`) are 200k. An
/// unknown model returns `None` and warns — the caller then treats ctx% as
/// unknown and never clears (fail safe, per the destructive-`/clear` invariant).
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
    eprintln!(
        "[s760] WARNING: unknown model {model:?}; context window unknown, ctx% None (never clears)"
    );
    None
}

/// Compute context-usage percent for `used` tokens against `model`'s window
/// (`#s760b`), clamped to `[0, 100]`. `None` when the model (hence window) is
/// unknown — the caller never clears on `None`.
pub fn context_pct(used: u64, model: &str) -> Option<f64> {
    let window = context_window_for_model(model)?;
    if window == 0 {
        return None;
    }
    let pct = (used as f64) / (window as f64) * 100.0;
    Some(pct.clamp(0.0, 100.0))
}

/// Read a transcript and compute ctx% in one call (`#s760a` + `#s760b`). `None`
/// (never clear) on any failure: unreadable/empty transcript, unsupported
/// harness, or unknown model.
pub fn transcript_context_pct(harness: Harness, transcript: &Path, model: &str) -> Option<f64> {
    match harness {
        Harness::Claude => {
            let used = read_used_tokens(harness, transcript)?;
            context_pct(used.total(), model)
        }
        Harness::Codex => {
            let content = std::fs::read_to_string(transcript).ok()?;
            parse_codex_jsonl_context_pct(&content)
        }
        Harness::OpenCode => {
            eprintln!(
                "[s760] transcript context reading unsupported for {harness:?}; ctx% None (never clears)"
            );
            None
        }
    }
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

/// Parse Codex TUI session JSONL and compute live context usage from the latest
/// `token_count` event. Codex reports the current turn's prompt/context usage in
/// `last_token_usage` and the exact `model_context_window` for that model. The
/// CLI displays cached input separately (`input=N (+ M cached)`), and cached
/// input still occupies the model context window, so it is included here.
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

/// Outcome of the `#s760c` dispatch-time pre-emptive `/clear` gate. `clear` is
/// whether the caller should fire the destructive `/clear`; `diagnostic` is the
/// canonical `[s760] clear-decision …` line the caller emits to ops.log so the
/// decision is observable in production without re-deriving it.
#[derive(Clone, Debug, PartialEq)]
pub struct ClearDecision {
    pub clear: bool,
    pub diagnostic: String,
}

/// Decide whether the route/supervisor dispatch gate should pre-emptively
/// `/clear` before sending a queue head (`#s760c`). Pure over the resolved
/// inputs so it is unit-testable; the caller resolves `opted_in`, the live
/// transcript ctx% (`pct`), and `threshold`, then emits [`ClearDecision::diagnostic`]
/// to ops.log and only sends the destructive `/clear` when [`ClearDecision::clear`]
/// is true.
///
/// Fails safe per the `plan-s760` invariants: an unknown ctx% (`pct = None`, from
/// an unknown model / missing transcript / unsupported harness) never clears, and
/// a disabled opt-in never clears regardless of `pct`. The diagnostic always
/// renders so a `clear=false` decision (and its reason) is still observable.
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

/// Compose the `~/.claude/projects/<project-hash>/` directory that holds a
/// project's Claude Code session transcripts (`#s760c`). The per-session
/// transcript file lives inside this directory.
pub fn claude_projects_subdir(home: &Path, project_dir: &Path) -> PathBuf {
    home.join(".claude")
        .join("projects")
        .join(claude_project_hash(project_dir))
}

/// Locate the active Claude Code session transcript as the most-recently-modified
/// `*.jsonl` under `projects_subdir` (`#s760c` live locator). The supervisor does
/// not track the managed harness's session id, so newest-mtime is the live signal
/// for "the transcript this session is writing". Returns `None` when the directory
/// is absent/unreadable or holds no `.jsonl` file, so the caller fails safe and
/// never clears.
pub fn latest_claude_transcript(projects_subdir: &Path) -> Option<PathBuf> {
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    for entry in std::fs::read_dir(projects_subdir).ok()?.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
            continue;
        }
        let Ok(modified) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if newest.as_ref().is_none_or(|(best, _)| modified > *best) {
            newest = Some((modified, path));
        }
    }
    newest.map(|(_, path)| path)
}

fn codex_session_meta_cwd(path: &Path) -> Option<PathBuf> {
    let file = std::fs::File::open(path).ok()?;
    let reader = std::io::BufReader::new(file);
    use std::io::BufRead;
    for line in reader.lines().map_while(Result::ok).take(20) {
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

fn path_matches_project_dir(path: &Path, project_dir: &Path) -> bool {
    if path == project_dir {
        return true;
    }
    match (path.canonicalize(), project_dir.canonicalize()) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    }
}

/// Locate the newest Codex TUI session transcript for the current project.
/// Codex stores sessions under `~/.codex/sessions/<year>/<month>/<day>/`; the
/// first `session_meta` record carries `payload.cwd`, which is the stable
/// project match key.
pub fn latest_codex_transcript(home: &Path, project_dir: &Path) -> Option<PathBuf> {
    let root = home.join(".codex").join("sessions");
    let mut stack = vec![root];
    let mut newest: Option<(std::time::SystemTime, PathBuf)> = None;
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(metadata) = entry.metadata() else {
                continue;
            };
            if metadata.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|e| e.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if !file_name.starts_with("rollout-") {
                continue;
            }
            let Ok(modified) = metadata.modified() else {
                continue;
            };
            if newest.as_ref().is_some_and(|(best, _)| modified <= *best) {
                continue;
            }
            let Some(cwd) = codex_session_meta_cwd(&path) else {
                continue;
            };
            if path_matches_project_dir(&cwd, project_dir) {
                newest = Some((modified, path));
            }
        }
    }
    newest.map(|(_, path)| path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    const FIXTURE: &str = r#"{"type":"user","message":{"role":"user","content":"hi"}}
{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":10,"output_tokens":5,"cache_read_input_tokens":100,"cache_creation_input_tokens":20}}}
{"type":"assistant","message":{"role":"assistant","usage":{"input_tokens":2,"output_tokens":4205,"cache_read_input_tokens":243320,"cache_creation_input_tokens":2232,"server_tool_use":{"web_search_requests":0}}}}
"#;

    #[test]
    fn parse_jsonl_returns_latest_usage() {
        let used = parse_claude_jsonl_used_tokens(FIXTURE).expect("latest usage");
        // The LAST assistant entry wins, not the first.
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
        // A zero-total usage block is treated as no usage (fail safe).
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
        // `/.` collapses to `--`, matching Claude Code's real encoding.
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
        // 100k / 200k = 50%.
        assert_eq!(context_pct(100_000, "claude-opus-4-8"), Some(50.0));
        // Over-window clamps to 100, never above.
        assert_eq!(context_pct(500_000, "opus"), Some(100.0));
        assert_eq!(context_pct(0, "sonnet"), Some(0.0));
    }

    #[test]
    fn context_pct_unknown_model_is_none() {
        assert!(context_pct(100_000, "gpt-5").is_none());
    }

    #[test]
    fn read_used_tokens_unsupported_harness_is_none() {
        let tmp = NamedTempFile::new().unwrap();
        assert!(read_used_tokens(Harness::Codex, tmp.path()).is_none());
        assert!(read_used_tokens(Harness::OpenCode, tmp.path()).is_none());
        // Missing file is also None (fail safe).
        assert!(
            read_used_tokens(Harness::Claude, Path::new("/no/such/transcript.jsonl")).is_none()
        );
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
    fn transcript_context_pct_end_to_end() {
        let mut tmp = NamedTempFile::new().unwrap();
        tmp.write_all(FIXTURE.as_bytes()).unwrap();
        // total = 2 + 4205 + 243320 + 2232 = 249759; /200000*100 clamps to 100.
        let pct = transcript_context_pct(Harness::Claude, tmp.path(), "claude-opus-4-8").unwrap();
        assert_eq!(pct, 100.0);
        // Below window: build a small fixture.
        let mut small = NamedTempFile::new().unwrap();
        small
            .write_all(
                b"{\"message\":{\"usage\":{\"input_tokens\":50000,\"output_tokens\":10000}}}\n",
            )
            .unwrap();
        let pct = transcript_context_pct(Harness::Claude, small.path(), "sonnet").unwrap();
        assert_eq!(pct, 30.0); // 60000 / 200000 * 100
        let mut codex = NamedTempFile::new().unwrap();
        codex
            .write_all(
                br#"{"type":"event_msg","payload":{"type":"token_count","info":{"last_token_usage":{"input_tokens":20000,"cached_input_tokens":10000,"output_tokens":0},"model_context_window":100000}}}
"#,
            )
            .unwrap();
        assert_eq!(
            transcript_context_pct(Harness::Codex, codex.path(), "gpt-5"),
            Some(30.0)
        );
        // Unsupported harness -> None.
        assert!(transcript_context_pct(Harness::OpenCode, tmp.path(), "opus").is_none());
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
        // Opted in, at threshold → clear, with the canonical diagnostic.
        let d = clear_decision(true, Some(50.0), 50);
        assert!(d.clear);
        assert_eq!(
            d.diagnostic,
            "[s760] clear-decision optIn=true threshold=50 pct=50.0 clear=true"
        );
        // Above threshold → clear.
        assert!(clear_decision(true, Some(83.4), 50).clear);
        // Below threshold → no clear.
        let below = clear_decision(true, Some(49.9), 50);
        assert!(!below.clear);
        assert_eq!(
            below.diagnostic,
            "[s760] clear-decision optIn=true threshold=50 pct=49.9 clear=false"
        );
    }

    #[test]
    fn clear_decision_fails_safe_on_unknown_pct_and_disabled_opt_in() {
        // Unknown ctx% never clears, even far past any threshold.
        let unknown = clear_decision(true, None, 50);
        assert!(!unknown.clear);
        assert_eq!(
            unknown.diagnostic,
            "[s760] clear-decision optIn=true threshold=50 pct=none clear=false"
        );
        // Disabled opt-in never clears, even at 100%.
        let off = clear_decision(false, Some(100.0), 50);
        assert!(!off.clear);
        assert_eq!(
            off.diagnostic,
            "[s760] clear-decision optIn=false threshold=50 pct=100.0 clear=false"
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
    fn latest_claude_transcript_picks_newest_jsonl() {
        let dir = tempfile::tempdir().unwrap();
        // No .jsonl yet → None (fail safe).
        assert!(latest_claude_transcript(dir.path()).is_none());

        let older = dir.path().join("old-session.jsonl");
        let newer = dir.path().join("new-session.jsonl");
        std::fs::write(&older, b"{}").unwrap();
        std::fs::write(&newer, b"{}").unwrap();
        // Force `newer` to have a strictly later mtime regardless of fs resolution.
        let later = std::time::SystemTime::now() + std::time::Duration::from_secs(60);
        filetime::set_file_mtime(&newer, filetime::FileTime::from_system_time(later)).unwrap();

        assert_eq!(latest_claude_transcript(dir.path()), Some(newer));

        // A non-.jsonl file is ignored even if it is newest.
        let txt = dir.path().join("zzz.txt");
        std::fs::write(&txt, b"x").unwrap();
        let even_later = std::time::SystemTime::now() + std::time::Duration::from_secs(120);
        filetime::set_file_mtime(&txt, filetime::FileTime::from_system_time(even_later)).unwrap();
        assert_eq!(
            latest_claude_transcript(dir.path()).unwrap().extension(),
            Some(std::ffi::OsStr::new("jsonl"))
        );
    }

    #[test]
    fn latest_claude_transcript_missing_dir_is_none() {
        assert!(latest_claude_transcript(Path::new("/no/such/projects/dir")).is_none());
    }

    #[test]
    fn latest_codex_transcript_picks_newest_matching_project() {
        let home = tempfile::tempdir().unwrap();
        let day = home
            .path()
            .join(".codex")
            .join("sessions")
            .join("2026")
            .join("06")
            .join("15");
        std::fs::create_dir_all(&day).unwrap();
        let project = home.path().join("project");
        std::fs::create_dir_all(&project).unwrap();
        let older = day.join("rollout-old.jsonl");
        let newer = day.join("rollout-new.jsonl");
        let other = day.join("rollout-other.jsonl");
        let non_rollout = day.join("notes.jsonl");

        std::fs::write(
            &older,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n",
                project.display()
            ),
        )
        .unwrap();
        std::fs::write(
            &newer,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n",
                project.display()
            ),
        )
        .unwrap();
        std::fs::write(
            &other,
            "{\"type\":\"session_meta\",\"payload\":{\"cwd\":\"/tmp/other\"}}\n",
        )
        .unwrap();
        std::fs::write(
            &non_rollout,
            format!(
                "{{\"type\":\"session_meta\",\"payload\":{{\"cwd\":\"{}\"}}}}\n",
                project.display()
            ),
        )
        .unwrap();

        let old_time = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
        let new_time = std::time::SystemTime::now() + std::time::Duration::from_secs(20);
        let other_time = std::time::SystemTime::now() + std::time::Duration::from_secs(30);
        let non_rollout_time = std::time::SystemTime::now() + std::time::Duration::from_secs(40);
        filetime::set_file_mtime(&older, filetime::FileTime::from_system_time(old_time)).unwrap();
        filetime::set_file_mtime(&newer, filetime::FileTime::from_system_time(new_time)).unwrap();
        filetime::set_file_mtime(&other, filetime::FileTime::from_system_time(other_time)).unwrap();
        filetime::set_file_mtime(
            &non_rollout,
            filetime::FileTime::from_system_time(non_rollout_time),
        )
        .unwrap();

        assert_eq!(latest_codex_transcript(home.path(), &project), Some(newer));
    }
}
