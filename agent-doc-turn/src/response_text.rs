//! Pure response text policy for turn closeout and write guards.

/// Strip leading `## Assistant` and trailing `## User` headings from append-mode
/// response text.
///
/// The append writer adds its own `## Assistant` prefix and `## User` suffix, so
/// echoed transcript headings are removed before the response is persisted.
pub fn strip_assistant_heading(response: &str) -> String {
    let mut result = response.to_string();

    let trimmed = result.trim_start();
    if let Some(rest) = trimmed.strip_prefix("## Assistant") {
        let rest = rest.strip_prefix('\n').unwrap_or(rest);
        let rest = rest.trim_start_matches('\n');
        result = rest.to_string();
    }

    let trimmed_end = result.trim_end();
    if let Some(before) = trimmed_end.strip_suffix("## User") {
        result = before.trim_end_matches('\n').to_string();
        if !result.ends_with('\n') {
            result.push('\n');
        }
    }

    result
}

/// Separator between the model short name and the response timestamp inside a
/// `### Re:` heading's attribution segment (`#timestampresponseheader`).
///
/// Deliberately NOT another spaced em dash. Four separate parsers read this
/// heading, and they do not agree on which dash bounds the attribution: three
/// take the FIRST (`response_prompt_target_from_re_heading` here,
/// `agent_doc_queue::queue_response::response_heading_topic`, and
/// `agent_doc_turn::response_replay::normalize_replay_topic`) while
/// `agent_doc_document::transient_markers::strip_re_heading_attribution` takes the
/// LAST, via `rfind(" — ")`. A second em dash would therefore leave the model name
/// behind on the strip path, and since that strip backs
/// `normalize_post_commit_re_heading_drift` — the post-commit comparison that lets
/// an attributed heading match an unattributed one — the residue would read as
/// permanent heading drift on every committed response.
///
/// Keeping exactly one ` — ` in the heading preserves all four readings unchanged:
/// the topic is everything before it, the attribution everything after.
pub const RESPONSE_ATTRIBUTION_SEPARATOR: &str = " · ";

/// The spaced em dash that separates a response topic from its attribution.
pub const RESPONSE_TOPIC_ATTRIBUTION_SEPARATOR: &str = " \u{2014} ";

/// Split a `### Re:` heading's attribution segment into model and timestamp.
///
/// Returns `None` when the heading carries no attribution at all. The timestamp is
/// `None` for a pre-timestamp heading, which stays valid.
pub fn response_heading_model_and_timestamp(line: &str) -> Option<(&str, Option<&str>)> {
    let trimmed = line.trim().trim_start_matches('❯').trim();
    let without_hashes = trimmed.trim_start_matches('#').trim_start();
    let rest = without_hashes.strip_prefix("Re:")?.trim();
    let (_, attribution) = rest.split_once(RESPONSE_TOPIC_ATTRIBUTION_SEPARATOR)?;
    let attribution = attribution.trim();
    match attribution.split_once(RESPONSE_ATTRIBUTION_SEPARATOR) {
        Some((model, timestamp)) => Some((model.trim(), Some(timestamp.trim()))),
        None => Some((attribution, None)),
    }
}

/// Whether a response timestamp matches the documented
/// `YYYY-MM-DDTHH:MM±HH:MM` shape (`#timestampresponseheader`).
///
/// A shape check, not a calendar check: it exists so a guard can tell a real
/// timestamp from prose that happened to land in the attribution slot, without
/// pulling a date library into a pure text module.
pub fn response_heading_timestamp_is_wellformed(timestamp: &str) -> bool {
    let bytes = timestamp.as_bytes();
    if bytes.len() != 22 {
        return false;
    }
    let digits_at =
        |positions: &[usize]| positions.iter().all(|index| bytes[*index].is_ascii_digit());
    digits_at(&[0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18, 20, 21])
        && bytes[4] == b'-'
        && bytes[7] == b'-'
        && bytes[10] == b'T'
        && bytes[13] == b':'
        && matches!(bytes[16], b'+' | b'-')
        && bytes[19] == b':'
}

/// Extract the session prompt target from the first `### Re:`-style response
/// heading, dropping the model suffix after a dash separator.
pub fn response_prompt_target_from_re_heading(response_body: &str) -> Option<String> {
    for line in response_body.lines() {
        let without_hashes = line.trim().trim_start_matches('#').trim_start();
        let Some(rest) = without_hashes.strip_prefix("Re:") else {
            continue;
        };
        let target = rest
            .split_once(" \u{2014} ")
            .map(|(target, _)| target)
            .or_else(|| rest.split_once(" - ").map(|(target, _)| target))
            .unwrap_or(rest)
            .trim();
        if !target.is_empty() {
            return Some(target.to_string());
        }
    }
    None
}

/// `#queuetypingsteer`: the labels session-check puts before realtime operator
/// steering (`prompt_bearing_route::prompt_bearing_route_context_from_change`).
/// `content_edit` is the operator typing inside an existing queue item or
/// exchange prompt, which is steering exactly as much as a new `prompt_target`.
const STEERING_MARKERS: [&str; 2] = ["prompt_target:", "content_edit:"];

fn first_steering_marker(reason: &str) -> Option<(usize, &'static str)> {
    STEERING_MARKERS
        .iter()
        .filter_map(|marker| reason.find(marker).map(|at| (at, *marker)))
        .min_by_key(|(at, _)| *at)
}

/// `#steerinterruptexit`: the line `session-check` prints for operator
/// steering that arrived after a committed cycle. It is a status, not a
/// failure: exit `0`, and every consumer (Stop hooks, the Codex final gate,
/// closeout reports, the dogfood terminal-issue notice, queue continuation)
/// keys on this prefix rather than on `INTERRUPTED`.
pub const SESSION_CHECK_STEERING_PENDING_PREFIX: &str = "[session-check] steering pending:";

/// Whether `message` is a `session-check` pending-steering report (anywhere in
/// a multi-line diagnostic, so a wrapped error chain still classifies).
pub fn is_session_check_steering_pending(message: &str) -> bool {
    message.contains(SESSION_CHECK_STEERING_PENDING_PREFIX)
}

/// A committed cycle followed by unanswered operator steering. Matches both
/// the `steering pending` status and the legacy `INTERRUPTED` wording a
/// pre-`#steerinterruptexit` binary prints.
pub fn is_committed_prompt_diff_interruption(reason: &str) -> bool {
    reason.contains("is `committed`")
        && first_steering_marker(reason).is_some()
        && (reason.contains("unresolved prompt-bearing user changes")
            || reason.contains(
                "active harness session changed this document after the last committed closeout",
            ))
        && (reason.contains("no new agent-doc cycle started")
            || reason.contains("without reopening the binary-owned write/commit path"))
}

pub fn prompt_target_from_interruption_reason(reason: &str) -> Option<String> {
    let (at, marker) = first_steering_marker(reason)?;
    let tail = reason[at + marker.len()..].trim();
    (!tail.is_empty()).then(|| tail.to_string())
}

pub fn first_nonempty_prompt_line(prompt: &str) -> String {
    prompt
        .lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or(prompt)
        .trim()
        .to_string()
}

/// Summarize captured response text for external hook consumers. Agent-doc patch
/// comments are metadata, not useful response content, so they are removed
/// before the summary is bounded.
pub fn summarize_response_for_hook(response_body: &str) -> String {
    let lines = response_body
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            trimmed != "<!-- patch:exchange -->"
                && trimmed != "<!-- /patch:exchange -->"
                && !(trimmed.starts_with("<!--") && trimmed.ends_with("-->"))
        })
        .collect::<Vec<_>>()
        .join("\n");
    truncate_response_summary(lines.trim(), 4000)
}

pub fn render_interleaved_thinking_response(thinking: &str, response: &str) -> String {
    if thinking.is_empty() {
        return response.to_string();
    }

    format!(
        "<details>\n<summary>Thinking</summary>\n\n{}\n</details>\n\n{}",
        thinking, response
    )
}

fn truncate_response_summary(input: &str, max_chars: usize) -> String {
    let mut chars = input.chars();
    let mut output = String::new();
    for _ in 0..max_chars {
        let Some(ch) = chars.next() else {
            return input.to_string();
        };
        output.push(ch);
    }
    if chars.next().is_some() {
        output.push_str("\n[truncated]");
    }
    output
}

/// A conservative response-text signal that a closeout left a concrete action
/// or decision with the operator (`#operatortodo`, GH #314, GH #318).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OperatorTodoResponseSignal {
    ExplicitRequest,
    UnpublishedDraft,
    /// An item of a bolded residual-work list (`**Still stale:**`,
    /// `**Not done:**`, ...), GH #318.
    ResidualWork,
}

impl OperatorTodoResponseSignal {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ExplicitRequest => "explicit operator request",
            Self::UnpublishedDraft => "unpublished draft path",
            Self::ResidualWork => "residual work list",
        }
    }
}

/// One operator ask a closeout response leaves pending (GH #318).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorTodoAsk {
    pub signal: OperatorTodoResponseSignal,
    /// The response text that matched, exactly as written (a signal phrase
    /// such as `Say \`post\``, or a residual-list label such as `Still stale`).
    pub matched: String,
    /// The ask itself: the list item or line that carries the signal, with any
    /// list marker removed and truncated for display.
    pub text: String,
}

const EXPLICIT_OPERATOR_TODO_SIGNALS: &[&str] = &[
    "say `post`",
    "say 'post'",
    "say \"post\"",
    "say post",
    "reply `post`",
    "reply 'post'",
    "reply \"post\"",
    "reply post",
    "needs your ok",
    "needs your approval",
    "awaits your ok",
    "awaits your approval",
    "waits for your go",
    "waiting for your go",
    "waits for your approval",
    "waiting for your approval",
    "should i post",
    "shall i post",
    "should i publish",
    "shall i publish",
];

const UNPUBLISHED_DRAFT_SIGNALS: &[&str] = &[
    "unpublished",
    "not posted",
    "not published",
    "not yet posted",
    "not yet published",
    "has not been posted",
    "has not been published",
    "hasn't been posted",
    "hasn't been published",
];

/// Bolded labels that introduce a list of residual operator work (GH #318).
/// Matched against the whole bold span, case-insensitively, as a word prefix:
/// `**Also stale, not drafted:**` is an `also stale` list.
const RESIDUAL_WORK_LABELS: &[&str] = &[
    "still stale",
    "also stale",
    "not done",
    "still open",
    "open",
    "remaining",
];

const OPERATOR_TODO_ASK_TEXT_MAX_CHARS: usize = 160;

/// Detect only high-confidence operator actions. General future-looking prose is
/// intentionally excluded: it belongs to backlog capture, not `agent:todo`.
/// Returns the first ask's signal; see [`operator_todo_response_asks`].
pub fn operator_todo_response_signal(response: &str) -> Option<OperatorTodoResponseSignal> {
    operator_todo_response_asks(response)
        .first()
        .map(|ask| ask.signal)
}

/// Every operator ask a closeout response leaves pending (GH #314, GH #318),
/// in response order:
///
/// - an explicit post/publish decision request (`say \`post\``, `needs your
///   ok`, ...), one ask per line;
/// - an unpublished/not-posted draft whose paragraph names a Markdown path;
/// - each top-level item of a list introduced by a bolded residual-work label
///   (`**Still stale:**`, `**Also stale:**`, `**Not done:**`, `**Still
///   open:**`, `**Open:**`, `**Remaining:**`); a label with inline text and no
///   list is one ask.
///
/// Signals match on word boundaries, so `reply posted` is not `reply post`.
/// Markdown headings and fenced code never count: a heading names what the
/// response is about, not what the operator still has to do.
pub fn operator_todo_response_asks(response: &str) -> Vec<OperatorTodoAsk> {
    let lines = operator_todo_scan_lines(response);
    let mut asks: Vec<(usize, OperatorTodoAsk)> = Vec::new();
    let mut consumed = vec![false; lines.len()];
    let mut index = 0;
    while index < lines.len() {
        let Some(line) = lines[index] else {
            index += 1;
            continue;
        };
        if let Some((label, inline)) = residual_work_label(line) {
            consumed[index] = true;
            // A label that is itself a list item owns only its nested items:
            // the next sibling item is not part of its list.
            let label_indent = line.len() - line.trim_start().len();
            let min_indent = if strip_list_marker(line.trim_start()).is_some() {
                label_indent + 1
            } else {
                0
            };
            let items = residual_work_list_items(&lines, index + 1, min_indent);
            if items.is_empty() {
                if !is_empty_residual_text(inline) {
                    asks.push((
                        index,
                        OperatorTodoAsk {
                            signal: OperatorTodoResponseSignal::ResidualWork,
                            matched: label.to_string(),
                            text: operator_todo_ask_text(inline),
                        },
                    ));
                }
            } else {
                for (item_index, item) in items {
                    consumed[item_index] = true;
                    if is_empty_residual_text(item) {
                        continue;
                    }
                    asks.push((
                        item_index,
                        OperatorTodoAsk {
                            signal: OperatorTodoResponseSignal::ResidualWork,
                            matched: label.to_string(),
                            text: operator_todo_ask_text(item),
                        },
                    ));
                }
            }
        }
        index += 1;
    }
    for (index, line) in lines.iter().enumerate() {
        let Some(line) = line else { continue };
        if consumed[index] {
            continue;
        }
        let lower = line.to_ascii_lowercase();
        if let Some((start, len)) = find_word_bounded_signal(&lower, EXPLICIT_OPERATOR_TODO_SIGNALS)
        {
            asks.push((
                index,
                OperatorTodoAsk {
                    signal: OperatorTodoResponseSignal::ExplicitRequest,
                    matched: line[start..start + len].to_string(),
                    text: operator_todo_ask_text(line),
                },
            ));
            consumed[index] = true;
            continue;
        }
        if let Some((start, len)) = find_word_bounded_signal(&lower, UNPUBLISHED_DRAFT_SIGNALS)
            && lower.contains("draft")
            && paragraph_mentions_markdown_path(&lines, index)
        {
            asks.push((
                index,
                OperatorTodoAsk {
                    signal: OperatorTodoResponseSignal::UnpublishedDraft,
                    matched: line[start..start + len].to_string(),
                    text: operator_todo_ask_text(line),
                },
            ));
            consumed[index] = true;
        }
    }
    // Response order: residual lists were collected in a first pass.
    asks.sort_by_key(|(index, _)| *index);
    asks.into_iter().map(|(_, ask)| ask).collect()
}

/// The response's lines, with headings and fenced code replaced by `None`, so
/// every operator-todo matcher skips them by construction.
fn operator_todo_scan_lines(response: &str) -> Vec<Option<&str>> {
    let mut fence: Option<&str> = None;
    response
        .lines()
        .map(|raw| {
            let line = raw.strip_suffix('\r').unwrap_or(raw);
            let indent = line.len() - line.trim_start_matches(' ').len();
            let trimmed = line.trim_start();
            if indent <= 3 {
                let marker = if trimmed.starts_with("```") {
                    Some("```")
                } else if trimmed.starts_with("~~~") {
                    Some("~~~")
                } else {
                    None
                };
                if let Some(marker) = marker {
                    match fence {
                        Some(open) if open == marker => fence = None,
                        None => fence = Some(marker),
                        Some(_) => {}
                    }
                    return None;
                }
            }
            if fence.is_some() || (indent <= 3 && is_atx_heading(trimmed)) {
                return None;
            }
            Some(line)
        })
        .collect()
}

fn is_atx_heading(trimmed: &str) -> bool {
    let hashes = trimmed.bytes().take_while(|byte| *byte == b'#').count();
    (1..=6).contains(&hashes)
        && trimmed[hashes..]
            .chars()
            .next()
            .is_none_or(|ch| ch == ' ' || ch == '\t')
}

/// Strip one leading list marker (`- `, `* `, `+ `, `1. `, `1) `).
fn strip_list_marker(trimmed: &str) -> Option<&str> {
    for bullet in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(bullet) {
            return Some(rest);
        }
    }
    let digits = trimmed.bytes().take_while(u8::is_ascii_digit).count();
    if (1..=9).contains(&digits) {
        let rest = &trimmed[digits..];
        if let Some(rest) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some(rest);
        }
    }
    None
}

/// A line that opens with a bolded residual-work label, optionally after a
/// list marker: returns the label as written and the text after it.
fn residual_work_label(line: &str) -> Option<(&str, &str)> {
    let trimmed = line.trim_start();
    let trimmed = strip_list_marker(trimmed).unwrap_or(trimmed);
    let (open, rest) = if let Some(rest) = trimmed.strip_prefix("**") {
        ("**", rest)
    } else {
        ("__", trimmed.strip_prefix("__")?)
    };
    let close = rest.find(open)?;
    let span = rest[..close].trim();
    let after = rest[close + open.len()..].trim_start();
    // A label ends with a colon, inside the bold span or right after it.
    let after = if span.ends_with(':') {
        after
    } else {
        after.strip_prefix(':')?
    }
    .trim();
    let label_body = span.trim_end_matches([':', ' ']);
    let lower = label_body.to_ascii_lowercase();
    for label in RESIDUAL_WORK_LABELS {
        if let Some(tail) = lower.strip_prefix(label)
            && residual_work_label_tail(tail)
        {
            return Some((&label_body[..label.len()], after));
        }
    }
    None
}

/// What may follow a residual-work label inside the bold span: nothing, a
/// qualifier after a comma or dash (`Also stale, not drafted`), or a plain
/// noun for the work itself (`Open questions`). `Remaining budget`,
/// `Opened PR #12`, and `Open-source tools` are not residual-work labels.
fn residual_work_label_tail(tail: &str) -> bool {
    const NOUNS: &[&str] = &[
        "work",
        "items",
        "questions",
        "follow-ups",
        "followups",
        "asks",
        "actions",
        "decisions",
        "operator actions",
        "operator items",
    ];
    tail.is_empty()
        || tail.starts_with(',')
        || tail.starts_with(" (")
        || tail.starts_with(" \u{2014}")
        || tail.starts_with(" - ")
        || tail
            .strip_prefix(' ')
            .is_some_and(|noun| NOUNS.contains(&noun))
}

/// The top-level list items that directly follow a residual-work label, with
/// their line indices. One blank line may separate the label from its list;
/// the list ends at the first blank line, the first non-item line at its own
/// indent, or the first line indented less than `min_indent`.
fn residual_work_list_items<'a>(
    lines: &[Option<&'a str>],
    start: usize,
    min_indent: usize,
) -> Vec<(usize, &'a str)> {
    let mut index = start;
    if lines
        .get(index)
        .is_some_and(|line| line.is_some_and(|line| line.trim().is_empty()))
    {
        index += 1;
    }
    let mut items = Vec::new();
    let mut item_indent: Option<usize> = None;
    while let Some(Some(line)) = lines.get(index) {
        if line.trim().is_empty() {
            break;
        }
        let indent = line.len() - line.trim_start().len();
        if indent < min_indent {
            break;
        }
        match strip_list_marker(line.trim_start()) {
            Some(item) if item_indent.is_none_or(|top| indent == top) => {
                item_indent = Some(indent);
                items.push((index, item));
            }
            // Nested items and continuation lines belong to the item above.
            _ if item_indent.is_some_and(|top| indent > top) => {}
            _ => break,
        }
        index += 1;
    }
    items
}

fn paragraph_mentions_markdown_path(lines: &[Option<&str>], index: usize) -> bool {
    let in_paragraph = |line: &Option<&str>| line.is_some_and(|line| !line.trim().is_empty());
    let mut first = index;
    while first > 0 && in_paragraph(&lines[first - 1]) {
        first -= 1;
    }
    let mut last = index;
    while last + 1 < lines.len() && in_paragraph(&lines[last + 1]) {
        last += 1;
    }
    lines[first..=last]
        .iter()
        .flatten()
        .any(|line| line.to_ascii_lowercase().contains(".md"))
}

/// `**Remaining:** none.` / `- Nothing.` report an empty list, not an ask.
fn is_empty_residual_text(text: &str) -> bool {
    let lower = text
        .trim()
        .trim_matches(|ch: char| matches!(ch, '.' | '*' | '_' | '`' | '(' | ')' | ' '))
        .to_ascii_lowercase();
    lower.is_empty()
        || matches!(lower.as_str(), "n/a" | "na" | "-" | "\u{2014}" | "no")
        || find_word_bounded_signal(&lower, &["none", "nothing"])
            .is_some_and(|(start, _)| start == 0)
}

fn operator_todo_ask_text(line: &str) -> String {
    let trimmed = line.trim();
    let text = strip_list_marker(trimmed).unwrap_or(trimmed).trim();
    let mut out: String = text
        .chars()
        .take(OPERATOR_TODO_ASK_TEXT_MAX_CHARS)
        .collect();
    if text.chars().count() > OPERATOR_TODO_ASK_TEXT_MAX_CHARS {
        out.push_str("...");
    }
    out
}

/// The first `signals` entry that occurs in `haystack` on word boundaries
/// (GH #318): the characters on either side of the match must not be ASCII
/// alphanumeric, so `reply post` does not match inside `reply posted`.
/// Returns the byte range of the match.
fn find_word_bounded_signal(haystack: &str, signals: &[&str]) -> Option<(usize, usize)> {
    let bytes = haystack.as_bytes();
    let is_word = |byte: u8| byte.is_ascii_alphanumeric() || byte == b'_';
    let mut best: Option<(usize, usize)> = None;
    for signal in signals {
        let mut from = 0;
        while let Some(offset) = haystack[from..].find(signal) {
            let start = from + offset;
            let end = start + signal.len();
            let left_ok = start == 0 || !is_word(bytes[start - 1]);
            let right_ok = end == bytes.len() || !is_word(bytes[end]);
            if left_ok && right_ok {
                if best.is_none_or(|(best_start, _)| start < best_start) {
                    best = Some((start, signal.len()));
                }
                break;
            }
            from = start + 1;
            while from < haystack.len() && !haystack.is_char_boundary(from) {
                from += 1;
            }
        }
    }
    best
}

/// The outcome of comparing a response's operator asks with the todos that
/// record them (GH #318). Present only when the advisory should fire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorTodoAudit {
    /// Every operator ask the response leaves pending.
    pub asks: usize,
    /// The number of `--todo-add` items this closeout supplied.
    pub todo_adds: usize,
    /// The asks that no supplied `--todo-add` and no open `agent:todo` item
    /// matches by content.
    pub unrecorded: Vec<OperatorTodoAsk>,
}

/// Compare the response's operator asks with this closeout's `--todo-add`
/// texts and the document's open `agent:todo` actions (GH #318).
///
/// An ask is covered when at least half of its content words (and at least
/// one) appear in one todo. A `--todo-add` that covers no ask is assumed to
/// record one in other words, so it absorbs one unrecorded ask. The advisory
/// fires (`Some`) when more unrecorded asks remain than such unmatched adds:
/// four asks and two adds still fire, naming the asks left out, while one
/// `--todo-add` no longer silences a whole residual-work list.
pub fn audit_operator_todo_response(
    response: &str,
    todo_adds: &[String],
    open_todo_actions: &[String],
) -> Option<OperatorTodoAudit> {
    let asks = operator_todo_response_asks(response);
    if asks.is_empty() {
        return None;
    }
    let add_words: Vec<Vec<String>> = todo_adds.iter().map(|add| content_words(add)).collect();
    let open_words: Vec<Vec<String>> = open_todo_actions
        .iter()
        .map(|action| content_words(action))
        .collect();
    let mut add_matched = vec![false; todo_adds.len()];
    let mut unrecorded = Vec::new();
    for ask in &asks {
        let words = content_words(&ask.text);
        let mut covered = false;
        for (add_index, add) in add_words.iter().enumerate() {
            if todo_covers_ask(&words, add) {
                add_matched[add_index] = true;
                covered = true;
            }
        }
        if !covered && !open_words.iter().any(|open| todo_covers_ask(&words, open)) {
            unrecorded.push(ask.clone());
        }
    }
    let unmatched_adds = add_matched.iter().filter(|matched| !**matched).count();
    let fires = unrecorded.len() > unmatched_adds;
    fires.then_some(OperatorTodoAudit {
        asks: asks.len(),
        todo_adds: todo_adds.len(),
        unrecorded,
    })
}

const CONTENT_WORD_STOPWORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "you",
    "your",
    "yours",
    "not",
    "but",
    "are",
    "was",
    "has",
    "have",
    "had",
    "its",
    "can",
    "now",
    "say",
    "any",
    "all",
    "our",
    "out",
    "with",
    "from",
    "that",
    "this",
    "these",
    "those",
    "been",
    "still",
    "also",
    "after",
    "before",
    "over",
    "into",
    "they",
    "them",
    "then",
    "than",
    "when",
    "what",
    "which",
    "will",
    "would",
    "should",
    "could",
    "about",
    "there",
    "their",
    "some",
    "only",
    "more",
    "most",
    "each",
    "does",
    "done",
    "just",
    "once",
    "here",
    "were",
    "where",
    "while",
    "again",
    "need",
    "needs",
    "longer",
    "makes",
    "make",
    "stale",
    "open",
    "remaining",
];

fn content_words(text: &str) -> Vec<String> {
    let mut words: Vec<String> = Vec::new();
    for word in text
        .to_ascii_lowercase()
        .split(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'))
        .map(|word| word.trim_matches(['-', '_']))
    {
        if word.len() >= 3
            && !CONTENT_WORD_STOPWORDS.contains(&word)
            && !words.iter().any(|seen| seen == word)
        {
            words.push(word.to_string());
        }
    }
    words
}

fn todo_covers_ask(ask_words: &[String], todo_words: &[String]) -> bool {
    if ask_words.is_empty() {
        return false;
    }
    let shared = ask_words
        .iter()
        .filter(|word| todo_words.contains(word))
        .count();
    shared >= 1 && shared * 2 >= ask_words.len()
}

/// A blocking `[#subagent-operator-request-<unique-id>]` packet a closeout response presents
/// to the operator (`#packettodo`, `#wayposttodo`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockingOperatorRequestPacket {
    /// The packet's request-specific tracked id.
    pub id: String,
    /// `question` or `permission`.
    pub kind: String,
    /// The packet's `request:` line, trimmed.
    pub request: String,
    /// The dispatch-time queue item this request blocks. New worker prompts
    /// provide this exact value so queue-local visibility can link back to the
    /// packet and todo without guessing from prose.
    pub queue_item: Option<String>,
    /// The plain-text label of every `- **<label>**: <effect>` choice line, in order.
    /// Legacy unbolded choice labels remain readable.
    pub choice_labels: Vec<String>,
}

const OPERATOR_REQUEST_TODO_SUMMARY_MAX_CHARS: usize = 200;
const OPERATOR_REQUEST_TODO_CHOICE_MAX_CHARS: usize = 40;

/// Every blocking `[#subagent-operator-request-<unique-id>]` packet in `response`, parsed
/// structurally (`#packettodo`).
///
/// A packet is a request-specific marker line (at most three leading spaces,
/// so an indented code block never counts) followed, without a blank line, by
/// its field lines. The canonical packet is a nested list: the marker is the
/// top-level bullet (`- [#subagent-operator-request-<id>] subagent operator
/// request`), each field is one `- *key:* value` bullet nested beneath it, and
/// context lines and choices are nested one level deeper. The historical flat
/// packet (a bare marker line followed by top-level `- *key:* value` or plain
/// `key: value` fields, choices nested beneath `choices`) remains readable.
///
/// The indentation of the first field line is the field level. A line
/// indented deeper belongs to the field above it: under `choices` the
/// shallowest such bullets are the choices (deeper lines are choice detail),
/// under `request` they continue an otherwise empty request, and under any
/// other field (such as `context`) they are prose that can never be read as a
/// field. A line indented shallower than the field level ends the packet.
///
/// It counts only when it carries `kind: question` or `kind: permission`,
/// `blocking: true`, and a concrete `request:` (not the `<...>` protocol
/// placeholder). Lines inside fenced code blocks and blockquoted (`>`) lines
/// are quoted prose, never a packet.
pub fn blocking_operator_request_packets(response: &str) -> Vec<BlockingOperatorRequestPacket> {
    let lines: Vec<&str> = response.lines().collect();
    let mut packets = Vec::new();
    let mut fence: Option<(char, usize)> = None;
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        let trimmed_start = line.trim_start();
        if let Some(open) = fence {
            if is_closing_fence(trimmed_start, open) {
                fence = None;
            }
            continue;
        }
        if let Some(open) = opening_fence(trimmed_start) {
            fence = Some(open);
            continue;
        }
        let indent = line.len() - trimmed_start.len();
        let Some(packet_id) = (indent <= 3)
            .then(|| agent_doc_queue::subagent_brief::subagent_operator_request_id(line))
            .flatten()
        else {
            continue;
        };
        let mut kind = None;
        let mut blocking = None;
        let mut request: Option<String> = None;
        let mut queue_item = None;
        let mut choice_labels = Vec::new();
        let mut field_indent: Option<usize> = None;
        let mut current_key = String::new();
        let mut choice_indent: Option<usize> = None;
        while index < lines.len() {
            let raw = lines[index];
            let field = raw.trim();
            if field.is_empty()
                || agent_doc_queue::subagent_brief::subagent_operator_request_id(field).is_some()
            {
                break;
            }
            let line_indent = raw.len() - raw.trim_start().len();
            let level = *field_indent.get_or_insert(line_indent);
            if line_indent < level {
                break;
            }
            index += 1;
            if line_indent > level || (current_key == "choices" && is_legacy_choice_line(field)) {
                // Nested beneath the field above (or, for a historical flat
                // packet, a choice bullet written at the field level).
                match current_key.as_str() {
                    "choices" => {
                        let shallowest = *choice_indent.get_or_insert(line_indent);
                        if line_indent <= shallowest
                            && let Some(label) = operator_request_choice_label(field)
                        {
                            choice_labels.push(label.to_string());
                        }
                    }
                    "request" => {
                        if let Some(request) = request.as_mut() {
                            let text = field.strip_prefix("- ").unwrap_or(field).trim();
                            if !text.is_empty() {
                                if !request.is_empty() {
                                    request.push(' ');
                                }
                                request.push_str(text);
                            }
                        }
                    }
                    _ => {}
                }
                continue;
            }
            current_key.clear();
            choice_indent = None;
            let Some((key, value)) = operator_request_field(field) else {
                continue;
            };
            let key = key.to_ascii_lowercase();
            match key.as_str() {
                "kind" => kind = Some(value.to_ascii_lowercase()),
                "blocking" => blocking = Some(value.eq_ignore_ascii_case("true")),
                "request" => request = Some(value.to_string()),
                "queue_item" => queue_item = (!value.is_empty()).then(|| value.to_string()),
                _ => {}
            }
            // Only an empty field opens nested lines (`- *choices:* []` has none),
            // so detail nested under an inline request never rewrites it.
            if value.is_empty() {
                current_key = key;
            }
        }
        let (Some(kind), Some(true), Some(request)) = (kind, blocking, request) else {
            continue;
        };
        if !matches!(kind.as_str(), "question" | "permission")
            || request.is_empty()
            || (request.starts_with('<') && request.ends_with('>'))
        {
            continue;
        }
        packets.push(BlockingOperatorRequestPacket {
            id: packet_id.to_string(),
            kind,
            request,
            queue_item,
            choice_labels,
        });
    }
    packets
}

/// A historical flat packet wrote its choices as bullets at the field level
/// directly after a plain `choices:` line. A bullet that parses as a known
/// packet field is a field, never a choice.
fn is_legacy_choice_line(field: &str) -> bool {
    field.starts_with("- ")
        && !operator_request_field(field).is_some_and(|(key, _)| {
            matches!(
                key.to_ascii_lowercase().as_str(),
                "queue_item" | "kind" | "blocking" | "request" | "context" | "choices"
            )
        })
}

/// The plain-text label of a `- **<label>**: <effect>` choice bullet. Legacy
/// unbolded labels remain readable.
fn operator_request_choice_label(field: &str) -> Option<&str> {
    let choice = field.strip_prefix("- ")?;
    let label = choice
        .split_once(':')
        .map_or(choice, |(label, _)| label)
        .trim();
    let label = label
        .strip_prefix("**")
        .and_then(|label| label.strip_suffix("**"))
        .unwrap_or(label)
        .trim();
    (!label.is_empty()).then_some(label)
}

/// Parse a request field after removing the canonical list marker and emphasis.
/// Plain historical fields remain readable, as do the common colon-outside-
/// emphasis and bold-key variants produced by hand-edited packets.
fn operator_request_field(field: &str) -> Option<(&str, &str)> {
    let field = field.trim().strip_prefix("- ").unwrap_or(field.trim());
    for (prefix, delimiter) in [("**", ":**"), ("**", "**:"), ("*", ":*"), ("*", "*:")] {
        if let Some(emphasized) = field.strip_prefix(prefix)
            && let Some((key, value)) = emphasized.split_once(delimiter)
        {
            return Some((key.trim(), value.trim()));
        }
    }
    let (key, value) = field.split_once(':')?;
    Some((key.trim(), value.trim()))
}

/// The first request id reused by more than one blocking packet, if any.
pub fn duplicate_blocking_operator_request_id(
    packets: &[BlockingOperatorRequestPacket],
) -> Option<&str> {
    let mut seen = std::collections::HashSet::new();
    packets
        .iter()
        .map(|packet| packet.id.as_str())
        .find(|id| !seen.insert(*id))
}

fn opening_fence(trimmed_start: &str) -> Option<(char, usize)> {
    let ch = trimmed_start.chars().next()?;
    if ch != '`' && ch != '~' {
        return None;
    }
    let run = trimmed_start.chars().take_while(|c| *c == ch).count();
    (run >= 3).then_some((ch, run))
}

fn is_closing_fence(trimmed_start: &str, (ch, run): (char, usize)) -> bool {
    let closing = trimmed_start.chars().take_while(|c| *c == ch).count();
    closing >= run && trimmed_start[closing * ch.len_utf8()..].trim().is_empty()
}

/// The deterministic `agent:todo` action for a blocking operator request packet
/// (`#packettodo`). It names the packet's place (`agent:exchange`), the first
/// sentence of its `request:` line, and the offered choice labels, so the todo
/// is understandable by itself and points at the full packet. Its explicit id
/// is the packet id, so `agent:exchange`, `agent:todo`, and the queue's
/// `🙋 [#id]` annotation share one stable identity. `tracked_id` is descriptive
/// context only; it deliberately is not rendered as a `#id` citation because
/// todo id layering would otherwise replace the request identity.
pub fn blocking_operator_request_todo_text(
    packet: &BlockingOperatorRequestPacket,
    tracked_id: Option<&str>,
) -> String {
    let subject = if packet.kind == "permission" {
        "permission request"
    } else {
        "question"
    };
    let target = tracked_id
        .map(|id| format!(" for tracked work {id}"))
        .unwrap_or_default();
    let summary = truncate_on_word(
        first_sentence(&packet.request),
        OPERATOR_REQUEST_TODO_SUMMARY_MAX_CHARS,
    );
    let queue_target = packet
        .queue_item
        .as_deref()
        .map(|item| {
            format!(
                " for queue item ‘{}’",
                truncate_on_word(item, OPERATOR_REQUEST_TODO_SUMMARY_MAX_CHARS)
            )
        })
        .unwrap_or_default();
    let mut text = format!(
        "id={} Answer the blocking subagent {subject}{target}{queue_target} in agent:exchange: {summary}",
        packet.id
    );
    if !packet.choice_labels.is_empty() {
        let labels = packet
            .choice_labels
            .iter()
            .map(|label| truncate_on_word(label, OPERATOR_REQUEST_TODO_CHOICE_MAX_CHARS))
            .collect::<Vec<_>>()
            .join(", ");
        text.push_str(&format!(" (choices: {labels})"));
    }
    text
}

fn first_sentence(text: &str) -> &str {
    let text = text.trim();
    for (at, ch) in text.char_indices() {
        if matches!(ch, '?' | '.' | '!') {
            let end = at + ch.len_utf8();
            if text[end..].is_empty() || text[end..].starts_with(char::is_whitespace) {
                return &text[..end];
            }
        }
    }
    text
}

fn truncate_on_word(text: &str, max_chars: usize) -> String {
    if text.chars().count() <= max_chars {
        return text.to_string();
    }
    let cut = text
        .char_indices()
        .nth(max_chars)
        .map_or(text.len(), |(at, _)| at);
    let head = &text[..cut];
    let head = head
        .rfind(char::is_whitespace)
        .filter(|at| *at > 0)
        .map_or(head, |at| &head[..at]);
    format!("{}…", head.trim_end())
}

/// Letter-led tracked-work ids a response cites as `#id` / `[#id]`, lowercase,
/// distinct, in first-seen order (`#packettodo`). Packet marker ids in the
/// reserved `subagent-operator-request-*` family are not citations.
pub fn cited_tracked_ids_in_response(response: &str) -> Vec<String> {
    let mut ids: Vec<String> = Vec::new();
    for (start, _) in response.match_indices('#') {
        let preceding = response[..start].chars().next_back();
        if preceding.is_some_and(|c| c.is_ascii_alphanumeric() || c == '&') {
            continue;
        }
        let id: String = response[start + 1..]
            .chars()
            .take_while(|c| c.is_ascii_alphanumeric() || *c == '-' || *c == '_')
            .collect::<String>()
            .to_ascii_lowercase();
        if !id.starts_with(|c: char| c.is_ascii_alphabetic())
            || id == "subagent-operator-request"
            || id.starts_with(agent_doc_queue::subagent_brief::SUBAGENT_OPERATOR_REQUEST_ID_PREFIX)
            || ids.contains(&id)
        {
            continue;
        }
        ids.push(id);
    }
    ids
}

const IMPERATIVE_STATUS_ONLY_SIGNALS: &[&str] = &[
    "in progress",
    "continuing",
    "starting",
    "working on it",
    "still working",
    "next i'll",
    "next i will",
    "i'll update",
    "i will update",
    "i'm going to",
    "i am going to",
    "let me do that",
];

const IMPERATIVE_META_REFUSAL_SIGNALS: &[&str] = &[
    "because you asked me to run agent-doc",
    "treated that text as document content",
    "not to execute",
    "say do #",
    "repeat the instruction in chat",
    "i stayed on the first layer",
    "operate on the session document",
];

const IMPERATIVE_BLOCKER_SIGNALS: &[&str] = &[
    "blocked",
    "blocker",
    "failed",
    "error",
    "cannot",
    "can't",
    "unable",
    "missing",
    "permission denied",
    "requires approval",
    "needs approval",
    "lock file",
    "timed out",
];

const IMPERATIVE_EVIDENCE_LABELS: &[&str] = &[
    "what changed:",
    "verification:",
    "commit / push:",
    "outcome:",
    "root cause:",
    "blocked:",
    "blocker:",
];

/// Decide whether a response satisfies an executable imperative directive.
///
/// This is the pure response half of the binary backstop. Diff extraction,
/// ops-log emission, and fail-closed error formatting stay in orchestration.
pub fn response_satisfies_imperative_contract(response: &str) -> bool {
    let lower = response.to_ascii_lowercase();
    if contains_any_signal(&lower, IMPERATIVE_BLOCKER_SIGNALS) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_META_REFUSAL_SIGNALS) {
        return false;
    }
    if contains_execution_evidence(response, &lower) {
        return true;
    }
    if contains_any_signal(&lower, IMPERATIVE_STATUS_ONLY_SIGNALS) {
        return false;
    }
    false
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImperativeResponseContractDecision {
    NoImperativeDirective,
    Satisfied,
    Rejected { trigger: String },
}

/// Decide whether an imperative document directive has been concretely handled.
///
/// The diff and response-text classification are pure turn policy. File/snapshot
/// loading, ops-log emission, and returning a process error stay in orchestration.
pub fn imperative_response_contract_decision(
    diff_text: &str,
    response: &str,
) -> ImperativeResponseContractDecision {
    if !agent_doc_diff::diff_contains_imperative_directive(diff_text) {
        return ImperativeResponseContractDecision::NoImperativeDirective;
    }
    if response_satisfies_imperative_contract(response) {
        return ImperativeResponseContractDecision::Satisfied;
    }
    let trigger = agent_doc_diff::extract_imperative_directives(diff_text)
        .into_iter()
        .next()
        .unwrap_or_else(|| "approval".to_string());
    ImperativeResponseContractDecision::Rejected { trigger }
}

pub fn truncate_imperative_trigger(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        let mut cut = max;
        while cut > 0 && !value.is_char_boundary(cut) {
            cut -= 1;
        }
        format!("{}...", &value[..cut])
    }
}

fn contains_any_signal(haystack: &str, signals: &[&str]) -> bool {
    signals.iter().any(|signal| haystack.contains(signal))
}

fn contains_execution_evidence(response: &str, lower: &str) -> bool {
    if response.contains("```") || response.contains("~~~") {
        return true;
    }
    if IMPERATIVE_EVIDENCE_LABELS
        .iter()
        .any(|label| lower.contains(label))
    {
        return true;
    }
    if lower.contains("implemented and verified")
        || lower.contains("built and installed")
        || lower.contains("added regression coverage")
        || lower.contains("pushed to ")
    {
        return true;
    }
    response.lines().any(|line| {
        has_commandish_backticks(line)
            || has_code_path(line)
            || contains_commit_hash(line)
            || line.trim_start().starts_with("- `")
    })
}

fn has_commandish_backticks(line: &str) -> bool {
    if !line.contains('`') {
        return false;
    }
    let lower = line.to_ascii_lowercase();
    lower.contains("cargo ")
        || lower.contains("git ")
        || lower.contains("make ")
        || lower.contains("npm ")
        || lower.contains("pnpm ")
        || lower.contains("yarn ")
        || lower.contains("pytest")
        || lower.contains("uv run")
        || lower.contains("agent-doc ")
        || line.contains('/')
}

fn has_code_path(line: &str) -> bool {
    let lower = line.to_ascii_lowercase();
    line.contains("src/")
        || line.contains("tests/")
        || line.contains("specs/")
        || line.contains("runbooks/")
        || lower.contains(".rs")
        || lower.contains(".md")
        || lower.contains(".toml")
        || lower.contains(".json")
        || lower.contains(".sh")
        || lower.contains(".kt")
        || lower.contains(".ts")
}

fn contains_commit_hash(line: &str) -> bool {
    let mut run = 0usize;
    for ch in line.chars() {
        if ch.is_ascii_hexdigit() {
            run += 1;
            if run >= 7 {
                return true;
            }
        } else {
            run = 0;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    /// `#timestampresponseheader`: the whole reason the timestamp joins the model with
    /// `·` instead of a second spaced em dash. Four parsers read this heading and they
    /// disagree about which dash bounds the attribution — three take the first, and
    /// `strip_re_heading_attribution` takes the LAST. This drives all four off one
    /// timestamped heading, so a future format change cannot satisfy one and break the
    /// others silently.
    #[test]
    fn a_timestamped_heading_reads_the_same_through_every_parser() {
        let heading = "### Re: do [#fix1] \u{2014} opus-5 \u{00B7} 2026-09-11T23:45-04:00";
        assert_eq!(
            heading,
            "### Re: do [#fix1] \u{2014} opus-5 \u{00B7} 2026-09-11T23:45-04:00"
        );
        assert_eq!(
            heading.matches(" \u{2014} ").count(),
            1,
            "exactly one spaced em dash must separate topic from attribution: {heading}"
        );

        // 1. This module's own parser (first dash).
        assert_eq!(
            super::response_prompt_target_from_re_heading(heading).as_deref(),
            Some("do [#fix1]")
        );
        // 2. The queue-closeout parser (first dash).
        assert_eq!(
            agent_doc_queue::queue_response::response_heading_topic(heading),
            Some("do [#fix1]")
        );
        // 3. The compact digest's topic summarizer.
        assert_eq!(
            agent_doc_topic::summarize_compacted_exchange(&format!("{heading}\nbody\n")),
            vec!["Archived 1 response topic(s): do [#fix1]".to_string()]
        );
        // 4. The post-commit drift strip (LAST dash) must remove model AND timestamp.
        assert_eq!(
            agent_doc_document::transient_markers::strip_re_heading_attribution(&format!(
                "{heading}\n"
            )),
            "### Re: do [#fix1]\n",
            "the attribution strip must not leave the model name behind"
        );
    }

    /// The pre-timestamp heading stays valid — a harness with no clock must degrade to
    /// it rather than emit a dangling separator.
    #[test]
    fn an_untimestamped_heading_still_round_trips() {
        let heading = "### Re: do [#fix1] \u{2014} gpt-5";
        assert_eq!(heading, "### Re: do [#fix1] \u{2014} gpt-5");
        assert_eq!(
            super::response_heading_model_and_timestamp(heading),
            Some(("gpt-5", None))
        );
        assert_eq!(
            agent_doc_document::transient_markers::strip_re_heading_attribution(&format!(
                "{heading}\n"
            )),
            "### Re: do [#fix1]\n"
        );
    }

    #[test]
    fn heading_attribution_splits_into_model_and_timestamp() {
        let heading = "### Re: topic \u{2014} opus-5 \u{00B7} 2026-09-11T23:45-04:00";
        assert_eq!(
            super::response_heading_model_and_timestamp(heading),
            Some(("opus-5", Some("2026-09-11T23:45-04:00")))
        );
        // No attribution at all.
        assert_eq!(
            super::response_heading_model_and_timestamp("### Re: topic"),
            None
        );
        // The active-prompt marker and heading level must not defeat the split.
        assert_eq!(
            super::response_heading_model_and_timestamp(
                "\u{276F} #### Re: topic \u{2014} opus-5 \u{00B7} 2026-09-11T23:45-04:00"
            ),
            Some(("opus-5", Some("2026-09-11T23:45-04:00")))
        );
    }

    /// A shape check, so a guard can tell a timestamp from prose that landed in the
    /// attribution slot. The offset sign sits at index 16 and the offset colon at 19 —
    /// transposing them accepts `...23:45:04-00` and rejects the real format.
    #[test]
    fn timestamp_shape_check_pins_each_separator_position() {
        assert!(super::response_heading_timestamp_is_wellformed(
            "2026-09-11T23:45-04:00"
        ));
        assert!(super::response_heading_timestamp_is_wellformed(
            "2026-09-11T23:45+05:30"
        ));
        for bad in [
            "2026-09-11T23:45:04-00",  // sign/colon transposed
            "2026-09-11T23:45",        // no offset
            "2026-09-11 23:45-04:00",  // space instead of T
            "2026-9-11T23:45-04:00",   // unpadded month
            "2026-09-11T23:45-04:0",   // too short
            "2026-09-11T23:45-04:000", // too long
            "opus-5",
            "",
        ] {
            assert!(
                !super::response_heading_timestamp_is_wellformed(bad),
                "must reject {bad:?}"
            );
        }
    }

    use super::*;

    #[test]
    fn operator_todo_signal_detects_explicit_post_request() {
        assert_eq!(
            operator_todo_response_signal(
                "**Description: draft below, not posted.** Say `post` to apply it."
            ),
            Some(OperatorTodoResponseSignal::ExplicitRequest)
        );
    }

    #[test]
    fn operator_todo_signal_detects_unpublished_markdown_draft_path() {
        assert_eq!(
            operator_todo_response_signal(
                "The unpublished draft is ready at `docs/launch-note.md`."
            ),
            Some(OperatorTodoResponseSignal::UnpublishedDraft)
        );
    }

    #[test]
    fn operator_todo_signal_ignores_completed_or_pathless_draft_prose() {
        assert_eq!(
            operator_todo_response_signal("Published the draft at `docs/launch-note.md`."),
            None
        );
        assert_eq!(
            operator_todo_response_signal("The draft is not posted because CI is still red."),
            None
        );
        assert_eq!(
            operator_todo_response_signal(
                "Implementation is complete; no operator action remains."
            ),
            None
        );
    }

    // GH #318 cycle 1 and cycle 2 closeout responses (anonymized).
    const GH318_CYCLE1: &str = "### Re: review notes \u{2014} opus-5\n\nRewrote the description.\n\n**Todo added: yes**, two items. Say `post` for the reply, and OK the note `n-b` rewrite.\n**Also stale, not drafted:**\n- Your note `n-a` (thread `t-a`) still argues tags over a list.\n- The description's \"Option considered\" section points to note `n-b` for the tags-vs-list argument. After the rewrite, that note no longer makes it.\n";
    const GH318_CYCLE2: &str = "### Re: alice reply posted, note rewritten \u{2014} opus-5\n\nPosted the reply and rewrote the note.\n\n**Still stale:**\n- Your note `n-a` in `t-a` still argues for tags over a list.\n- The description's \"Option considered\" section still says \"Tags vs list was argued in note `n-b`\". The rewritten note no longer makes that argument.\n";

    #[test]
    fn operator_todo_signal_matches_on_word_boundaries_and_skips_headings() {
        // GH #318 failure 3: `reply posted` in the heading matched `reply post`.
        let heading_only = "### Re: alice reply posted, note rewritten \u{2014} opus-5\n\nDone.\n";
        assert_eq!(operator_todo_response_signal(heading_only), None);
        assert_eq!(
            operator_todo_response_signal("The reply posted cleanly; nothing else to do."),
            None
        );
        // A heading never counts, even when it carries a real signal phrase.
        assert_eq!(
            operator_todo_response_signal("## Say `post` to publish\n\nDone."),
            None
        );
        // Fenced code never counts.
        assert_eq!(
            operator_todo_response_signal("```\nsay post\n```\nDone."),
            None
        );
        // The signal itself still matches, and the ask quotes the matched text.
        let asks = operator_todo_response_asks("Draft ready. Say `post` to apply it.");
        assert_eq!(asks.len(), 1);
        assert_eq!(asks[0].signal, OperatorTodoResponseSignal::ExplicitRequest);
        assert_eq!(asks[0].matched, "Say `post`");
        assert_eq!(asks[0].text, "Draft ready. Say `post` to apply it.");
    }

    #[test]
    fn operator_todo_asks_count_each_residual_work_list_item() {
        // GH #318 failure 2: a bolded residual-work list is operator work.
        let asks = operator_todo_response_asks(GH318_CYCLE2);
        assert_eq!(asks.len(), 2, "{asks:?}");
        assert!(
            asks.iter()
                .all(|ask| ask.signal == OperatorTodoResponseSignal::ResidualWork
                    && ask.matched == "Still stale"),
            "{asks:?}"
        );
        assert!(
            asks[0].text.starts_with("Your note `n-a` in `t-a`"),
            "{asks:?}"
        );
        assert!(asks[1].text.starts_with("The description's"), "{asks:?}");

        let cycle1 = operator_todo_response_asks(GH318_CYCLE1);
        let signals: Vec<_> = cycle1
            .iter()
            .map(|ask| (ask.signal, ask.matched.as_str()))
            .collect();
        assert_eq!(
            signals,
            vec![
                (OperatorTodoResponseSignal::ExplicitRequest, "Say `post`"),
                (OperatorTodoResponseSignal::ResidualWork, "Also stale"),
                (OperatorTodoResponseSignal::ResidualWork, "Also stale"),
            ]
        );

        for label in ["Not done", "Still open", "Open", "Remaining", "Also stale"] {
            let response = format!(
                "Done.\n\n**{label}:**\n\n1. Rotate the key.\n2. Re-run the deploy.\n   - nested detail\n\nThat is all.\n"
            );
            let asks = operator_todo_response_asks(&response);
            assert_eq!(asks.len(), 2, "{label}: {asks:?}");
            assert_eq!(asks[0].text, "Rotate the key.");
            assert_eq!(asks[1].text, "Re-run the deploy.");
        }
        // A bulleted label owns its nested items, never its sibling items.
        let bulleted = operator_todo_response_asks(
            "- **Done:** the rewrite.\n- **Remaining:** the changelog entry.\n- **Notes:** none.\n",
        );
        assert_eq!(bulleted.len(), 1, "{bulleted:?}");
        assert_eq!(bulleted[0].text, "the changelog entry.");
        let nested = operator_todo_response_asks(
            "- **Not done:**\n  - Rotate the key.\n  - Re-run the deploy.\n- **Notes:** none.\n",
        );
        assert_eq!(nested.len(), 2, "{nested:?}");
        // Inline label text without a list is one ask.
        let inline = operator_todo_response_asks("**Not done:** the changelog entry.");
        assert_eq!(inline.len(), 1);
        assert_eq!(inline[0].text, "the changelog entry.");
        // Colon outside the bold span, and a noun or qualifier tail.
        for response in [
            "**Open questions**:\n- Which region?\n",
            "- **Remaining work:** the changelog entry.",
            "__Still open (2):__\n- Which region?\n- Which tier?\n",
        ] {
            assert!(
                !operator_todo_response_asks(response).is_empty(),
                "{response:?}"
            );
        }
    }

    #[test]
    fn operator_todo_asks_ignore_empty_or_unrelated_bold_labels() {
        for response in [
            "**Remaining:** none.",
            "**Open:** nothing.",
            "**Still open:**\n- None.\n",
            "**Opened PR #12:** merged.",
            "**Open-source tools:**\n- ripgrep\n",
            "**Remaining budget** is fine.\n\nDone.",
            "**Remaining budget:** 3k tokens.",
            "**Not done yet** \u{2014} see below.",
            "### Still stale\n- an item under a heading, not a bold label\n",
        ] {
            assert_eq!(
                operator_todo_response_asks(response),
                Vec::new(),
                "{response:?}"
            );
        }
    }

    #[test]
    fn operator_todo_audit_counts_asks_against_todo_adds() {
        // GH #318 failure 1: two adds for four operator items used to silence
        // the advisory. The two stale items have no matching todo.
        let adds = vec![
            "Say post for the reply to alice".to_string(),
            "OK the note n-b rewrite".to_string(),
        ];
        let audit = audit_operator_todo_response(GH318_CYCLE1, &adds, &[]).unwrap();
        assert_eq!(audit.asks, 3);
        assert_eq!(audit.todo_adds, 2);
        let unrecorded: Vec<_> = audit
            .unrecorded
            .iter()
            .map(|ask| ask.text.as_str())
            .collect();
        assert!(
            unrecorded
                .iter()
                .any(|text| text.starts_with("Your note `n-a`")),
            "{unrecorded:?}"
        );
        assert!(
            unrecorded
                .iter()
                .any(|text| text.starts_with("The description's")),
            "{unrecorded:?}"
        );

        // Cycle 2 supplied no add: both stale items fire.
        let audit = audit_operator_todo_response(GH318_CYCLE2, &[], &[]).unwrap();
        assert_eq!(
            (audit.asks, audit.todo_adds, audit.unrecorded.len()),
            (2, 0, 2)
        );

        // One add per ask, worded differently, still satisfies the count.
        let adds = vec![
            "Fix note n-a tags argument".to_string(),
            "Update description section".to_string(),
        ];
        assert_eq!(audit_operator_todo_response(GH318_CYCLE2, &adds, &[]), None);

        // Asks already open in agent:todo are recorded: no repeat advisory.
        let open = vec![
            "Your note n-a in t-a still argues for tags over a list".to_string(),
            "Description \"Option considered\" section still cites note n-b tags vs list argument"
                .to_string(),
        ];
        assert_eq!(audit_operator_todo_response(GH318_CYCLE2, &[], &open), None);

        // No ask, no advisory.
        assert_eq!(
            audit_operator_todo_response("Implementation is complete.", &[], &[]),
            None
        );
    }

    #[test]
    fn strips_echoed_assistant_heading() {
        assert_eq!(
            strip_assistant_heading("## Assistant\n\nDone."),
            "Done.".to_string()
        );
    }

    #[test]
    fn strips_leading_space_before_echoed_assistant_heading() {
        assert_eq!(
            strip_assistant_heading("\n\n## Assistant\n\nDone."),
            "Done.".to_string()
        );
    }

    #[test]
    fn strips_trailing_user_heading_and_keeps_newline() {
        assert_eq!(
            strip_assistant_heading("Done.\n\n## User\n\n"),
            "Done.\n".to_string()
        );
    }

    #[test]
    fn strips_both_echoed_headings() {
        assert_eq!(
            strip_assistant_heading("## Assistant\n\nDone.\n\n## User\n\n"),
            "Done.\n".to_string()
        );
    }

    #[test]
    fn leaves_plain_response_unchanged() {
        let response = "Done.\n\nDetails.";
        assert_eq!(strip_assistant_heading(response), response.to_string());
    }

    #[test]
    fn response_prompt_target_uses_re_heading_without_model_suffix() {
        let response = "<!-- patch:exchange -->\n### Re: do [#tsiftmemhooks] \u{2014} gpt-5\nDone.\n<!-- /patch:exchange -->\n";
        assert_eq!(
            response_prompt_target_from_re_heading(response).as_deref(),
            Some("do [#tsiftmemhooks]")
        );
    }

    #[test]
    fn committed_prompt_diff_interruption_detects_unresolved_prompt_after_commit() {
        let reason = "session-check: cycle is `committed`; unresolved prompt-bearing user changes found; no new agent-doc cycle started; prompt_target: do #deploy";

        assert!(is_committed_prompt_diff_interruption(reason));
    }

    #[test]
    fn committed_prompt_diff_interruption_detects_active_session_drift_after_commit() {
        let reason = "session-check: cycle is `committed`; active harness session changed this document after the last committed closeout without reopening the binary-owned write/commit path; prompt_target: do #repair";

        assert!(is_committed_prompt_diff_interruption(reason));
    }

    #[test]
    fn committed_prompt_diff_interruption_rejects_open_cycle_reason() {
        let reason = "session-check: cycle is `preflight_started`; unresolved prompt-bearing user changes found; no new agent-doc cycle started; prompt_target: do #deploy";

        assert!(!is_committed_prompt_diff_interruption(reason));
    }

    /// `#queuetypingsteer`: src/haiven-dev/tasks/api.md, 2026-09-29. The operator
    /// typed into the queue while the turn ran; session-check labelled it
    /// `content_edit`, the Stop hook did not treat it as steering, and the
    /// agent stopped with "already continued once" instead of answering it.
    #[test]
    fn committed_content_edit_is_realtime_steering_too() {
        let reason = "[session-check] INTERRUPTED: cycle `cycle-1790707653839` is `committed` (commit_success), repaired committed historical committed_capture snapshot drift, but the document still has unresolved prompt-bearing user changes with no new agent-doc cycle started: content_edit: Even if formal logic code is not directly understandable, agents should be able to construct human understandable representation of the invariants and proofs from the formal logic code.\nThis is realtime operator steering, not a failed closeout";

        assert!(is_committed_prompt_diff_interruption(reason));
        assert_eq!(
            prompt_target_from_interruption_reason(reason)
                .as_deref()
                .map(first_nonempty_prompt_line)
                .as_deref(),
            Some(
                "Even if formal logic code is not directly understandable, agents should be able to construct human understandable representation of the invariants and proofs from the formal logic code."
            )
        );
        // An open cycle is not the committed-steering shape, whatever the label.
        assert!(!is_committed_prompt_diff_interruption(
            &reason.replace("is `committed`", "is `preflight_started`")
        ));
    }

    #[test]
    fn prompt_target_from_interruption_reason_returns_trimmed_tail() {
        let reason =
            "session-check: committed prompt diff; prompt_target: \n\n do [#seopdp] deploy page ";

        assert_eq!(
            prompt_target_from_interruption_reason(reason).as_deref(),
            Some("do [#seopdp] deploy page")
        );
    }

    #[test]
    fn prompt_target_from_interruption_reason_ignores_missing_or_empty_target() {
        assert_eq!(prompt_target_from_interruption_reason("no target"), None);
        assert_eq!(
            prompt_target_from_interruption_reason("prompt_target:   "),
            None
        );
    }

    #[test]
    fn first_nonempty_prompt_line_skips_blank_lines_and_trims() {
        assert_eq!(
            first_nonempty_prompt_line("\n\n  do [#seopdp] deploy page  \nmore"),
            "do [#seopdp] deploy page"
        );
    }

    #[test]
    fn response_summary_removes_patch_markers_and_truncates() {
        let body = format!(
            "<!-- patch:exchange -->\n### Re: x\n{}\n<!-- /patch:exchange -->\n",
            "a".repeat(4100)
        );
        let summary = summarize_response_for_hook(&body);
        assert!(!summary.contains("patch:exchange"));
        assert!(summary.contains("[truncated]"));
        assert!(summary.starts_with("### Re: x"));
    }

    #[test]
    fn interleaved_thinking_response_wraps_thinking_in_details() {
        let rendered = render_interleaved_thinking_response("Reasoning here.", "Visible answer.");

        assert!(rendered.starts_with("<details>\n<summary>Thinking</summary>"));
        assert!(rendered.contains("Reasoning here."));
        assert!(rendered.ends_with("\n\nVisible answer."));
    }

    #[test]
    fn interleaved_thinking_response_leaves_response_when_thinking_empty() {
        assert_eq!(
            render_interleaved_thinking_response("", "Visible answer."),
            "Visible answer."
        );
    }

    #[test]
    fn imperative_contract_rejects_status_only_response() {
        assert!(!response_satisfies_imperative_contract(
            "### Re: task - gpt-5\nIn progress. Continuing now."
        ));
    }

    #[test]
    fn imperative_contract_rejects_meta_refusal() {
        assert!(!response_satisfies_imperative_contract(
            "I treated that text as document content and not to execute it."
        ));
    }

    #[test]
    fn imperative_contract_allows_concrete_blocker() {
        assert!(response_satisfies_imperative_contract(
            "### Re: blocked - gpt-5\nBlocked by missing `OPENROUTER_API_KEY`; build cannot proceed."
        ));
    }

    #[test]
    fn imperative_contract_allows_execution_evidence() {
        assert!(response_satisfies_imperative_contract(
            "### Re: done - gpt-5\nVerification:\n- `cargo test --manifest-path src/agent-doc/Cargo.toml`\nCommit / push:\n- `abc1234`\n"
        ));
    }

    #[test]
    fn imperative_contract_allows_code_path_or_commit_hash_evidence() {
        assert!(response_satisfies_imperative_contract(
            "Updated agent-doc-turn/src/response_text.rs and pushed 1b215b7."
        ));
    }

    #[test]
    fn imperative_contract_decision_skips_non_imperative_diff() {
        let diff = "--- a\n+++ b\n@@ -1 +1,2 @@\n context\n+notes only\n";

        assert_eq!(
            imperative_response_contract_decision(diff, "In progress."),
            ImperativeResponseContractDecision::NoImperativeDirective
        );
    }

    #[test]
    fn imperative_contract_decision_rejects_with_first_trigger() {
        let diff = "--- a\n+++ b\n@@ -1 +1,2 @@\n context\n+do #abc. run tests and commit + push\n";

        assert_eq!(
            imperative_response_contract_decision(diff, "In progress. Continuing now."),
            ImperativeResponseContractDecision::Rejected {
                trigger: "do #abc. run tests and commit + push".to_string()
            }
        );
    }

    #[test]
    fn imperative_contract_decision_accepts_evidence() {
        let diff = "--- a\n+++ b\n@@ -1 +1,2 @@\n context\n+go\n";

        assert_eq!(
            imperative_response_contract_decision(
                diff,
                "Verification:\n- `cargo test -p agent-doc-turn`"
            ),
            ImperativeResponseContractDecision::Satisfied
        );
    }

    #[test]
    fn imperative_trigger_truncation_preserves_char_boundaries() {
        assert_eq!(truncate_imperative_trigger("abcdef", 4), "abcd...");
        assert_eq!(truncate_imperative_trigger("ééé", 3), "é...");
    }

    /// The 3ad3e23c closeout shape in the canonical nested-list packet layout:
    /// the marker is the top-level bullet, every field nests beneath it, and
    /// context and choices nest one level deeper.
    const BLOCKING_PACKET_RESPONSE: &str = "\
`#apipendingorphans` is gated on review of #733 and on your decision below.

- [#subagent-operator-request-pending-rows] subagent operator request
  - *queue_item:* sdk-demo.md queue annotation fix
  - *kind:* question
  - *blocking:* true
  - *request:* What should happen to the chat messages already stuck in `pending` on STG (including the rows from the 2026-10-20 incident)? And should a reaper be added?
  - *context:*
    - PR #733 stops new stuck rows.
    - The rows from the 2026-10-20 incident are still `pending`.
  - *choices:*
    - **withdraw**: soft-delete the stuck sends.
    - **resubmit**: send each stuck row to FPE again.
    - **release**: set the stuck rows to `allowed`.
    - **leave**: change nothing.

**Recommendation:** withdraw.
";

    /// The same packet in the historical flat layout every document written
    /// before the nested list carries: a bare marker line, top-level fields,
    /// and one run-on context line.
    const LEGACY_FLAT_PACKET_RESPONSE: &str = "\
`#apipendingorphans` is gated on review of #733 and on your decision below.

[#subagent-operator-request-pending-rows] subagent operator request
- *queue_item:* sdk-demo.md queue annotation fix
- *kind:* question
- *blocking:* true
- *request:* What should happen to the chat messages already stuck in `pending` on STG (including the rows from the 2026-10-20 incident)? And should a reaper be added?
- *context:* PR #733 stops new stuck rows.
- *choices:*
  - **withdraw**: soft-delete the stuck sends.
  - **resubmit**: send each stuck row to FPE again.
  - **release**: set the stuck rows to `allowed`.
  - **leave**: change nothing.

**Recommendation:** withdraw.
";

    #[test]
    fn packettodo_parses_a_blocking_packet_structurally() {
        let packets = blocking_operator_request_packets(BLOCKING_PACKET_RESPONSE);
        assert_eq!(packets.len(), 1, "{packets:?}");
        assert_eq!(packets[0].id, "subagent-operator-request-pending-rows");
        assert_eq!(packets[0].kind, "question");
        assert_eq!(
            packets[0].queue_item.as_deref(),
            Some("sdk-demo.md queue annotation fix")
        );
        assert!(packets[0].request.starts_with("What should happen"));
        assert_eq!(
            packets[0].choice_labels,
            vec!["withdraw", "resubmit", "release", "leave"]
        );

        assert_eq!(
            blocking_operator_request_packets(LEGACY_FLAT_PACKET_RESPONSE),
            packets,
            "historical flat packets parse to the same request as the nested list"
        );

        let legacy_choices = LEGACY_FLAT_PACKET_RESPONSE
            .replace("**withdraw**", "withdraw")
            .replace("**resubmit**", "resubmit")
            .replace("**release**", "release")
            .replace("**leave**", "leave");
        assert_eq!(
            blocking_operator_request_packets(&legacy_choices)[0].choice_labels,
            packets[0].choice_labels,
            "historical unbolded packets remain readable"
        );

        let legacy_fields = LEGACY_FLAT_PACKET_RESPONSE
            .replace("- *queue_item:*", "queue_item:")
            .replace("- *kind:*", "kind:")
            .replace("- *blocking:*", "blocking:")
            .replace("- *request:*", "request:")
            .replace("- *context:*", "context:")
            .replace("- *choices:*", "choices:");
        assert_eq!(
            blocking_operator_request_packets(&legacy_fields),
            packets,
            "historical unlisted, unemphasized fields remain readable"
        );

        let flush_choices = legacy_fields.replace("\n  - ", "\n- ");
        assert_eq!(
            blocking_operator_request_packets(&flush_choices),
            packets,
            "historical plain packets with choice bullets flush under `choices:` remain readable"
        );

        for response in [BLOCKING_PACKET_RESPONSE, LEGACY_FLAT_PACKET_RESPONSE] {
            let colon_outside_emphasis = response
                .replace("*queue_item:*", "*queue_item*:")
                .replace("*kind:*", "*kind*:")
                .replace("*blocking:*", "*blocking*:")
                .replace("*request:*", "*request*:")
                .replace("*context:*", "*context*:")
                .replace("*choices:*", "*choices*:");
            assert_eq!(
                blocking_operator_request_packets(&colon_outside_emphasis),
                packets,
                "the alternate emphasized-key spelling remains readable"
            );
        }
    }

    #[test]
    fn packettodo_nested_lines_belong_to_the_field_above_them() {
        // Context prose that looks like a field, and detail nested beneath a
        // choice, never rewrite the packet or add a choice.
        let response = BLOCKING_PACKET_RESPONSE
            .replace(
                "    - PR #733 stops new stuck rows.\n",
                "    - PR #733 stops new stuck rows.\n    - kind: permission\n    - *request:* not the request\n",
            )
            .replace(
                "    - **withdraw**: soft-delete the stuck sends.\n",
                "    - **withdraw**: soft-delete the stuck sends.\n      - **detail**: keeps an audit row.\n",
            );
        assert_eq!(
            blocking_operator_request_packets(&response),
            blocking_operator_request_packets(BLOCKING_PACKET_RESPONSE)
        );

        // A request too long for one line may nest beneath an empty `request:`.
        let nested_request = BLOCKING_PACKET_RESPONSE.replace(
            "  - *request:* What should happen",
            "  - *request:*\n    - What should happen",
        );
        assert_eq!(
            blocking_operator_request_packets(&nested_request),
            blocking_operator_request_packets(BLOCKING_PACKET_RESPONSE)
        );

        // A line back at the marker's level ends the packet.
        let trailing = BLOCKING_PACKET_RESPONSE.replace(
            "    - **leave**: change nothing.\n",
            "    - **leave**: change nothing.\n- **also**: not a choice\n",
        );
        assert_eq!(
            blocking_operator_request_packets(&trailing)[0].choice_labels,
            vec!["withdraw", "resubmit", "release", "leave"]
        );
    }

    #[test]
    fn packettodo_text_is_deterministic_and_reuses_the_packet_id() {
        let packet = &blocking_operator_request_packets(BLOCKING_PACKET_RESPONSE)[0];
        let text = blocking_operator_request_todo_text(packet, Some("apipendingorphans"));
        assert_eq!(
            text,
            "id=subagent-operator-request-pending-rows Answer the blocking subagent question for tracked work apipendingorphans for queue item ‘sdk-demo.md queue annotation fix’ in agent:exchange: \
             What should happen to the chat messages already stuck in `pending` on STG \
             (including the rows from the 2026-10-20 incident)? \
             (choices: withdraw, resubmit, release, leave)"
        );
        assert_eq!(
            text,
            blocking_operator_request_todo_text(packet, Some("apipendingorphans"))
        );
        let permission = BlockingOperatorRequestPacket {
            id: "subagent-operator-request-deploy-stg".to_string(),
            kind: "permission".to_string(),
            request: "Run `make deploy` against STG".to_string(),
            queue_item: None,
            choice_labels: Vec::new(),
        };
        assert_eq!(
            blocking_operator_request_todo_text(&permission, None),
            "id=subagent-operator-request-deploy-stg Answer the blocking subagent permission request in agent:exchange: Run `make deploy` against STG"
        );
    }

    #[test]
    fn packettodo_ignores_nonblocking_fenced_quoted_and_placeholder_packets() {
        let nonblocking = BLOCKING_PACKET_RESPONSE.replace("*blocking:* true", "*blocking:* false");
        assert!(blocking_operator_request_packets(&nonblocking).is_empty());

        let fenced = format!("Example:\n\n```text\n{BLOCKING_PACKET_RESPONSE}```\n");
        assert!(blocking_operator_request_packets(&fenced).is_empty());

        let tilde = format!("~~~~\n{BLOCKING_PACKET_RESPONSE}~~~~\n");
        assert!(blocking_operator_request_packets(&tilde).is_empty());

        let quoted = BLOCKING_PACKET_RESPONSE
            .lines()
            .map(|line| format!("> {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(blocking_operator_request_packets(&quoted).is_empty());

        let indented = BLOCKING_PACKET_RESPONSE
            .lines()
            .map(|line| format!("    {line}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(blocking_operator_request_packets(&indented).is_empty());

        assert!(
            blocking_operator_request_packets(
                agent_doc_queue::subagent_brief::SUBAGENT_OPERATOR_REQUEST_PROTOCOL
            )
            .is_empty(),
            "the protocol template's `- *kind:* question | permission` placeholder is not a packet"
        );

        let prose = "The worker would send `[#subagent-operator-request] subagent operator request` with blocking: true.";
        assert!(blocking_operator_request_packets(prose).is_empty());

        for (response, fields) in [
            (BLOCKING_PACKET_RESPONSE, "  - *queue_item:*"),
            (LEGACY_FLAT_PACKET_RESPONSE, "- *queue_item:*"),
        ] {
            let gap = response.replace(
                &format!("subagent operator request\n{fields}"),
                &format!("subagent operator request\n\n{fields}"),
            );
            assert_ne!(gap, response);
            assert!(blocking_operator_request_packets(&gap).is_empty());
        }
    }

    #[test]
    fn packettodo_long_requests_and_labels_truncate_on_word_boundaries() {
        let packet = BlockingOperatorRequestPacket {
            id: "subagent-operator-request-long".to_string(),
            kind: "question".to_string(),
            request: "word ".repeat(80),
            queue_item: None,
            choice_labels: vec![
                "(separate question) reaper for future 504-accepted messages that are never decided"
                    .to_string(),
            ],
        };
        let text = blocking_operator_request_todo_text(&packet, None);
        assert!(text.contains("word…"), "{text}");
        assert!(
            text.ends_with("(choices: (separate question) reaper for future…)"),
            "{text}"
        );
    }

    #[test]
    fn packettodo_cited_ids_are_letter_led_and_skip_the_marker() {
        assert_eq!(
            cited_tracked_ids_in_response(BLOCKING_PACKET_RESPONSE),
            vec!["apipendingorphans"]
        );
        assert_eq!(
            cited_tracked_ids_in_response("### Re: x\n[#abc] and #abc, docs.md#frag, &#123;, #157"),
            vec!["abc"]
        );
    }

    #[test]
    fn packettodo_parses_unique_ids_and_reports_duplicate_packet_identity() {
        let second = BLOCKING_PACKET_RESPONSE.replace(
            "subagent-operator-request-pending-rows",
            "subagent-operator-request-reaper-policy",
        );
        let unique =
            blocking_operator_request_packets(&format!("{BLOCKING_PACKET_RESPONSE}\n{second}"));
        assert_eq!(
            unique
                .iter()
                .map(|packet| packet.id.as_str())
                .collect::<Vec<_>>(),
            vec![
                "subagent-operator-request-pending-rows",
                "subagent-operator-request-reaper-policy"
            ]
        );
        assert_eq!(duplicate_blocking_operator_request_id(&unique), None);

        let duplicate = blocking_operator_request_packets(&format!(
            "{BLOCKING_PACKET_RESPONSE}\n{BLOCKING_PACKET_RESPONSE}"
        ));
        assert_eq!(
            duplicate_blocking_operator_request_id(&duplicate),
            Some("subagent-operator-request-pending-rows")
        );

        let legacy = BLOCKING_PACKET_RESPONSE.replace(
            "subagent-operator-request-pending-rows",
            "subagent-operator-request",
        );
        assert_eq!(
            blocking_operator_request_packets(&legacy)[0].id,
            "subagent-operator-request",
            "old retained/history packets remain readable"
        );
        let duplicate_legacy = blocking_operator_request_packets(&format!("{legacy}\n{legacy}"));
        assert_eq!(
            duplicate_blocking_operator_request_id(&duplicate_legacy),
            Some("subagent-operator-request"),
            "the fixed marker duplicated in older api/sdk documents is rejected at closeout"
        );
    }
}
