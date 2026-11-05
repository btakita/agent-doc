//! Text normalization, subject anchors, decision kinds and similarity used by
//! the answer-ledger matcher tiers.
//!
//! Everything here is deterministic and pure. The tiers that may act on their
//! own (identity, anchors plus an id overlap) only use exact comparisons from
//! this module; token similarity is used for annotation only.

use std::collections::BTreeSet;

/// Words that carry no subject. Removed before similarity and kind detection.
const STOPWORDS: &[&str] = &[
    "a", "an", "and", "any", "are", "as", "at", "be", "by", "do", "does", "for", "from", "has",
    "have", "if", "in", "into", "is", "it", "its", "of", "on", "one", "or", "so", "that", "the",
    "their", "then", "this", "to", "was", "were", "which", "will", "with", "you", "your", "we",
    "our", "should", "would", "can", "not", "no", "yes", "before", "after", "also",
];

/// Generated wrapper a binary-derived request todo puts in front of the real
/// question (`Answer the blocking subagent question for queue item ‘…’ in
/// agent:exchange: <question>`).
const REQUEST_WRAPPER_PREFIX: &str = "answer the blocking subagent question";

/// Strip every binary-owned decoration from one list row's text: list marker,
/// heading hashes, lifecycle glyphs, `[#id]` tokens, `⟦…⟧` tokens, HTML
/// comments, strike markers and the trailing `▫️` reply marker.
pub fn strip_decorations(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while !rest.is_empty() {
        if let Some(after) = rest.strip_prefix("<!--") {
            rest = after.find("-->").map_or("", |end| &after[end + 3..]);
            out.push(' ');
            continue;
        }
        if let Some(after) = rest.strip_prefix("[#")
            && let Some(end) = after.find(']')
            && !after[..end].contains(char::is_whitespace)
        {
            rest = &after[end + 1..];
            out.push(' ');
            continue;
        }
        if let Some(after) = rest.strip_prefix('⟦') {
            rest = after
                .find('⟧')
                .map_or("", |end| &after[end + '⟧'.len_utf8()..]);
            out.push(' ');
            continue;
        }
        if let Some(after) = rest.strip_prefix("~~") {
            rest = after;
            continue;
        }
        let ch = rest.chars().next().unwrap_or_default();
        out.push(ch);
        rest = &rest[ch.len_utf8()..];
    }
    let trimmed = out.trim();
    let trimmed = trimmed
        .trim_start_matches(['-', '*', '+', '#', ' '])
        .trim_start_matches(|c: char| !c.is_ascii() || c.is_whitespace());
    let trimmed =
        trimmed.trim_end_matches(|c: char| c == '▫' || c == '\u{fe0f}' || c.is_whitespace());
    collapse_whitespace(trimmed)
}

fn collapse_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// The question a row asks, without generated wrappers and trailing
/// provenance: `Answer the blocking subagent question for queue item ‘…’ in
/// agent:exchange:` is removed, and so are trailing `(choices: …)`,
/// `(source: …)` and `(stays open: …)` clauses.
pub fn question_body(text: &str) -> String {
    let mut body = strip_decorations(text);
    let lower = body.to_lowercase();
    if lower.starts_with(REQUEST_WRAPPER_PREFIX)
        && let Some(index) = body.find("agent:exchange:")
    {
        body = body[index + "agent:exchange:".len()..].trim().to_string();
    }
    for clause in ["(choices:", "(source:", "(stays open:"] {
        // Generated clauses are lowercase ASCII, so search the body itself.
        if let Some(index) = body.find(clause) {
            body.truncate(index);
        }
    }
    collapse_whitespace(body.trim())
}

/// Lowercase word tokens of `text` (alphanumerics plus inner `.`/`-` so a
/// version string or a hyphenated rule stays one token).
pub fn tokens(text: &str) -> Vec<String> {
    let lower = text.to_lowercase();
    let mut tokens = Vec::new();
    let mut current = String::new();
    let chars: Vec<char> = lower.chars().collect();
    for (index, ch) in chars.iter().enumerate() {
        let inner_joiner = (*ch == '.' || *ch == '-' || *ch == '_')
            && !current.is_empty()
            && chars
                .get(index + 1)
                .is_some_and(|next| next.is_alphanumeric());
        if ch.is_alphanumeric() || *ch == '\'' || inner_joiner {
            if *ch != '\'' {
                current.push(*ch);
            }
        } else if !current.is_empty() {
            tokens.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Normalized question text: the question body's tokens joined by one space.
/// Two rows with the same normalized text ask the same question (tier 1).
pub fn normalize_question(text: &str) -> String {
    tokens(&question_body(text)).join(" ")
}

/// Content tokens (stopwords removed) used by similarity (tier 3).
pub fn content_tokens(text: &str) -> BTreeSet<String> {
    tokens(&question_body(text))
        .into_iter()
        .filter(|token| !STOPWORDS.contains(&token.as_str()))
        .collect()
}

/// Token-set Jaccard similarity.
pub fn jaccard(a: &BTreeSet<String>, b: &BTreeSet<String>) -> f64 {
    if a.is_empty() && b.is_empty() {
        return 0.0;
    }
    let shared = a.intersection(b).count() as f64;
    let union = a.union(b).count() as f64;
    shared / union
}

fn numeric(token: &str) -> bool {
    token.chars().any(|c| c.is_ascii_digit())
        && token
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
}

/// Whether two token sets name conflicting numbers: each side carries a
/// number the other lacks (`cap 5 or 10` versus `cap 5 or 20`). A conflicting
/// pair is never reported as similar, however many words it shares.
pub fn numbers_conflict(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    let only_a = a.difference(b).any(|token| numeric(token));
    let only_b = b.difference(a).any(|token| numeric(token));
    only_a && only_b
}

/// Minimum Jaccard similarity for a tier-3 "possibly the same question" note.
pub const TEXT_SIMILARITY_THRESHOLD: f64 = 0.7;

/// Tier 3: normalized-text similarity. Annotation only.
pub fn similar_questions(a: &str, b: &str) -> Option<f64> {
    let left = content_tokens(a);
    let right = content_tokens(b);
    if left.len() < 4 || right.len() < 4 || numbers_conflict(&left, &right) {
        return None;
    }
    let score = jaccard(&left, &right);
    (score >= TEXT_SIMILARITY_THRESHOLD).then_some(score)
}

/// What kind of decision a question asks for, from its leading verb. `None`
/// means the row is work, not a question (`Implement …`, `Merge …`).
pub fn decision_kind(text: &str) -> Option<&'static str> {
    let body = question_body(text);
    let words = tokens(&body);
    for word in words.iter().take(6) {
        let kind = match word.as_str() {
            "choose" | "pick" | "select" => "choose",
            "decide" | "whether" | "which" => "decide",
            "name" | "identify" => "name",
            "approve" | "authorize" | "authorise" => "authorize",
            "confirm" | "say" | "tell" | "record" | "answer" => "answer",
            _ => continue,
        };
        return Some(kind);
    }
    None
}

/// Tier 2: the entities a question is about. Each anchor is `kind:value`:
/// `adr:41`, `ref:88` (PR/issue numbers), `ver:0.1.0`, `url:…`, `path:…`,
/// `opts:a|b` (a parenthesized `(a or b)` option set).
pub fn anchors(text: &str) -> BTreeSet<String> {
    let body = question_body(text);
    let mut anchors = BTreeSet::new();
    let lower = body.to_lowercase();
    for word in lower.split_whitespace() {
        let word = word
            .trim_matches(|c: char| matches!(c, ',' | ';' | ')' | '(' | '`' | '\'' | '"' | '.'));
        if word.starts_with("http://") || word.starts_with("https://") {
            anchors.insert(format!("url:{word}"));
        } else if word.contains('/')
            && word
                .rsplit('/')
                .next()
                .is_some_and(|tail| tail.contains('.'))
        {
            anchors.insert(format!("path:{word}"));
        }
    }
    let words = tokens(&body);
    for (index, word) in words.iter().enumerate() {
        let next = words.get(index + 1).map(String::as_str);
        if let Some(digits) = word.strip_prefix("adr")
            && let Some(number) = adr_number(digits, next)
        {
            anchors.insert(format!("adr:{number}"));
        }
        if matches!(word.as_str(), "pr" | "pull" | "issue")
            && let Some(number) = next
                .filter(|value| value.chars().all(|c| c.is_ascii_digit()))
                .or_else(|| {
                    words.get(index + 2).map(String::as_str).filter(|value| {
                        next == Some("request") && value.chars().all(|c| c.is_ascii_digit())
                    })
                })
        {
            anchors.insert(format!("ref:{}", number.trim_start_matches('0')));
        }
        if is_version(word) {
            anchors.insert(format!("ver:{}", word.trim_start_matches('v')));
        }
    }
    for (index, _) in lower.match_indices('#') {
        let digits: String = lower[index + 1..]
            .chars()
            .take_while(char::is_ascii_digit)
            .collect();
        if !digits.is_empty() {
            anchors.insert(format!("ref:{}", digits.trim_start_matches('0')));
        }
    }
    let mut rest = lower.as_str();
    while let Some(open) = rest.find('(') {
        let after = &rest[open + 1..];
        let Some(close) = after.find(')') else { break };
        let inner = &after[..close];
        if inner.contains(" or ") {
            let mut options: Vec<String> = inner
                .split(" or ")
                .map(|option| tokens(option).join(" "))
                .filter(|option| !option.is_empty())
                .collect();
            options.sort();
            if options.len() >= 2 {
                anchors.insert(format!("opts:{}", options.join("|")));
            }
        }
        rest = &after[close..];
    }
    anchors
}

fn adr_number(suffix: &str, next: Option<&str>) -> Option<String> {
    let suffix = suffix.trim_start_matches('-');
    let digits = if suffix.is_empty() { next? } else { suffix };
    (!digits.is_empty() && digits.chars().all(|c| c.is_ascii_digit()))
        .then(|| digits.trim_start_matches('0').to_string())
}

fn is_version(word: &str) -> bool {
    let core = word.strip_prefix('v').unwrap_or(word);
    let numeric_part: String = core
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    numeric_part.matches('.').count() >= 2 && numeric_part.split('.').all(|part| !part.is_empty())
}

/// Tier 2 anchor comparison: both sides carry anchors, at least two of them
/// (or one option set) are shared, and neither carries an anchor of a kind the
/// other has with a different value (`adr:41` versus `adr:46`).
pub fn anchors_match(a: &BTreeSet<String>, b: &BTreeSet<String>) -> bool {
    if a.is_empty() || b.is_empty() {
        return false;
    }
    let shared: Vec<&String> = a.intersection(b).collect();
    let strong = shared.len() >= 2 || shared.iter().any(|anchor| anchor.starts_with("opts:"));
    if !strong {
        return false;
    }
    let kinds = |set: &BTreeSet<String>, kind: &str| -> BTreeSet<String> {
        set.iter()
            .filter(|anchor| anchor.starts_with(kind))
            .cloned()
            .collect()
    };
    let numbers = |set: &BTreeSet<String>| -> BTreeSet<String> {
        set.iter()
            .flat_map(|anchor| tokens(anchor.split_once(':').map_or("", |(_, value)| value)))
            .filter(|token| numeric(token))
            .collect()
    };
    if numbers_conflict(&numbers(a), &numbers(b)) {
        return false;
    }
    for kind in ["adr:", "ref:", "ver:"] {
        let left = kinds(a, kind);
        let right = kinds(b, kind);
        if !left.is_empty() && !right.is_empty() && left.is_disjoint(&right) {
            return false;
        }
    }
    true
}

/// The clause a heuristic amend adds: the later answer with the earlier one
/// (as normalized tokens) removed.
pub fn amended_clause(previous: &str, next: &str) -> String {
    let left = tokens(previous).join(" ");
    let right = tokens(next).join(" ");
    let clause = if left.is_empty() {
        right
    } else {
        right.replacen(&left, " ", 1)
    };
    let clause = clause.split_whitespace().collect::<Vec<_>>().join(" ");
    if clause.is_empty() {
        next.trim().to_string()
    } else {
        clause
    }
}

/// How a later answer relates to the one before it.
pub fn answer_relation(previous: &str, next: &str) -> crate::RevisionRelation {
    let left = tokens(previous).join(" ");
    let right = tokens(next).join(" ");
    if left == right {
        return crate::RevisionRelation::Restates;
    }
    if !left.is_empty() && right.contains(&left) {
        return crate::RevisionRelation::Amends;
    }
    let a: BTreeSet<String> = content_tokens(previous);
    let b: BTreeSet<String> = content_tokens(next);
    if jaccard(&a, &b) >= 0.2 {
        crate::RevisionRelation::Restates
    } else {
        crate::RevisionRelation::Supersedes
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decorations_and_wrappers_are_removed() {
        let row = "  - 🙋 ⟦you: 1 · agent: 0 · done: 0⟧ [#subagent-operator-request-c690prov-cap] Answer the blocking subagent question for queue item ‘do [#tdprovauth]’ in agent:exchange: Choose the cap N (5 or 10). (choices: N = 5, N = 10) (source: agent:review [#tdprovauth]) <!-- agent-doc:waiting-item -->";
        assert_eq!(question_body(row), "Choose the cap N (5 or 10).");
        assert_eq!(normalize_question(row), "choose the cap n 5 or 10");
    }

    #[test]
    fn anchors_capture_adr_refs_versions_and_option_sets() {
        let found = anchors(
            "Name the approver for docs PR #88 (ADR 0046) and keep v0.1.0 (oldest-issued or least-recently-used)",
        );
        assert!(found.contains("adr:46"), "{found:?}");
        assert!(found.contains("ref:88"), "{found:?}");
        assert!(found.contains("ver:0.1.0"), "{found:?}");
        assert!(
            found.contains("opts:least-recently-used|oldest-issued"),
            "{found:?}"
        );
    }

    #[test]
    fn conflicting_numbers_are_never_similar() {
        let a =
            "Choose the live credential cap N for every integration account environment (5 or 10)";
        let b =
            "Choose the live credential cap N for every integration account environment (5 or 20)";
        assert!(similar_questions(a, b).is_none());
        let c = "Choose the live credential cap N for every integration account environment (5 or 10) now";
        assert!(similar_questions(a, c).is_some());
    }

    #[test]
    fn anchor_kinds_with_different_values_conflict() {
        let a = anchors("Pick the approver for ADR 0046 PR #88");
        let b = anchors("Pick the approver for ADR 0041 PR #88");
        assert!(!anchors_match(&a, &b));
        let c = anchors("Name who approves ADR 0046 in PR #88");
        assert!(anchors_match(&a, &c));
    }

    #[test]
    fn decision_kind_reads_the_leading_verb() {
        assert_eq!(decision_kind("Choose the cap"), Some("choose"));
        assert_eq!(
            decision_kind("As release owner, decide whether to bump"),
            Some("decide")
        );
        assert_eq!(decision_kind("Implement and reconcile ADR 0041"), None);
    }

    #[test]
    fn answer_relations() {
        use crate::RevisionRelation::*;
        assert_eq!(
            answer_relation("Pat Owner", "Pat Owner is the Alpha release."),
            Amends
        );
        assert_eq!(
            answer_relation("Add Alex + Sam", "Keep the approver as is."),
            Supersedes
        );
        assert_eq!(
            answer_relation(
                "JWT makes this moot",
                "JWS makes the cap moot. We don't store credentials."
            ),
            Restates
        );
    }
}
