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
//! An optional [`CompletionClassifier`] may answer before the deterministic
//! tier does (`#steergateperceptron`: the online-learned
//! [`crate::learned_gate::LearnedGate`]). The deterministic rules stay the hard
//! floor: an unbalanced delimiter always holds, and the max-hold always
//! delivers, whatever the classifier says. A "finished" verdict settles after
//! half the quiet window at the earliest; a "still typing" verdict holds at
//! most until max-hold.

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

    /// The verdict for one decision, given its observable features
    /// (`#steergateperceptron`). Defaults to the content-hash cache.
    fn assess(&self, _features: &GateFeatures, content_hash: &str) -> Option<CompletionVerdict> {
        self.cached_verdict(content_hash)
    }
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
    if ends_with_empty_code_span(last_line) {
        // `#halftypedcoin`: an empty trailing span (`Add a ``) is the editor's
        // auto-paired backticks with the caret between them, not a finished
        // reference: the operator is about to type the span's content.
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

/// True when `line` ends with an empty inline code span: a bare backtick pair
/// standing alone as its own token (`` Add a `` ``), the shape an editor's
/// auto-paired backticks leave while the caret sits between them
/// (`#halftypedcoin`). A double-backtick span closing real content
/// (``` ``ab`` ```) is not empty: its closing pair follows a non-space.
fn ends_with_empty_code_span(line: &str) -> bool {
    let trimmed = line.trim_end();
    let Some(before) = trimmed.strip_suffix("``") else {
        return false;
    };
    if before.ends_with('`') {
        return false;
    }
    before
        .chars()
        .next_back()
        .is_none_or(|ch| ch.is_whitespace() || matches!(ch, '(' | '[' | '{' | '"' | '\''))
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
    /// The text leaves a delimiter open: a hard hold no verdict overrides.
    pub unbalanced_delimiters: bool,
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
///
/// The deterministic tier decides unless a classifier verdict applies:
/// - `Complete` settles once half the quiet window has passed, except over
///   an unbalanced delimiter (hard floor);
/// - `Incomplete` / `StillTyping` holds what the deterministic tier would
///   settle, re-checking after half the window, never past max-hold (hard
///   floor).
pub fn settle_decision(inputs: SettleInputs) -> SettleDecision {
    let deterministic = windowed_decision(inputs, inputs.signal);
    match inputs.verdict {
        None => deterministic,
        Some(CompletionVerdict::Complete) if inputs.unbalanced_delimiters => deterministic,
        Some(CompletionVerdict::Complete) => windowed_decision(inputs, CompletionSignal::Complete),
        Some(CompletionVerdict::Incomplete | CompletionVerdict::StillTyping) => {
            if deterministic != SettleDecision::Settled {
                return deterministic;
            }
            let quiet = inputs.quiet_for_ms.max(inputs.stable_for_ms).unwrap_or(0);
            let held = inputs.held_for_ms.max(quiet);
            if held >= inputs.max_hold_ms {
                return SettleDecision::MaxHoldExpired;
            }
            SettleDecision::Held {
                recheck_after_ms: (inputs.debounce_ms / 2)
                    .min(inputs.max_hold_ms - held)
                    .max(100),
            }
        }
    }
}

fn windowed_decision(inputs: SettleInputs, effective: CompletionSignal) -> SettleDecision {
    let quiet = inputs.quiet_for_ms.max(inputs.stable_for_ms).unwrap_or(0);
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

// ---------------------------------------------------------------------------
// Decision features (`#steergatelog`).
//
// Every settle decision can be described by a small, fixed set of observable
// features. The decision log (`agent_doc_session_check_io::steering_gate_log`)
// persists them with each decision so outcomes observed later (a re-edit right
// after delivery, a hold nobody needed) can label the decision. They are all
// pure functions of the text, the settle inputs, and the operator's pause
// profile, so the same features are computed in every harness.
// ---------------------------------------------------------------------------

/// Class of the last token of an edit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TrailingToken {
    /// Nothing but whitespace.
    Empty,
    /// `a`, `an`, `the`.
    Article,
    /// `and`, `or`, `but`, `if`, `that`, `which`, ...
    Conjunction,
    /// `to`, `of`, `with`, `for`, `in`, ...
    Preposition,
    /// Any other dangling function word: auxiliaries, pronouns, `not`.
    FunctionWord,
    /// Ends in `?`.
    Question,
    /// Ends in `.` or `!`.
    Terminal,
    /// A closed `[#id]` reference.
    IdRef,
    /// A URL.
    Url,
    /// A closed inline code span.
    CodeSpan,
    /// A dangling connector (`,` `:` `+` `&` `/` `=` an open bracket, ...).
    DanglingPunct,
    /// A closing quote, paren, or bracket that is not an id reference.
    Closer,
    /// An ordinary content word.
    Word,
}

impl TrailingToken {
    pub const ALL: [Self; 13] = [
        Self::Empty,
        Self::Article,
        Self::Conjunction,
        Self::Preposition,
        Self::FunctionWord,
        Self::Question,
        Self::Terminal,
        Self::IdRef,
        Self::Url,
        Self::CodeSpan,
        Self::DanglingPunct,
        Self::Closer,
        Self::Word,
    ];

    pub fn parse(text: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|class| class.as_str() == text)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Empty => "empty",
            Self::Article => "article",
            Self::Conjunction => "conjunction",
            Self::Preposition => "preposition",
            Self::FunctionWord => "function_word",
            Self::Question => "question",
            Self::Terminal => "terminal",
            Self::IdRef => "id_ref",
            Self::Url => "url",
            Self::CodeSpan => "code_span",
            Self::DanglingPunct => "dangling_punct",
            Self::Closer => "closer",
            Self::Word => "word",
        }
    }
}

const ARTICLES: &[&str] = &["a", "an", "the"];
const CONJUNCTIONS: &[&str] = &[
    "and", "or", "but", "nor", "so", "because", "while", "when", "where", "whether", "if", "then",
    "than", "that", "which", "who", "whose",
];
const PREPOSITIONS: &[&str] = &[
    "to", "of", "with", "without", "for", "in", "into", "on", "onto", "at", "by", "from", "as",
    "via", "per",
];

/// The class of the last token of `text` (`#steergatelog`).
pub fn trailing_token(text: &str) -> TrailingToken {
    let trimmed = text.trim_end();
    if trimmed.trim().is_empty() {
        return TrailingToken::Empty;
    }
    let last_line = trimmed.lines().last().unwrap_or("").trim();
    if last_line.ends_with('`') && last_line.chars().filter(|&ch| ch == '`').count() % 2 == 0 {
        return TrailingToken::CodeSpan;
    }
    let tail = strip_code_spans(last_line);
    let tail = tail.trim_end();
    if tail.ends_with(DANGLING_TAILS) {
        return TrailingToken::DanglingPunct;
    }
    let last_token = tail.split_whitespace().last().unwrap_or("");
    if last_token.starts_with("http://") || last_token.starts_with("https://") {
        return TrailingToken::Url;
    }
    if last_token.ends_with(']') && last_token.contains("[#") {
        return TrailingToken::IdRef;
    }
    if tail.ends_with('?') {
        return TrailingToken::Question;
    }
    if tail.ends_with(['.', '!']) {
        return TrailingToken::Terminal;
    }
    if tail.ends_with([')', ']', '"', '\'', '>']) {
        return TrailingToken::Closer;
    }
    let word = last_token
        .trim_matches(|ch: char| !ch.is_alphanumeric())
        .to_ascii_lowercase();
    let word = word.as_str();
    if ARTICLES.contains(&word) {
        TrailingToken::Article
    } else if CONJUNCTIONS.contains(&word) {
        TrailingToken::Conjunction
    } else if PREPOSITIONS.contains(&word) {
        TrailingToken::Preposition
    } else if DANGLING_WORDS.contains(&word) {
        TrailingToken::FunctionWord
    } else {
        TrailingToken::Word
    }
}

/// True when `text` leaves a backtick, quote, paren, bracket, brace, or code
/// fence open. The same balance rules [`completion_signal`] applies.
pub fn has_unbalanced_delimiters(text: &str) -> bool {
    let lines: Vec<&str> = text.trim_end().lines().collect();
    let fence = |line: &str| {
        let t = line.trim_start();
        t.starts_with("```") || t.starts_with("~~~")
    };
    if lines.iter().filter(|line| fence(line)).count() % 2 == 1 {
        return true;
    }
    let mut prose = String::new();
    let mut in_fence = false;
    for line in &lines {
        if fence(line) {
            in_fence = !in_fence;
            continue;
        }
        if !in_fence {
            prose.push_str(line);
            prose.push('\n');
        }
    }
    if prose.chars().filter(|&ch| ch == '`').count() % 2 == 1 {
        return true;
    }
    let scan = strip_code_spans(&prose);
    scan.chars().filter(|&ch| ch == '"').count() % 2 == 1
        || unbalanced(&scan, '(', ')')
        || unbalanced(&scan, '[', ']')
        || unbalanced(&scan, '{', '}')
}

fn starts_list_item(line: &str) -> bool {
    let t = line.trim_start();
    if matches!(t, "-" | "*" | "+") || t.starts_with("- ") || t.starts_with("* ") {
        return true;
    }
    if t.starts_with("+ ") {
        return true;
    }
    let digits = t.chars().take_while(char::is_ascii_digit).count();
    digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") "))
}

/// True when the operator already started another list item after the one
/// whose text is `verbatim` (`#steergatelog`): moving on to a new bullet is
/// evidence the previous one is finished.
pub fn closed_list_item(document: &str, verbatim: &str) -> bool {
    let Some(last) = verbatim
        .lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
    else {
        return false;
    };
    let lines: Vec<&str> = document.lines().collect();
    let Some(index) = lines
        .iter()
        .rposition(|line| line.trim_end().ends_with(last))
    else {
        return false;
    };
    lines[index + 1..]
        .iter()
        .find(|line| !line.trim().is_empty())
        .is_some_and(|line| starts_list_item(line))
}

/// The edit's component.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GateComponent {
    Queue,
    Exchange,
}

impl GateComponent {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queue => "queue",
            Self::Exchange => "exchange",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "queue" => Some(Self::Queue),
            "exchange" => Some(Self::Exchange),
            _ => None,
        }
    }
}

impl CompletionSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Complete => "complete",
            Self::Inconclusive => "inconclusive",
            Self::Incomplete => "incomplete",
        }
    }

    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "complete" => Some(Self::Complete),
            "inconclusive" => Some(Self::Inconclusive),
            "incomplete" => Some(Self::Incomplete),
            _ => None,
        }
    }
}

/// Which deterministic tier produced (or would produce) a decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleTier {
    /// Finished-looking text settled after half the quiet window.
    HalfWindow,
    /// Inconclusive text settled after the full quiet window.
    FullWindow,
    /// Delivered by the hard max-hold, flagged `possibly_partial`.
    MaxHold,
    /// Held: the text looks unfinished.
    HeldUnfinished,
    /// Held: still inside the window its signal requires.
    HeldQuiet,
}

impl SettleTier {
    pub fn parse(text: &str) -> Option<Self> {
        [
            Self::HalfWindow,
            Self::FullWindow,
            Self::MaxHold,
            Self::HeldUnfinished,
            Self::HeldQuiet,
        ]
        .into_iter()
        .find(|tier| tier.as_str() == text)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::HalfWindow => "half_window",
            Self::FullWindow => "full_window",
            Self::MaxHold => "max_hold",
            Self::HeldUnfinished => "held_unfinished",
            Self::HeldQuiet => "held_quiet",
        }
    }
}

/// The deterministic tier for `inputs` (the classifier verdict is ignored).
pub fn deterministic_tier(inputs: SettleInputs) -> SettleTier {
    let decision = settle_decision(SettleInputs {
        verdict: None,
        ..inputs
    });
    match (decision, inputs.signal) {
        (SettleDecision::MaxHoldExpired, _) => SettleTier::MaxHold,
        (SettleDecision::Settled, CompletionSignal::Inconclusive) => SettleTier::FullWindow,
        (SettleDecision::Settled, _) => SettleTier::HalfWindow,
        (SettleDecision::Held { .. }, CompletionSignal::Incomplete) => SettleTier::HeldUnfinished,
        (SettleDecision::Held { .. }, _) => SettleTier::HeldQuiet,
    }
}

/// Everything observable about one settle decision (`#steergatelog`).
///
/// Integers only, so a feature row compares exactly and round-trips through
/// storage without float drift. (This crate has no dependencies, so the
/// serialized form lives with the decision log.)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GateFeatures {
    pub trailing_token: TrailingToken,
    /// The structural signal the gate used (a removal counts as complete).
    pub signal: CompletionSignal,
    pub unbalanced_delimiters: bool,
    pub closed_list_item: bool,
    /// How long the edit has been quiet: the larger of document quiet and
    /// this exact version's stability.
    pub quiet_ms: u64,
    /// The operator's rolling median pause between document edits, when known.
    pub median_pause_ms: Option<u64>,
    /// Characters typed into this item per minute since it started, when
    /// more than one observation of the item exists.
    pub typing_chars_per_min: Option<u32>,
    /// Time since the item was first observed (any version).
    pub item_age_ms: u64,
    pub component: GateComponent,
    pub debounce_ms: u64,
    pub max_hold_ms: u64,
}

impl GateFeatures {
    /// The current pause relative to the operator's median pause.
    pub fn pause_ratio(&self) -> Option<f64> {
        self.median_pause_ms
            .filter(|median| *median > 0)
            .map(|median| self.quiet_ms as f64 / median as f64)
    }
}

/// Typing speed in characters per minute, `None` without a measurable span.
pub fn typing_chars_per_min(chars_added: u64, elapsed_ms: u64) -> Option<u32> {
    (chars_added > 0 && elapsed_ms > 0)
        .then(|| (chars_added.saturating_mul(60_000) / elapsed_ms).min(u64::from(u32::MAX)) as u32)
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
            "Add a ``",
            "Add a `` ",
            "``",
            "wrap it (``",
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
            "use ``ab``",
            "Add a `Dashboard`",
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
            unbalanced_delimiters: false,
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
    fn trailing_token_classes() {
        use TrailingToken::*;
        for (text, class) in [
            ("", Empty),
            ("Should we release + publish the", Article),
            ("this and", Conjunction),
            ("do it with", Preposition),
            ("we should", FunctionWord),
            ("Why does it hang?", Question),
            ("Fix the bug.", Terminal),
            ("do [#abc]", IdRef),
            (
                "#subagent: https://github.com/btakita/agent-doc/issues/118",
                Url,
            ),
            ("use `foo()`", CodeSpan),
            ("items:", DanglingPunct),
            ("release + publish", Word),
            ("(see above)", Closer),
        ] {
            assert_eq!(trailing_token(text), class, "{text:?}");
        }
    }

    #[test]
    fn unbalanced_delimiters_and_closed_list_items() {
        assert!(has_unbalanced_delimiters("fix the `foo"));
        assert!(has_unbalanced_delimiters("rename (the helper"));
        assert!(has_unbalanced_delimiters("```rust\nfn main() {"));
        assert!(!has_unbalanced_delimiters(
            "Should we release + publish the"
        ));
        assert!(!has_unbalanced_delimiters("use `foo()` (now)"));
        let doc = "<!-- agent:queue -->\n- first item\n- second\n<!-- /agent:queue -->\n";
        assert!(closed_list_item(doc, "first item"));
        assert!(!closed_list_item(doc, "second"));
        let started = "- first item\n-\n";
        assert!(closed_list_item(started, "first item"));
    }

    #[test]
    fn deterministic_tiers() {
        use CompletionSignal::*;
        assert_eq!(
            deterministic_tier(inputs(Complete, 1000, 1000)),
            SettleTier::HalfWindow
        );
        assert_eq!(
            deterministic_tier(inputs(Inconclusive, 2000, 2000)),
            SettleTier::FullWindow
        );
        assert_eq!(
            deterministic_tier(inputs(Inconclusive, 1000, 1000)),
            SettleTier::HeldQuiet
        );
        assert_eq!(
            deterministic_tier(inputs(Incomplete, 3000, 3000)),
            SettleTier::HeldUnfinished
        );
        assert_eq!(
            deterministic_tier(inputs(Incomplete, 45_000, 45_000)),
            SettleTier::MaxHold
        );
        assert_eq!(typing_chars_per_min(30, 6_000), Some(300));
        assert_eq!(typing_chars_per_min(0, 6_000), None);
        for class in TrailingToken::ALL {
            assert_eq!(TrailingToken::parse(class.as_str()), Some(class));
        }
        assert_eq!(SettleTier::parse("max_hold"), Some(SettleTier::MaxHold));
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
