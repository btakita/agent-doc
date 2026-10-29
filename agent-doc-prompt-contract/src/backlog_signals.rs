//! Backlog-capture signal matching.
//!
//! Preflight arms `requires_backlog_capture` when the cycle's prompt text asks
//! for backlog work. The signals are prose words and phrases, so they match
//! whole words in prose only. A bare substring match armed the gate on paths
//! and identifiers that merely contain a signal: `tasks/agent-doc/x.md`,
//! `src/todo_list.rs`, a `#todo-id` hashtag, or a `tasks` inside a code span.
//!
//! Rules:
//! - Fenced code blocks and inline code spans are not prose.
//! - A whitespace-delimited chunk that is a path, URL, file name, hashtag,
//!   mention, `::` path or `key=value` pair is not prose. It also breaks a
//!   phrase, so a phrase never spans it.
//! - A word containing `_` is an identifier, not prose.
//! - Hyphens join a word (`follow-up`, `capture-todo-gate`), so a kebab-case
//!   token is one word and only matches a signal spelled the same way.
//! - Every signal word matches a whole word. The last word of a signal may
//!   also carry an inflection suffix (`todos`, `recommended`, `backlogs`), which
//!   keeps the coverage the substring match had at the end of a word.
//! - Line ends break phrases, as they did for the substring match.

/// Prose signals that a prompt asks for backlog work. Each entry is matched as
/// a sequence of whole words (see the module docs).
const BACKLOG_SIGNALS: &[&str] = &[
    "tasks",
    // Singular `task` only as a new item ("add a task for X"), never as a
    // reference to existing work ("this task", "the task failed").
    "a task",
    "new task",
    "another task",
    "todo",
    "backlog",
    "what's next",
    "what should we do",
    "what do we need",
    "recommendations",
    "recommend",
    "next steps",
    "action items",
    "what's left",
    "what remains",
    "what else",
    "add to backlog",
    "add to pending",
    "follow-up backlog",
    "follow up backlog",
    "follow-up items",
    "follow up items",
];

/// Suffixes the last word of a signal may carry.
const INFLECTION_SUFFIXES: &[&str] = &["", "s", "es", "ed", "ing", "ation", "ations"];

pub(crate) fn text_requests_backlog_work(text: &str) -> bool {
    let words = prose_words(text);
    BACKLOG_SIGNALS
        .iter()
        .any(|signal| words_contain_signal(&words, signal))
}

/// The prose words of `text`, lowercased. `None` is a barrier no phrase spans.
fn prose_words(text: &str) -> Vec<Option<String>> {
    let normalized = text.replace(['\u{2019}', '\u{2018}'], "'");
    let mut words = Vec::new();
    let mut fence: Option<(char, usize)> = None;

    for line in normalized.lines() {
        let trimmed = line.trim_start();
        // A blockquoted prompt-tree line is context for a nested leaf, not a
        // request (`#nestsubmit`).
        if trimmed.starts_with('>') {
            words.push(None);
            continue;
        }
        let fence_marker = trimmed
            .chars()
            .next()
            .filter(|ch| *ch == '`' || *ch == '~')
            .map(|ch| (ch, trimmed.chars().take_while(|c| *c == ch).count()));
        match (fence, fence_marker) {
            (None, Some((ch, count))) if count >= 3 => {
                fence = Some((ch, count));
                continue;
            }
            (Some((ch, len)), Some((marker, count)))
                if marker == ch && count >= len && trimmed[count..].trim().is_empty() =>
            {
                fence = None;
                continue;
            }
            (Some(_), _) => continue,
            _ => {}
        }

        for (index, segment) in line.split('`').enumerate() {
            // Odd segments sit between a pair of backticks: an inline code span.
            // A final unmatched backtick is literal text, so its tail is prose.
            let in_code_span = index % 2 == 1 && line.matches('`').count() > index;
            if in_code_span {
                words.push(None);
                continue;
            }
            push_segment_words(segment, &mut words);
        }
        words.push(None);
    }

    words
}

fn push_segment_words(segment: &str, words: &mut Vec<Option<String>>) {
    for chunk in segment.split_whitespace() {
        let core = chunk.trim_matches(|ch: char| {
            matches!(
                ch,
                '(' | ')'
                    | '['
                    | ']'
                    | '{'
                    | '}'
                    | '<'
                    | '>'
                    | '"'
                    | '\''
                    | ','
                    | ';'
                    | ':'
                    | '!'
                    | '?'
                    | '.'
                    | '*'
                    | '_'
                    | '~'
            )
        });
        if is_non_prose_chunk(core) {
            words.push(None);
            continue;
        }
        for token in
            core.split(|ch: char| !(ch.is_ascii_alphanumeric() || matches!(ch, '\'' | '-' | '_')))
        {
            let token = token.trim_matches(|ch| ch == '\'' || ch == '-');
            if token.is_empty() {
                continue;
            }
            if token.contains('_') {
                words.push(None);
                continue;
            }
            words.push(Some(token.to_ascii_lowercase()));
        }
    }
}

fn is_non_prose_chunk(core: &str) -> bool {
    if core.starts_with('#') || core.starts_with('@') {
        return true;
    }
    if core.contains('/') || core.contains('\\') || core.contains("::") || core.contains('=') {
        return true;
    }
    // A dot between two alphanumerics: a file name, extension or dotted path.
    let bytes = core.as_bytes();
    bytes.windows(3).any(|window| {
        window[1] == b'.' && window[0].is_ascii_alphanumeric() && window[2].is_ascii_alphanumeric()
    })
}

fn words_contain_signal(words: &[Option<String>], signal: &str) -> bool {
    let signal_words = signal.split_whitespace().collect::<Vec<_>>();
    let Some((last, leading)) = signal_words.split_last() else {
        return false;
    };
    words.windows(signal_words.len()).any(|window| {
        let leading_match = leading
            .iter()
            .zip(window)
            .all(|(expected, word)| word.as_deref() == Some(*expected));
        leading_match
            && window[leading.len()].as_deref().is_some_and(|word| {
                word.strip_prefix(*last)
                    .is_some_and(|suffix| INFLECTION_SUFFIXES.contains(&suffix))
            })
    })
}

#[cfg(test)]
mod tests {
    use super::text_requests_backlog_work as arms;

    #[test]
    fn paths_do_not_arm() {
        for text in [
            "fix the clear in haiven-dev tasks/fpe.md",
            "see tasks/agent-doc/x.md for context",
            "the bug lives in src/todo_list.rs",
            "edit ./tasks/backlog.md",
            "C:\\work\\tasks\\notes.md",
            "open https://example.com/todo/backlog",
            "agent_doc_queue::backlog::insert is wrong",
            "set queue=backlog in the frontmatter",
            "rename todo.md",
        ] {
            assert!(!arms(text), "a path must not arm: {text}");
        }
    }

    #[test]
    fn ids_and_identifiers_do_not_arm() {
        for text in [
            "re #todo-id: the guard still fires",
            "do [#backlogfix] now",
            "(#tasks-cleanup) shipped",
            "ping @todo-bot",
            "the todo_list helper panics",
            "BACKLOG_SIGNALS is too broad",
            "the capture-todo-gate check refused",
            "subtasks are fine",
        ] {
            assert!(!arms(text), "an id or identifier must not arm: {text}");
        }
    }

    #[test]
    fn code_spans_and_fences_do_not_arm() {
        for text in [
            "the `tasks` signal armed the gate",
            "call `add to backlog` in the helper",
            "```\nadd these to the backlog\n```\nwhy did it fail?",
            "~~~~\nwhat's next\n~~~~\n",
            "> add these to the backlog",
            "do the leaf\n> What's next?",
        ] {
            assert!(!arms(text), "code must not arm: {text}");
        }
    }

    #[test]
    fn real_requests_arm() {
        for text in [
            "add a task for X",
            "Add a new task for the flaky test",
            "track this as a todo",
            "track these as todos",
            "add these to the backlog",
            "Add to backlog: the crash",
            "create tasks for each finding",
            "What's next?",
            "What\u{2019}s left to do?",
            "What should we do about it?",
            "Any recommendations?",
            "What would you recommend?",
            "I recommended it earlier; capture it.",
            "list the next steps",
            "Action items, please.",
            "Any follow-up items to place in the backlog?",
            "add to the follow-up backlog",
            "TODO: wire the retry",
            "**Tasks**: _backlog_ ~~todo~~",
            "a stray backtick ` then add these to the backlog",
            "the `tasks` span aside, add these to the backlog",
            "```\ncode\n```\nadd these to the backlog",
        ] {
            assert!(arms(text), "a real request must arm: {text}");
        }
    }

    #[test]
    fn a_path_beside_a_real_request_still_arms() {
        assert!(arms("in tasks/fpe.md, add a task for the clear"));
        assert!(arms("#todo-id: add these to the backlog"));
    }

    #[test]
    fn singular_task_arms_only_as_a_new_item() {
        assert!(!arms("this task failed twice"));
        assert!(!arms("the task authorization covers one item"));
        assert!(arms("add a task for the retry"));
        assert!(arms("open another task for it"));
    }

    #[test]
    fn phrases_need_adjacent_prose_words() {
        assert!(!arms("what's tasks/x.md next"));
        assert!(!arms("next `x` steps"));
        assert!(!arms("what's\nnext"));
        assert!(arms("next steps"));
    }
}
