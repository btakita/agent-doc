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
//!   and read the latest entry's cumulative `usage`. Codex/OpenCode transcript
//!   stores are not yet confirmed, so they return `None` (unsupported, skip)
//!   rather than guess.
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
        let field = |k: &str| usage.get(k).and_then(serde_json::Value::as_u64).unwrap_or(0);
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
/// (`#s760a`). Claude → parse JSONL; Codex/OpenCode → `None` (unsupported until
/// their transcript stores are confirmed live, so the caller fails safe and
/// never clears). A missing/unreadable file is `None`.
pub fn read_used_tokens(harness: Harness, transcript: &Path) -> Option<UsedTokens> {
    match harness {
        Harness::Claude => {
            let content = std::fs::read_to_string(transcript).ok()?;
            parse_claude_jsonl_used_tokens(&content)
        }
        Harness::Codex | Harness::OpenCode => {
            eprintln!(
                "[s760] transcript token reading unsupported for {harness:?}; ctx% None (never clears)"
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
    eprintln!("[s760] WARNING: unknown model {model:?}; context window unknown, ctx% None (never clears)");
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
    let used = read_used_tokens(harness, transcript)?;
    context_pct(used.total(), model)
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
            assert_eq!(context_window_for_model(m), Some(CLAUDE_CONTEXT_WINDOW), "{m}");
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
        assert!(read_used_tokens(Harness::Claude, Path::new("/no/such/transcript.jsonl")).is_none());
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
            .write_all(b"{\"message\":{\"usage\":{\"input_tokens\":50000,\"output_tokens\":10000}}}\n")
            .unwrap();
        let pct = transcript_context_pct(Harness::Claude, small.path(), "sonnet").unwrap();
        assert_eq!(pct, 30.0); // 60000 / 200000 * 100
        // Unsupported harness → None.
        assert!(transcript_context_pct(Harness::Codex, tmp.path(), "opus").is_none());
    }

    #[test]
    fn harness_parse_known_and_unknown() {
        assert_eq!(Harness::parse("claude"), Some(Harness::Claude));
        assert_eq!(Harness::parse("Claude-Code"), Some(Harness::Claude));
        assert_eq!(Harness::parse("codex"), Some(Harness::Codex));
        assert_eq!(Harness::parse("opencode"), Some(Harness::OpenCode));
        assert!(Harness::parse("junie").is_none());
    }
}
