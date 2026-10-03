//! Operator-edit completion gate (`#steeringtypinggate`, `#qheadcomposing`).
//!
//! "Is the operator done typing this?" has one answer in agent-doc, shared by
//! preflight admission (`wait_for_operator_edit_quiescence`) and steering
//! delivery (`midturn_steering::observe`). Both read [`settle_decision`]; only
//! their mechanics differ (preflight waits inside one bounded call, steering
//! re-observes from durable watermarks).
//!
//! The gate is deterministic:
//! 1. **Structural completion** ([`completion_signal`]): a line that ends in a
//!    function word or article (`… publish the`), an unbalanced backtick,
//!    quote, paren, bracket, or code fence, or a dangling connector (`,` `:`
//!    `+`) is plainly unfinished. Terminal punctuation, a closed `[#id]`, or a
//!    URL is plainly finished. Anything else is inconclusive.
//! 2. **Adaptive quiescence**: a finished-looking edit settles after half the
//!    debounce window, an inconclusive one after the full window, an
//!    unfinished one not at all on quiescence alone.
//! 3. **Hard max-hold**: past `max_hold_ms` an item is delivered anyway,
//!    flagged `possibly_partial`, so a stuck gate can never swallow a prompt.
//!
//! An optional [`CompletionClassifier`] may resolve an *inconclusive or
//! unfinished-looking* edit earlier. None is built in: the deterministic gate is
//! always active, and a classifier's answer can only shorten a hold, never
//! extend one past max-hold.

/// What the text alone says about whether the operator finished it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionSignal {
    /// Plainly finished: terminal punctuation, a closed `[#id]`, a URL.
    Complete,
    /// No structural evidence either way.
    Inconclusive,
    /// Plainly unfinished: trailing function word, unbalanced delimiter,
    /// dangling connector, open fence, empty bullet.
    Incomplete,
}

/// A classifier's verdict on one edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionVerdict {
    Complete,
    Incomplete,
    StillTyping,
}

/// An optional completion classifier consulted only when the deterministic
/// signals are inconclusive or say "unfinished". Implementations must be
/// cheap to call from a hot path, so the trait answers from a cache keyed by
/// the edit's content hash; producing a verdict (a local model, for example)
/// happens elsewhere and fills that cache. None is enabled today.
pub trait CompletionClassifier: std::fmt::Debug {
    fn cached_verdict(&self, content_hash: &str) -> Option<CompletionVerdict>;
}

/// The deterministic-only classifier: never has a verdict.
#[derive(Debug, Clone, Copy, Default)]
pub struct DeterministicOnly;

impl CompletionClassifier for DeterministicOnly {
    fn cached_verdict(&self, _content_hash: &str) -> Option<CompletionVerdict> {
        None
    }
}

/// Default hard max-hold before an unsettled item is delivered anyway.
pub const DEFAULT_MAX_HOLD_MS: u64 = 45_000;

/// Trailing words that leave a sentence unfinished.
const DANGLING_WORDS: &[&str] = &[
    "a", "an", "the", "and", "or", "but", "nor", "to", "of", "with", "without", "for", "in",
    "into", "on", "onto", "at", "by", "from", "as", "than", "that", "which", "who", "whose", "if",
    "then", "so", "because", "while", "when", "where", "whether", "is", "are", "was", "were", "be",
    "been", "should", "would", "could", "can", "will", "shall", "must", "may", "might", "do",
    "does", "we", "i", "you", "it", "this", "these", "those", "my", "our", "your", "their", "its",
    "not", "also", "via", "per",
];

/// Characters that leave a line dangling when they end it.
const DANGLING_TAILS: &[char] = &[
    ',', ':', ';', '+', '&', '/', '=', '(', '[', '{', '-', '|', '\\',
];

fn strip_code_spans(text: &str) -> String {
    // Fenced blocks are judged by fence balance; inline spans by backtick
    // balance. Neither should contribute words or brackets to the checks.
    let mut out = String::with_capacity(text.len());
    let mut in_span = false;
    for ch in text.chars() {
        if ch == '`' {
            in_span = !in_span;
            continue;
        }
        if !in_span {
            out.push(ch);
        }
    }
    out
}

fn unbalanced(text: &str, open: char, close: char) -> bool {
    let mut depth: i64 = 0;
    for ch in text.chars() {
        if ch == open {
            depth += 1;
        } else if ch == close {
            depth -= 1;
        }
    }
    depth > 0
}

/// Structural completion signal for one edit's text.
pub fn completion_signal(text: &str) -> CompletionSignal {
    let trimmed = text.trim_end();
    if trimmed.trim().is_empty() {
        return CompletionSignal::Incomplete;
    }
    let lines: Vec<&str> = trimmed.lines().collect();
    let fences = lines
        .iter()
        .map(|line| line.trim_start())
        .filter(|line| line.starts_with("```") || line.starts_with("~~~"))
        .count();
    if fences % 2 == 1 {
        return CompletionSignal::Incomplete;
    }
    let last_line = lines.last().copied().unwrap_or("").trim();
    if fences > 0 && (last_line.starts_with("```") || last_line.starts_with("~~~")) {
        return CompletionSignal::Complete;
    }
    // Outside fenced blocks, judge delimiter balance on prose only.
    let mut prose = String::new();
    let mut in_fence = false;
    for line in &lines {
        let t = line.trim_start();
        if t.starts_with("```") || t.starts_with("~~~") {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    if prose.chars().filter(|&ch| ch == '`').count() % 2 == 1 {
        return CompletionSignal::Incomplete;
    }
    let scan = strip_code_spans(&prose);
    if scan.chars().filter(|&ch| ch == '"').count() % 2 == 1
        || unbalanced(&scan, '(', ')')
        || unbalanced(&scan, '[', ']')
        || unbalanced(&scan, '{', '}')
    {
        return CompletionSignal::Incomplete;
    }
    if matches!(last_line, "-" | "*" | "+") || last_line.ends_with('#') {
        return CompletionSignal::Incomplete;
    }
    if last_line.ends_with('`') {
        // The line ends in a closed code span: a finished reference.
        return CompletionSignal::Complete;
    }
    let tail = strip_code_spans(last_line);
    let tail = tail.trim_end();
    if tail.ends_with(DANGLING_TAILS) {
        return CompletionSignal::Incomplete;
    }
    let last_token = tail.split_whitespace().last().unwrap_or("");
    if last_token.starts_with("http://") || last_token.starts_with("https://") {
        return CompletionSignal::Complete;
    }
    if tail.ends_with(['.', '?', '!', ')', ']', '"', '\'', '>']) {
        return CompletionSignal::Complete;
    }
    let last_word = last_token
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_ascii_lowercase();
    if DANGLING_WORDS.contains(&last_word.as_str()) {
        return CompletionSignal::Incomplete;
    }
    CompletionSignal::Inconclusive
}

/// Everything one settle decision needs, all observable without I/O.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SettleInputs {
    /// How long the document has been unchanged.
    pub quiet_for_ms: Option<u64>,
    /// How long this exact edit has been observed unchanged.
    pub stable_for_ms: Option<u64>,
    /// How long this item has been held (first observation of any version).
    pub held_for_ms: u64,
    pub debounce_ms: u64,
    pub max_hold_ms: u64,
    pub signal: CompletionSignal,
    pub verdict: Option<CompletionVerdict>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleDecision {
    /// Finished: deliver.
    Settled,
    /// Past max-hold without settling: deliver, flagged possibly partial.
    MaxHoldExpired,
    /// Keep holding; `recheck_after_ms` is when the decision can next change.
    Held { recheck_after_ms: u64 },
}

impl SettleDecision {
    pub fn deliver(self) -> bool {
        !matches!(self, Self::Held { .. })
    }
}

/// The one settle decision (`#steeringtypinggate`).
pub fn settle_decision(inputs: SettleInputs) -> SettleDecision {
    let quiet = inputs.quiet_for_ms.max(inputs.stable_for_ms).unwrap_or(0);
    let effective = match (inputs.signal, inputs.verdict) {
        (_, Some(CompletionVerdict::Complete)) => CompletionSignal::Complete,
        (_, Some(CompletionVerdict::Incomplete | CompletionVerdict::StillTyping)) => {
            CompletionSignal::Incomplete
        }
        (signal, None) => signal,
    };
    let window = match effective {
        CompletionSignal::Complete => Some(inputs.debounce_ms / 2),
        CompletionSignal::Inconclusive => Some(inputs.debounce_ms),
        CompletionSignal::Incomplete => None,
    };
    if let Some(window) = window
        && quiet >= window
    {
        return SettleDecision::Settled;
    }
    if inputs.held_for_ms.max(quiet) >= inputs.max_hold_ms {
        return SettleDecision::MaxHoldExpired;
    }
    let until_window = window.map(|window| window.saturating_sub(quiet));
    let until_max = inputs
        .max_hold_ms
        .saturating_sub(inputs.held_for_ms.max(quiet));
    SettleDecision::Held {
        recheck_after_ms: until_window
            .map_or(until_max, |w| w.min(until_max))
            .max(100),
    }
}

/// `#qheadcomposing` + `#steeringtypinggate`: true when every line `current`
/// adds relative to `admitted` is structurally finished (not
/// [`CompletionSignal::Incomplete`]). Preflight uses this with its quiescence
/// window so a quiet-but-unfinished line keeps it waiting (to its ceiling).
pub fn added_lines_look_finished(admitted: &str, current: &str) -> bool {
    let before: std::collections::HashSet<&str> = admitted.lines().map(str::trim).collect();
    current
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty() && !before.contains(line))
        .map(|line| line.strip_prefix("- ").unwrap_or(line))
        .all(|line| completion_signal(line) != CompletionSignal::Incomplete)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The real case (tasks/software/tsift.md, 2026-10-03): the deterministic
    /// path must not call this fragment finished.
    #[test]
    fn tsift_fragment_is_incomplete() {
        assert_eq!(
            completion_signal("Should we release + publish the"),
            CompletionSignal::Incomplete
        );
        assert_eq!(
            completion_signal("Should we release + publish the "),
            CompletionSignal::Incomplete
        );
        assert_eq!(
            completion_signal("Should we release + publish the C++ bindings?"),
            CompletionSignal::Complete
        );
    }

    #[test]
    fn structural_signals() {
        for unfinished in [
            "fix the `foo",
            "rename (the helper",
            "say \"hello",
            "see [#abc",
            "```rust\nfn main() {",
            "items:",
            "this and",
            "-",
            "do it with",
        ] {
            assert_eq!(
                completion_signal(unfinished),
                CompletionSignal::Incomplete,
                "{unfinished:?}"
            );
        }
        for finished in [
            "Fix the bug.",
            "Why does it hang?",
            "do [#abc]",
            "#subagent: https://github.com/btakita/agent-doc/issues/118",
            "use `foo()`",
            "```\ncode\n```",
        ] {
            assert_eq!(
                completion_signal(finished),
                CompletionSignal::Complete,
                "{finished:?}"
            );
        }
        assert_eq!(
            completion_signal("release + publish"),
            CompletionSignal::Inconclusive
        );
    }

    fn inputs(signal: CompletionSignal, quiet: u64, held: u64) -> SettleInputs {
        SettleInputs {
            quiet_for_ms: Some(quiet),
            stable_for_ms: None,
            held_for_ms: held,
            debounce_ms: 2000,
            max_hold_ms: 45_000,
            signal,
            verdict: None,
        }
    }

    #[test]
    fn adaptive_quiescence_and_max_hold() {
        use CompletionSignal::*;
        assert_eq!(
            settle_decision(inputs(Complete, 1000, 1000)),
            SettleDecision::Settled
        );
        assert!(!settle_decision(inputs(Inconclusive, 1000, 1000)).deliver());
        assert_eq!(
            settle_decision(inputs(Inconclusive, 2000, 2000)),
            SettleDecision::Settled
        );
        // Unfinished text never settles on quiescence alone...
        assert_eq!(
            settle_decision(inputs(Incomplete, 30_000, 30_000)),
            SettleDecision::Held {
                recheck_after_ms: 15_000
            }
        );
        // ...but max-hold guarantees delivery, flagged.
        assert_eq!(
            settle_decision(inputs(Incomplete, 45_000, 45_000)),
            SettleDecision::MaxHoldExpired
        );
    }

    #[test]
    fn a_classifier_can_only_shorten_a_hold() {
        let mut held = inputs(CompletionSignal::Incomplete, 1500, 1500);
        held.verdict = Some(CompletionVerdict::Complete);
        assert_eq!(settle_decision(held), SettleDecision::Settled);
        let mut typing = inputs(CompletionSignal::Complete, 45_000, 45_000);
        typing.verdict = Some(CompletionVerdict::StillTyping);
        assert_eq!(settle_decision(typing), SettleDecision::MaxHoldExpired);
        assert_eq!(DeterministicOnly.cached_verdict("x"), None);
    }

    #[test]
    fn preflight_added_line_check() {
        let admitted = "- current\n";
        assert!(!added_lines_look_finished(
            admitted,
            "- current\n- Should we release + publish the\n"
        ));
        assert!(added_lines_look_finished(
            admitted,
            "- current\n- Should we release + publish the C++ bindings?\n"
        ));
        assert!(added_lines_look_finished(admitted, admitted));
    }
}
