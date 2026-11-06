//! Harness usage-limit detection (`#harnessquotaexhausted`).
//!
//! A coding harness that runs out of plan usage or credits stops working but
//! keeps its TUI up: the pane sits at an idle prompt, its turn lease can still
//! look live, and retrying cannot help until the limit resets. Observed
//! 2026-11-05 on a live Grok 4.7 pane whose composer border read
//! `Weekly limit left: 0%`: route reported `owner_turn=ambiguous` and advised
//! a retry, and session-check reported a plain INTERRUPTED cycle.
//!
//! This module is the single pure classifier for that state. Two surfaces
//! read it:
//!
//! - [`pane_usage_limit`] / [`pane_is_quota_blocked`] read a pane capture;
//! - [`worker_handback_usage_limit`] reads what a dispatched worker returned.
//!
//! The phrases below are copied from the shipped harness binaries; each table
//! group names its version. A harness that changes its wording degrades to the
//! old behavior (an idle pane), never to a false block, so an unknown string
//! is not guessed at. Every phrase is anchored to the start of a line and a
//! line must be transcript-tail or chrome-positioned, because agents routinely
//! quote these strings (this module's own source, for one).

use crate::HarnessConfig;

/// Longest evidence line kept. Long enough for every known notice; bounded so
/// a pathological capture cannot flood route output or the ops log.
const EVIDENCE_LIMIT: usize = 240;
/// Non-empty bottom lines searched for a harness status gauge.
const GAUGE_WINDOW: usize = 10;
/// Transcript lines allowed below a limit notice before it counts as old
/// history (an upgrade hint, a follow-on dialog line, a sidebar row).
const NOTICE_TRAILING_LINES: usize = 6;
/// A worker's hand-back this short is the limit notice itself, even when a
/// short hint line follows it.
const SHORT_HANDBACK_LINES: usize = 4;

/// Where the limit was read from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageLimitEvidence {
    /// A live reading in the harness chrome. It stays true during a turn.
    StatusGauge,
    /// A notice the harness printed when a request was refused.
    LimitNotice,
}

/// A harness usage or credit limit, with the evidence that proved it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HarnessUsageLimit {
    /// Canonical harness name (`grok`, `claude`, `codex`, `opencode`), or
    /// `None` when the wording alone cannot tell which harness printed it.
    pub harness: Option<String>,
    /// Human name of the exhausted limit, e.g. `weekly limit`.
    pub limit: String,
    /// The matched line, ANSI-stripped, secret-redacted, and bounded.
    pub evidence: String,
    /// The harness's own reset wording, verbatim, when it printed one.
    pub reset_hint: Option<String>,
    pub source: UsageLimitEvidence,
}

impl HarnessUsageLimit {
    /// A limit a caller judged from wording this module does not know.
    pub fn reported(evidence: &str) -> Self {
        Self {
            harness: None,
            limit: "usage limit".to_string(),
            evidence: bounded_evidence(evidence.trim()),
            reset_hint: None,
            source: UsageLimitEvidence::LimitNotice,
        }
    }

    /// Display name of the harness that hit the limit.
    pub fn harness_label(&self) -> &str {
        match self.harness.as_deref() {
            Some("grok") => "Grok Build",
            Some("claude") => "Claude Code",
            Some("codex") => "Codex",
            Some("opencode") => "OpenCode",
            Some(other) => other,
            None => "the harness",
        }
    }

    /// One sentence naming the limit as the cause.
    pub fn cause(&self) -> String {
        format!(
            "{} is out of usage: its {} is used up (`{}`)",
            self.harness_label(),
            self.limit,
            self.evidence,
        )
    }

    /// The operator's next action for a document whose owner hit this limit.
    /// Deliberately never says to retry: a retry cannot succeed before reset.
    pub fn operator_next_step(&self) -> String {
        let wait = match self.reset_hint.as_deref() {
            Some(hint) => format!("Wait for the limit to reset ({hint})"),
            None => "Wait for the limit to reset".to_string(),
        };
        format!(
            "{wait}, or switch this document to another harness by changing `agent:` in its frontmatter. Running it again before then cannot help."
        )
    }
}

/// `true` when `capture` shows `harness` stopped by a usage or credit limit.
/// The one predicate for "do not dispatch or refill into this pane".
pub fn pane_is_quota_blocked(capture: &str, harness: &HarnessConfig) -> bool {
    pane_usage_limit(capture, harness).is_some()
}

/// Read a usage or credit limit from a pane capture (plain or ANSI).
///
/// A status gauge is a live reading, so it holds during a turn. A notice is
/// history: it counts only when no turn is running and nothing but chrome and
/// a few trailing lines sit below it, so a limit that has since reset, or an
/// agent quoting the notice mid-transcript, does not read as exhausted.
pub fn pane_usage_limit(capture: &str, harness: &HarnessConfig) -> Option<HarnessUsageLimit> {
    let lines = capture
        .lines()
        .map(agent_doc_turn_executor_tmux::prompt::strip_ansi)
        .map(|line| line.trim().to_string())
        .filter(|line| !line.is_empty())
        .collect::<Vec<_>>();
    if harness.binary == "grok"
        && let Some(limit) = lines
            .iter()
            .rev()
            .take(GAUGE_WINDOW)
            .find_map(|line| grok_gauge_exhausted(line))
    {
        return Some(limit);
    }
    if harness.has_busy_cue(capture) {
        return None;
    }
    let mut trailing = 0;
    for line in lines.iter().rev() {
        if is_composer_chrome(line, harness) {
            continue;
        }
        if let Some(mut limit) = classify_notice(line) {
            limit.harness = Some(harness.binary.clone());
            if notice_belongs_to(line, &harness.binary) {
                return Some(limit);
            }
        }
        trailing += 1;
        if trailing > NOTICE_TRAILING_LINES {
            return None;
        }
    }
    None
}

/// Read a usage or credit limit from what a dispatched worker handed back: its
/// final result text, or the JSON event a headless harness exited with.
///
/// A real report may discuss these strings at length, so the notice must be
/// the end of the hand-back (or the whole of a very short one), outside any
/// fence or quotation. Harness is `Some` only when the wording is unambiguous.
pub fn worker_handback_usage_limit(text: &str) -> Option<HarnessUsageLimit> {
    let lines = handback_lines(text);
    let last = lines.last()?;
    if let Some(limit) = classify_handback_line(last) {
        return Some(limit);
    }
    if lines.len() <= SHORT_HANDBACK_LINES {
        return lines.iter().find_map(|line| classify_handback_line(line));
    }
    None
}

fn classify_handback_line(line: &str) -> Option<HarnessUsageLimit> {
    let message = json_error_message(line);
    let line = message.as_deref().unwrap_or(line);
    let line = line
        .strip_prefix("API Error: ")
        .or_else(|| line.strip_prefix("Error: "))
        .unwrap_or(line);
    classify_notice(line)
}

/// Top-level prose lines of a hand-back: fenced blocks and blockquotes are
/// quotation, never the harness's own voice.
fn handback_lines(text: &str) -> Vec<String> {
    let mut in_fence = false;
    let mut lines = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") || trimmed.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence || trimmed.is_empty() || trimmed.starts_with('>') {
            continue;
        }
        lines.push(trimmed.to_string());
    }
    lines
}

/// The error text of a headless harness's JSON event, when `line` is one.
/// Shapes: `claude -p` `result`, `codex exec --json` `error.message`, and the
/// `message` of a bare error event.
fn json_error_message(line: &str) -> Option<String> {
    if !line.starts_with('{') {
        return None;
    }
    let event = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let text = |value: Option<&serde_json::Value>| value?.as_str().map(str::to_string);
    let is_error = event
        .get("is_error")
        .and_then(serde_json::Value::as_bool)
        .unwrap_or(false);
    let kind = event.get("type").and_then(serde_json::Value::as_str);
    if is_error {
        return text(event.get("result"));
    }
    if !matches!(kind, Some("error" | "turn.failed")) {
        return None;
    }
    text(event.pointer("/error/message"))
        .or_else(|| text(event.get("message")))
        .map(|message| match message.as_str() {
            // Grok Build's headless error is bare; its agent backend renders
            // the same event with the harness prefix.
            "usage limit reached" | "usage balance exhausted" => {
                format!("Grok Build: {message}")
            }
            _ => message,
        })
}

/// Empty rules, the composer, and harness footers: not transcript.
fn is_composer_chrome(line: &str, harness: &HarnessConfig) -> bool {
    line.chars().all(|ch| {
        ch.is_whitespace()
            || matches!(
                ch,
                '─' | '━' | '╭' | '╮' | '╰' | '╯' | '│' | '┃' | '╹' | '▀'
            )
    }) || harness.is_prompt_line(line)
        || harness.is_ignorable_output_line(line)
        || harness.is_idle_status_line(line)
}

/// One known notice. `harnesses` lists every harness that prints the wording.
struct Notice {
    prefix: &'static str,
    harnesses: &'static [&'static str],
    limit: &'static str,
}

const CLAUDE: &[&str] = &["claude"];
const CODEX: &[&str] = &["codex"];
const OPENCODE: &[&str] = &["opencode"];
const GROK: &[&str] = &["grok"];

/// Longest prefix first where one extends another.
const NOTICES: &[Notice] = &[
    // Claude Code 2.1.311, `/opt/claude-code/bin/claude`: the limit names in
    // its `five_hour`/`seven_day`/... table composed as `You've hit your ...`,
    // plus its own list of limit-message prefixes.
    Notice {
        prefix: "You've hit your limit",
        harnesses: CLAUDE,
        limit: "usage limit",
    },
    Notice {
        prefix: "You've hit your session limit",
        harnesses: CLAUDE,
        limit: "session limit",
    },
    Notice {
        prefix: "You've hit your weekly limit",
        harnesses: CLAUDE,
        limit: "weekly limit",
    },
    Notice {
        prefix: "You've hit your Opus limit",
        harnesses: CLAUDE,
        limit: "Opus limit",
    },
    Notice {
        prefix: "You've hit your Sonnet limit",
        harnesses: CLAUDE,
        limit: "Sonnet limit",
    },
    Notice {
        prefix: "You've hit your Fable limit",
        harnesses: CLAUDE,
        limit: "Fable limit",
    },
    Notice {
        prefix: "You've hit your usage credit limit",
        harnesses: CLAUDE,
        limit: "usage credit limit",
    },
    Notice {
        prefix: "You've hit your monthly spend limit",
        harnesses: CLAUDE,
        limit: "monthly spend limit",
    },
    Notice {
        prefix: "You've hit your channel's monthly spend limit",
        harnesses: CLAUDE,
        limit: "channel's monthly spend limit",
    },
    Notice {
        prefix: "You've hit your team's shared budget",
        harnesses: CLAUDE,
        limit: "team's shared budget",
    },
    Notice {
        prefix: "You're out of usage credits",
        harnesses: CLAUDE,
        limit: "usage credits",
    },
    Notice {
        prefix: "You're out of extra usage",
        harnesses: CLAUDE,
        limit: "extra usage",
    },
    Notice {
        prefix: "Your org is out of usage",
        harnesses: CLAUDE,
        limit: "organization usage",
    },
    Notice {
        prefix: "Your usage allocation has been disabled by your admin",
        harnesses: CLAUDE,
        limit: "usage allocation",
    },
    Notice {
        prefix: "Your group's usage limit is set to $0",
        harnesses: CLAUDE,
        limit: "group usage limit",
    },
    Notice {
        prefix: "Credit balance too low",
        harnesses: CLAUDE,
        limit: "API credit balance",
    },
    Notice {
        prefix: "Credit balance is too low",
        harnesses: CLAUDE,
        limit: "API credit balance",
    },
    Notice {
        prefix: "Goal paused · usage limit reached",
        harnesses: CLAUDE,
        limit: "usage limit",
    },
    Notice {
        prefix: "Usage limit reached ·",
        harnesses: CLAUDE,
        limit: "usage limit",
    },
    // Codex CLI 0.165.0: `UsageLimitReachedError`'s display strings. The shared
    // lead-in carries the plan-specific remainder.
    Notice {
        prefix: "You've hit your usage limit",
        harnesses: &["codex", "claude"],
        limit: "usage limit",
    },
    Notice {
        prefix: "You hit your spend cap",
        harnesses: CODEX,
        limit: "workspace spend cap",
    },
    // OpenCode 1.15.13 (`opencode-ai` npm binary).
    Notice {
        prefix: "Usage limit reached. It will reset in",
        harnesses: OPENCODE,
        limit: "usage limit",
    },
    Notice {
        prefix: "Free usage exceeded, subscribe to Go",
        harnesses: OPENCODE,
        limit: "free usage",
    },
    Notice {
        prefix: "Quota exceeded. Check your plan and billing details.",
        harnesses: OPENCODE,
        limit: "provider quota",
    },
    Notice {
        prefix: "Go limit reached",
        harnesses: OPENCODE,
        limit: "Go plan limit",
    },
    Notice {
        prefix: "Free limit reached",
        harnesses: OPENCODE,
        limit: "free usage",
    },
    // Grok Build 1.0.70, `/usr/bin/grok`.
    Notice {
        prefix: "You hit your weekly limit",
        harnesses: GROK,
        limit: "weekly limit",
    },
    Notice {
        prefix: "You hit your free usage limit",
        harnesses: GROK,
        limit: "free usage limit",
    },
    Notice {
        prefix: "You've hit the credit limit for your plan",
        harnesses: GROK,
        limit: "plan credit limit",
    },
    Notice {
        prefix: "You've hit your spending cap",
        harnesses: GROK,
        limit: "spending cap",
    },
    Notice {
        prefix: "You've reached your free Grok Build usage limit",
        harnesses: GROK,
        limit: "free usage limit",
    },
    Notice {
        prefix: "Grok Build: usage limit reached",
        harnesses: GROK,
        limit: "usage limit",
    },
    Notice {
        prefix: "Grok Build: usage balance exhausted",
        harnesses: GROK,
        limit: "usage balance",
    },
];

/// `<Plan> usage limit reached. It will reset in ...` from OpenCode names the
/// plan first, so it cannot be a fixed prefix.
const OPENCODE_NAMED_LIMIT: &str = " usage limit reached. It will reset in";

fn normalize_notice(line: &str) -> String {
    line.trim_start_matches(|ch: char| {
        ch.is_whitespace()
            || matches!(
                ch,
                '⎿' | '■' | '●' | '⏺' | '•' | '✗' | '✘' | '⚠' | '│' | '┃' | '▌' | '!'
            )
    })
    .trim()
    .replace('\u{2019}', "'")
}

fn classify_notice(line: &str) -> Option<HarnessUsageLimit> {
    let line = normalize_notice(line);
    let (harnesses, limit): (&[&str], String) = if let Some(notice) = NOTICES
        .iter()
        .find(|notice| line.starts_with(notice.prefix))
    {
        (notice.harnesses, notice.limit.to_string())
    } else if let Some(index) = line.find(OPENCODE_NAMED_LIMIT)
        && index > 0
        && !line[..index].contains(['.', ':', '`', '"'])
    {
        (OPENCODE, format!("{} usage limit", &line[..index]))
    } else {
        return None;
    };
    Some(HarnessUsageLimit {
        harness: match harnesses {
            [only] => Some((*only).to_string()),
            _ => None,
        },
        limit,
        evidence: bounded_evidence(&line),
        reset_hint: reset_hint(&line),
        source: UsageLimitEvidence::LimitNotice,
    })
}

fn notice_belongs_to(line: &str, harness: &str) -> bool {
    let line = normalize_notice(line);
    NOTICES
        .iter()
        .find(|notice| line.starts_with(notice.prefix))
        .is_none_or(|notice| notice.harnesses.contains(&harness))
        && (harness == "opencode" || !line.contains(OPENCODE_NAMED_LIMIT))
}

/// The harness's own reset wording. Never parsed into a time: each harness
/// prints its own format and a wrong parse would be worse than the verbatim
/// phrase.
fn reset_hint(line: &str) -> Option<String> {
    const MARKERS: [&str; 4] = [
        "resets ",
        "It will reset in ",
        "try again at ",
        "Try again at ",
    ];
    MARKERS.iter().find_map(|marker| {
        let start = line.find(marker)?;
        let tail = &line[start..];
        let end = tail
            .find(" · ")
            .or_else(|| tail.find(". To continue"))
            .unwrap_or(tail.len());
        Some(tail[..end].trim_end_matches('.').trim().to_string())
    })
}

/// `╰── Weekly limit left: 0% · Grok 4.7 (high) · always-approve ─╯`. Only
/// the composer's bottom border counts: the same words in a transcript are a
/// quotation. Exactly `0%`: a harness that rounds still printed its own
/// reading, and any positive reading is not exhausted.
fn grok_gauge_exhausted(line: &str) -> Option<HarnessUsageLimit> {
    const GAUGE: &str = " limit left: 0%";
    if !line.starts_with('╰') {
        return None;
    }
    let body = line.trim_matches(|ch: char| matches!(ch, '╰' | '╯' | '─' | ' '));
    let reading = body.split(" · ").find(|field| field.ends_with(GAUGE))?;
    let name = reading.strip_suffix(GAUGE)?.trim();
    Some(HarnessUsageLimit {
        harness: Some("grok".to_string()),
        limit: format!("{} limit", name.to_lowercase()),
        evidence: bounded_evidence(reading),
        reset_hint: None,
        source: UsageLimitEvidence::StatusGauge,
    })
}

fn bounded_evidence(line: &str) -> String {
    let redacted = agent_doc_secret_redact::redact(line);
    if redacted.chars().count() <= EVIDENCE_LIMIT {
        return redacted.to_string();
    }
    redacted.chars().take(EVIDENCE_LIMIT).collect::<String>() + "…"
}

#[cfg(test)]
mod tests {
    use super::*;

    const GROK_ZERO: &str = include_str!("fixtures/usage_limit/grok_weekly_zero_idle.txt");
    const GROK_FOUR: &str = include_str!("fixtures/usage_limit/grok_weekly_four_idle.txt");
    const CLAUDE_PANE: &str = include_str!("fixtures/usage_limit/claude_session_limit_idle.txt");
    const CODEX_PANE: &str = include_str!("fixtures/usage_limit/codex_usage_limit_idle.txt");
    const OPENCODE_PANE: &str = include_str!("fixtures/usage_limit/opencode_usage_limit_idle.txt");

    #[test]
    fn grok_weekly_gauge_at_zero_is_the_live_incident() {
        let grok = HarnessConfig::grok();
        let limit = pane_usage_limit(GROK_ZERO, &grok).expect("exhausted gauge");
        assert_eq!(limit.harness.as_deref(), Some("grok"));
        assert_eq!(limit.limit, "weekly limit");
        assert_eq!(limit.evidence, "Weekly limit left: 0%");
        assert_eq!(limit.source, UsageLimitEvidence::StatusGauge);
        // The pane is exactly what the route read as idle: the gauge must
        // explain that idle pane, not change its readiness.
        assert!(crate::pane_idle_dispatch_ready(GROK_ZERO, &grok));
    }

    #[test]
    fn grok_gauge_with_usage_left_is_not_exhausted() {
        let grok = HarnessConfig::grok();
        assert_eq!(pane_usage_limit(GROK_FOUR, &grok), None);
        let tenths = GROK_ZERO.replace("limit left: 0%", "limit left: 0.4%");
        assert_eq!(pane_usage_limit(&tenths, &grok), None);
        let ten = GROK_ZERO.replace("limit left: 0%", "limit left: 10%");
        assert_eq!(pane_usage_limit(&ten, &grok), None);
    }

    #[test]
    fn grok_gauge_stays_true_during_a_turn() {
        let grok = HarnessConfig::grok();
        let busy = GROK_ZERO.replace("Ctrl+x", "Ctrl+c:cancel  │  Ctrl+x");
        assert!(grok.has_busy_cue(&busy) || grok.dispatch_blocker_reason(&busy).is_some());
        assert!(pane_is_quota_blocked(&busy, &grok));
    }

    #[test]
    fn grok_gauge_words_in_the_transcript_are_a_quotation() {
        let grok = HarnessConfig::grok();
        let quoted = GROK_FOUR.replace(
            "Worked for 9m12s",
            "Weekly limit left: 0% · Grok 4.7 (high) · always-approve",
        );
        assert_eq!(pane_usage_limit(&quoted, &grok), None);
    }

    #[test]
    fn claude_session_limit_names_the_limit_and_its_reset() {
        let limit = pane_usage_limit(CLAUDE_PANE, &HarnessConfig::claude()).expect("limit");
        assert_eq!(limit.harness.as_deref(), Some("claude"));
        assert_eq!(limit.limit, "session limit");
        assert_eq!(
            limit.reset_hint.as_deref(),
            Some("resets 9pm (America/New_York)")
        );
        assert_eq!(limit.source, UsageLimitEvidence::LimitNotice);
    }

    #[test]
    fn codex_notice_uses_a_curly_apostrophe() {
        assert!(CODEX_PANE.contains("You\u{2019}ve hit"));
        let limit = pane_usage_limit(CODEX_PANE, &HarnessConfig::codex()).expect("limit");
        assert_eq!(limit.harness.as_deref(), Some("codex"));
        assert_eq!(limit.limit, "usage limit");
        assert_eq!(limit.reset_hint.as_deref(), Some("try again at 3:14 PM"));
    }

    #[test]
    fn opencode_notice_names_its_plan() {
        let limit = pane_usage_limit(OPENCODE_PANE, &HarnessConfig::opencode()).expect("limit");
        assert_eq!(limit.harness.as_deref(), Some("opencode"));
        assert_eq!(limit.limit, "Go usage limit");
        assert_eq!(
            limit.reset_hint.as_deref(),
            Some("It will reset in 3 hours 12 minutes")
        );
    }

    #[test]
    fn a_notice_buried_above_later_output_is_history() {
        let later = (1..=12)
            .map(|n| format!("⏺ finished step {n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let resumed = CLAUDE_PANE.replacen("\n────", &format!("\n{later}\n────"), 1);
        assert_eq!(pane_usage_limit(&resumed, &HarnessConfig::claude()), None);
    }

    #[test]
    fn a_notice_above_a_running_turn_is_history() {
        let claude = HarnessConfig::claude();
        let running =
            CLAUDE_PANE.replacen("\n────", "\n✢ Thinking… (12s · esc to interrupt)\n────", 1);
        assert!(claude.has_busy_cue(&running));
        assert_eq!(pane_usage_limit(&running, &claude), None);
    }

    #[test]
    fn the_operator_draft_is_never_a_notice() {
        let claude = HarnessConfig::claude();
        let idle = CLAUDE_PANE.replace(
            "⎿  You've hit your session limit · resets 9pm (America/New_York)\n     /upgrade to increase your usage limit.",
            "done",
        );
        let draft = idle.replace("❯ \n", "❯ You've hit your session limit\n");
        assert_eq!(pane_usage_limit(&draft, &claude), None);
    }

    #[test]
    fn another_harnesss_wording_does_not_match_this_pane() {
        assert_eq!(
            pane_usage_limit(OPENCODE_PANE, &HarnessConfig::claude()),
            None
        );
        assert_eq!(pane_usage_limit(CLAUDE_PANE, &HarnessConfig::grok()), None);
    }

    #[test]
    fn fast_mode_limit_is_not_a_block() {
        // Claude's fast mode falls back; the session keeps working.
        let pane = CLAUDE_PANE.replace(
            "You've hit your session limit",
            "You've hit your fast limit",
        );
        assert_eq!(pane_usage_limit(&pane, &HarnessConfig::claude()), None);
    }

    #[test]
    fn the_next_step_names_the_limit_and_never_advises_a_retry() {
        let limit = pane_usage_limit(GROK_ZERO, &HarnessConfig::grok()).unwrap();
        let cause = limit.cause();
        assert!(cause.contains("Grok Build"), "{cause}");
        assert!(cause.contains("Weekly limit left: 0%"), "{cause}");
        let next = limit.operator_next_step();
        assert!(next.contains("Wait for the limit to reset"), "{next}");
        assert!(next.contains("`agent:`"), "{next}");
        assert!(!next.contains("again in a few seconds"), "{next}");
    }

    #[test]
    fn worker_handback_is_the_notice_itself() {
        for (fixture, harness, limit) in [
            (
                include_str!("fixtures/usage_limit/worker_claude_handback.txt"),
                Some("claude"),
                "weekly limit",
            ),
            (
                include_str!("fixtures/usage_limit/worker_claude_partial_then_limit.txt"),
                Some("claude"),
                "session limit",
            ),
            (
                include_str!("fixtures/usage_limit/worker_claude_print_result.jsonl"),
                Some("claude"),
                "session limit",
            ),
            (
                include_str!("fixtures/usage_limit/worker_codex_exec_turn_failed.jsonl"),
                None,
                "usage limit",
            ),
            (
                include_str!("fixtures/usage_limit/worker_grok_headless_error.jsonl"),
                Some("grok"),
                "usage limit",
            ),
            (
                include_str!("fixtures/usage_limit/worker_opencode_handback.txt"),
                Some("opencode"),
                "usage limit",
            ),
        ] {
            let found = worker_handback_usage_limit(fixture)
                .unwrap_or_else(|| panic!("no limit read from: {fixture}"));
            assert_eq!(found.harness.as_deref(), harness, "{fixture}");
            assert_eq!(found.limit, limit, "{fixture}");
        }
    }

    #[test]
    fn a_real_report_that_discusses_limits_is_not_exhausted() {
        for fixture in [
            include_str!("fixtures/usage_limit/worker_real_report_quoting_limit.txt"),
            include_str!("fixtures/usage_limit/worker_real_report_ending_in_quote.txt"),
        ] {
            assert_eq!(worker_handback_usage_limit(fixture), None, "{fixture}");
        }
        assert_eq!(worker_handback_usage_limit("Done. All tests pass."), None);
        assert_eq!(worker_handback_usage_limit(""), None);
        let long_report = format!(
            "{}\nYou've hit your session limit · resets 9pm\nThat string is now covered by a fixture.\n",
            "A finding line.\n".repeat(8)
        );
        assert_eq!(worker_handback_usage_limit(&long_report), None);
    }

    #[test]
    fn a_non_error_json_line_is_not_a_notice() {
        let event =
            r#"{"type":"result","is_error":false,"result":"You've hit your session limit"}"#;
        assert_eq!(worker_handback_usage_limit(event), None);
    }

    #[test]
    fn evidence_is_redacted_and_bounded() {
        let line = format!("You've hit your limit {}", "x".repeat(400));
        let limit = classify_notice(&line).unwrap();
        assert!(limit.evidence.chars().count() <= EVIDENCE_LIMIT + 1);
    }
}
