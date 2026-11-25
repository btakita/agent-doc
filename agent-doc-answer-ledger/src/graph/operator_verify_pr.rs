//! Strike a live `🙋` `[operator-verify]` row once the pull request it asks
//! someone to approve or merge is merged or closed (`#prverify`).
//!
//! The row is the ask, not the whole queue and not the rest of the document.
//! A bare `PR 231` is resolved inside its own queue block: from the row's own
//! URL, else the parent pull-request heading, else the block's single
//! repository. Another repository elsewhere in the document is not consulted,
//! and a heading that merely sits beside the row is not watched. The caller
//! supplies the observed state, so this module never reads GitHub and never
//! rebuilds the decision graph.

use std::collections::BTreeSet;

use anyhow::Result;

use agent_doc_element_queue::struck_work_text;
use agent_doc_queue::document_queue::{self, QueueEntry, QueuePrompt};
use agent_doc_queue::queue_set::QueueSet;

use super::upstream::{
    GithubUpstream, GithubUpstreamKind, bare_mentions, explicit_upstreams, shorthand_refs,
};
use super::upstream_settled::UpstreamState;

/// Document after one strike pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OperatorVerifyPrStrike {
    /// Successor document.
    pub content: String,
    /// Live rows converted to struck rows.
    pub struck: usize,
}

/// Pull requests the live gate rows name. Empty when the document has none.
/// This is the only set a refresh may look up.
pub fn operator_verify_pr_watches(content: &str) -> Result<BTreeSet<GithubUpstream>> {
    let mut watches = BTreeSet::new();
    for hit in hits(content)? {
        watches.extend(hit.upstreams);
    }
    Ok(watches)
}

/// Strike every live gate row whose named pull requests are all terminal.
///
/// Merged (any of them, the rest terminal) is done. Every one closed without
/// merging is moot. An open or unobserved pull request keeps the row. A second
/// call is a no-op. `None` when nothing changes.
pub fn reevaluate_operator_verify_prs(
    content: &str,
    state: &dyn Fn(&GithubUpstream) -> Option<UpstreamState>,
) -> Result<Option<OperatorVerifyPrStrike>> {
    let components = agent_doc_element::element::parse(content)?;
    let queues = QueueSet::parse(content, &components).map_err(|err| anyhow::anyhow!("{err}"))?;
    let mut replacements = Vec::new();
    let mut struck = 0usize;
    for block in queues.blocks() {
        let found = block_hits(&block.entries);
        if found.is_empty() {
            continue;
        }
        let mut entries = block.entries.clone();
        let mut converted = 0usize;
        for hit in found {
            let Some(observed) = all_terminal(&hit.upstreams, state) else {
                continue;
            };
            let QueueEntry::Prompt(prompt) = &entries[hit.index] else {
                continue;
            };
            let mut completed = prompt.clone();
            completed.text = struck_work_text(&completed.text).to_string();
            completed.note = Some(strike_note(&observed));
            entries[hit.index] = QueueEntry::Completed(completed);
            converted += 1;
        }
        if converted == 0 {
            continue;
        }
        let body = document_queue::render(&entries);
        if body == block.component.content(content) {
            continue;
        }
        struck += converted;
        replacements.push((block.component.open_end, block.component.close_start, body));
    }
    if replacements.is_empty() {
        return Ok(None);
    }
    replacements.sort_by_key(|(start, _, _)| std::cmp::Reverse(*start));
    let mut projected = content.to_string();
    for (start, end, body) in replacements {
        projected.replace_range(start..end, &body);
    }
    Ok(Some(OperatorVerifyPrStrike {
        content: projected,
        struck,
    }))
}

struct Hit {
    index: usize,
    upstreams: BTreeSet<GithubUpstream>,
}

fn hits(content: &str) -> Result<Vec<Hit>> {
    let components = agent_doc_element::element::parse(content)?;
    let queues = QueueSet::parse(content, &components).map_err(|err| anyhow::anyhow!("{err}"))?;
    let mut out = Vec::new();
    for block in queues.blocks() {
        out.extend(block_hits(&block.entries));
    }
    Ok(out)
}

fn block_hits(entries: &[QueueEntry]) -> Vec<Hit> {
    let block_repo = block_repo(entries);
    let mut parents: Vec<(usize, GithubUpstream)> = Vec::new();
    let mut hits = Vec::new();
    for (index, entry) in entries.iter().enumerate() {
        let Some(prompt) = prompt_of(entry) else {
            continue;
        };
        while parents
            .last()
            .is_some_and(|(indent, _)| *indent >= prompt.indent)
        {
            parents.pop();
        }
        let parent = parents.last().map(|(_, upstream)| upstream);
        if matches!(entry, QueueEntry::Prompt(_))
            && is_pr_gate_row(&prompt.text)
            && !attempt_held(&prompt.text)
            && !in_flight(&prompt.text)
        {
            let upstreams = named_pulls(&prompt.text, parent, block_repo.as_ref());
            if !upstreams.is_empty() {
                hits.push(Hit { index, upstreams });
            }
        }
        if let Some(heading) = sole_explicit_pull(&prompt.text) {
            parents.push((prompt.indent, heading));
        }
    }
    hits
}

fn prompt_of(entry: &QueueEntry) -> Option<&QueuePrompt> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) if !prompt.multiline => {
            Some(prompt)
        }
        _ => None,
    }
}

fn is_pr_gate_row(text: &str) -> bool {
    text.contains('🙋') && text.contains("[operator-verify]") && is_pr_gate(text)
}

fn is_pr_gate(text: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    lower.contains("approve")
        || lower.contains("reviewdecision")
        || lower.contains("review_required")
        || lower.contains("review required")
        || (lower.contains("merge") && mentions_pull(&lower))
        || (lower.contains("wait") && mentions_pull(&lower))
}

fn mentions_pull(lower: &str) -> bool {
    lower.contains("github.com/")
        || lower.contains("pull")
        || lower.contains(" pr ")
        || lower.contains("pr ")
        || lower.contains("#")
}

fn block_repo(entries: &[QueueEntry]) -> Option<(String, String)> {
    let mut repos = BTreeSet::new();
    for entry in entries {
        let Some(text) = entry_text(entry) else {
            continue;
        };
        for upstream in explicit_upstreams(text) {
            if upstream.kind == GithubUpstreamKind::PullRequest {
                repos.insert((upstream.owner, upstream.repo));
            }
        }
    }
    (repos.len() == 1).then(|| repos.into_iter().next())?
}

fn entry_text(entry: &QueueEntry) -> Option<&str> {
    match entry {
        QueueEntry::Prompt(prompt) | QueueEntry::Completed(prompt) => Some(prompt.text.as_str()),
        QueueEntry::Freeform(line) => Some(line.as_str()),
        _ => None,
    }
}

fn sole_explicit_pull(text: &str) -> Option<GithubUpstream> {
    let pulls: BTreeSet<GithubUpstream> = explicit_upstreams(text)
        .into_iter()
        .filter(|upstream| upstream.kind == GithubUpstreamKind::PullRequest)
        .collect();
    (pulls.len() == 1).then(|| pulls.into_iter().next())?
}

/// Pull requests this row names. A bare `PR N` or an approve-shaped `#N`
/// borrows a repository only from this row, its parent heading, or the
/// block's single repository. No number and a parent heading that is one
/// pull request uses that heading.
fn named_pulls(
    text: &str,
    parent: Option<&GithubUpstream>,
    block_repo: Option<&(String, String)>,
) -> BTreeSet<GithubUpstream> {
    let mut named: BTreeSet<GithubUpstream> = explicit_upstreams(text)
        .into_iter()
        .filter(|upstream| upstream.kind == GithubUpstreamKind::PullRequest)
        .collect();
    for (owner, repo, number) in shorthand_refs(text) {
        named.insert(GithubUpstream::pull_request(&owner, &repo, number));
    }
    let same_repo = named.iter().next().is_some_and(|first| {
        named
            .iter()
            .all(|upstream| upstream.owner == first.owner && upstream.repo == first.repo)
    });
    let repo = named
        .iter()
        .next()
        .filter(|_| same_repo)
        .map(|upstream| (upstream.owner.clone(), upstream.repo.clone()))
        .or_else(|| parent.map(|upstream| (upstream.owner.clone(), upstream.repo.clone())))
        .or_else(|| block_repo.cloned());
    if let Some((owner, repo)) = repo.as_ref() {
        for (kind, number) in bare_mentions(text) {
            if kind == GithubUpstreamKind::PullRequest {
                named.insert(GithubUpstream::pull_request(owner, repo, number));
            }
        }
        for number in hash_pull_numbers(text) {
            named.insert(GithubUpstream::pull_request(owner, repo, number));
        }
    }
    // A gate that names no number borrows its parent heading. A bare number
    // that has no repository does not borrow a different heading's number.
    if named.is_empty()
        && !mentions_pull_number(text)
        && let Some(parent) = parent
    {
        named.insert(parent.clone());
    }
    named
}

fn mentions_pull_number(text: &str) -> bool {
    bare_mentions(text)
        .iter()
        .any(|(kind, _)| *kind == GithubUpstreamKind::PullRequest)
        || !hash_pull_numbers(text).is_empty()
}

/// `#N` whose nearby words ask to approve, merge, or wait on a pull request.
/// A `[#id]` is not a number. `option #2` is not one either.
fn hash_pull_numbers(text: &str) -> Vec<u64> {
    let lower = text.to_ascii_lowercase();
    let bytes = lower.as_bytes();
    let mut numbers = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'#' && (index == 0 || bytes[index - 1] != b'[') {
            let start = index + 1;
            let mut end = start;
            while end < bytes.len() && bytes[end].is_ascii_digit() {
                end += 1;
            }
            if end > start
                && (end == bytes.len() || !bytes[end].is_ascii_alphanumeric())
                && hash_is_pull_ask(&lower, index)
                && let Ok(number) = lower[start..end].parse::<u64>()
                && number > 0
            {
                numbers.push(number);
            }
            index = end;
        } else {
            index += 1;
        }
    }
    numbers.sort();
    numbers.dedup();
    numbers
}

fn hash_is_pull_ask(lower: &str, hash: usize) -> bool {
    let from = hash.saturating_sub(24);
    let window = lower[from..hash].trim_end();
    window.ends_with("approve")
        || window.ends_with("merge")
        || window.ends_with("pull")
        || window.ends_with("pr")
        || window.ends_with("wait")
        || (window.ends_with("request") && window.contains("pull"))
}

fn attempt_held(text: &str) -> bool {
    let mut rest = text;
    while let Some(start) = rest.find("[#") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(']') else {
            break;
        };
        if after[..end].contains(':') {
            return true;
        }
        rest = &after[end + 1..];
    }
    false
}

fn in_flight(text: &str) -> bool {
    let mut rest = text.trim_start();
    if let Some(after) = rest.strip_prefix('🙋') {
        rest = after.trim_start();
    }
    const MARKERS: &[&str] = &["🚧", "🟡", "☑️", "⚠️", "⛔", "🔄"];
    MARKERS.iter().any(|marker| rest.starts_with(marker))
}

fn all_terminal(
    upstreams: &BTreeSet<GithubUpstream>,
    state: &dyn Fn(&GithubUpstream) -> Option<UpstreamState>,
) -> Option<Vec<(GithubUpstream, UpstreamState)>> {
    upstreams
        .iter()
        .map(|upstream| {
            state(upstream)
                .filter(|found| found.is_terminal())
                .map(|found| (upstream.clone(), found))
        })
        .collect()
}

fn strike_note(observed: &[(GithubUpstream, UpstreamState)]) -> String {
    let listed = observed
        .iter()
        .map(|(upstream, found)| format!("{} {}", upstream.url(), found.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let verdict = if observed
        .iter()
        .any(|(_, found)| *found == UpstreamState::Merged)
    {
        "done"
    } else {
        "moot"
    };
    format!("{verdict}, {listed} (#prverify)")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn document(queue: &str) -> String {
        format!("<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n")
    }

    fn pull(number: u64) -> GithubUpstream {
        GithubUpstream::pull_request("acme", "widgets", number)
    }

    fn merged_at(numbers: &[u64]) -> impl Fn(&GithubUpstream) -> Option<UpstreamState> + '_ {
        move |upstream| {
            numbers
                .contains(&upstream.number)
                .then_some(UpstreamState::Merged)
        }
    }

    fn rows(content: &str) -> Vec<(bool, String, Option<String>)> {
        let components = agent_doc_element::element::parse(content).unwrap();
        let queues = QueueSet::parse(content, &components).unwrap();
        let mut out = Vec::new();
        for block in queues.blocks() {
            for entry in &block.entries {
                match entry {
                    QueueEntry::Prompt(prompt) => {
                        out.push((true, prompt.text.clone(), prompt.note.clone()))
                    }
                    QueueEntry::Completed(prompt) => {
                        out.push((false, prompt.text.clone(), prompt.note.clone()))
                    }
                    _ => {}
                }
            }
        }
        out
    }

    #[test]
    fn a_bare_pull_request_is_watched_and_sibling_headings_are_not() {
        let doc = document(concat!(
            "- ### https://github.com/acme/widgets/pull/214\n",
            "  - %% title %%\n",
            "- ### https://github.com/acme/widgets/pull/232\n",
            "- ### 🙋 [#q-origin] follow-up: human (maintainer): approve PR 231. `reviewDecision` is still REVIEW_REQUIRED, and this preset cannot approve or merge. [operator-verify] [#q-gate]\n",
            "- ### 🙋 follow-up: operator: decide whether the misses need a follow-up. [operator-verify] [#q-decide]\n",
            "- ### https://github.com/acme/widgets/pull/238\n",
            "  - 🙋 [#q-child] follow-up: human (maintainer): approve acme/widgets#238. The review was reset. [operator-verify] [#q-childgate]\n",
        ));
        let watches = operator_verify_pr_watches(&doc).unwrap();
        let numbers: Vec<u64> = watches.iter().map(|upstream| upstream.number).collect();
        assert_eq!(numbers, vec![231, 238], "{watches:?}");
        let seen = std::cell::RefCell::new(Vec::new());
        let struck = reevaluate_operator_verify_prs(&doc, &|upstream| {
            seen.borrow_mut().push(upstream.number);
            Some(UpstreamState::Merged)
        })
        .unwrap()
        .expect("both gate rows strike");
        assert_eq!(struck.struck, 2, "{seen:?}");
        assert!(
            seen.borrow()
                .iter()
                .all(|number| *number == 231 || *number == 238),
            "the state lookup must not poll a sibling heading: {seen:?}"
        );
        let found = rows(&struck.content);
        assert!(
            found.iter().any(|(live, text, note)| {
                !live
                    && text.contains("approve PR 231")
                    && note.as_deref().unwrap_or("").contains(
                        "done, https://github.com/acme/widgets/pull/231 merged (#prverify)",
                    )
            }),
            "{found:?}"
        );
        assert!(
            found
                .iter()
                .any(|(live, text, _)| *live && text.contains("pull/214")),
            "a sibling heading stays: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|(live, text, _)| *live && text.contains("decide whether")),
            "a decision row is not a pull-request gate: {found:?}"
        );
        assert!(
            reevaluate_operator_verify_prs(&struck.content, &merged_at(&[231, 238]))
                .unwrap()
                .is_none(),
            "a second pass is a no-op"
        );
    }

    #[test]
    fn a_closed_pull_request_is_moot_and_an_open_one_stays() {
        let doc = document(concat!(
            "- ### 🙋 follow-up: human (maintainer): approve https://github.com/acme/widgets/pull/1. [operator-verify] [#q-a]\n",
            "- ### 🙋 follow-up: human (maintainer): re-approve #2 at `abc`. [operator-verify] [#q-b]\n",
            "  - still noted\n",
        ));
        // `#2` has no repository until the block has one. The first row's URL
        // is that single repository.
        let state = |upstream: &GithubUpstream| match upstream.number {
            1 => Some(UpstreamState::Closed),
            2 => Some(UpstreamState::Open),
            _ => None,
        };
        let struck = reevaluate_operator_verify_prs(&doc, &state)
            .unwrap()
            .expect("the closed pull request is moot");
        let found = rows(&struck.content);
        assert!(
            found.iter().any(|(live, text, note)| {
                !live
                    && text.contains("pull/1")
                    && note.as_deref().unwrap_or("").contains("moot,")
                    && note.as_deref().unwrap_or("").contains("closed")
            }),
            "{found:?}"
        );
        assert!(
            found
                .iter()
                .any(|(live, text, _)| *live && text.contains("re-approve")),
            "an open pull request stays: {found:?}"
        );
        assert!(
            found
                .iter()
                .any(|(live, text, _)| *live && text.contains("still noted")),
            "a child note is not the gate row: {found:?}"
        );
    }

    #[test]
    fn an_unobserved_pull_request_and_a_held_row_stay() {
        let doc = document(concat!(
            "- ### https://github.com/acme/widgets/pull/9\n",
            "- ### 🙋 [#subagent:leaf#1] follow-up: human (maintainer): approve PR 9. [operator-verify] [#q-held]\n",
            "- ### 🚧 🙋 follow-up: human (maintainer): approve https://github.com/acme/widgets/pull/8. [operator-verify] [#q-run]\n",
            "- ### 🙋 follow-up: human (maintainer): approve PR 7. [operator-verify] [#q-miss]\n",
        ));
        assert!(
            reevaluate_operator_verify_prs(&doc, &|upstream| {
                (upstream.number != 7).then_some(UpstreamState::Merged)
            })
            .unwrap()
            .is_none()
        );
        assert_eq!(
            operator_verify_pr_watches(&doc)
                .unwrap()
                .into_iter()
                .map(|upstream| upstream.number)
                .collect::<Vec<_>>(),
            vec![7],
            "a held or in-flight row is not refreshed"
        );
    }

    #[test]
    fn two_repositories_do_not_lend_a_bare_number_a_repo() {
        let doc = document(concat!(
            "- ### https://github.com/acme/widgets/pull/1\n",
            "- ### https://github.com/other/tools/pull/2\n",
            "- ### 🙋 follow-up: human (maintainer): approve PR 231. [operator-verify] [#q-gate]\n",
        ));
        assert!(operator_verify_pr_watches(&doc).unwrap().is_empty());
    }

    #[test]
    fn a_parent_heading_lends_its_repository_when_the_block_has_several() {
        let doc = document(concat!(
            "- ### https://github.com/other/tools/pull/2\n",
            "- ### https://github.com/acme/widgets/pull/214\n",
            "  - 🙋 follow-up: human (maintainer): approve PR 231. [operator-verify] [#q-gate]\n",
        ));
        let watches = operator_verify_pr_watches(&doc).unwrap();
        assert_eq!(watches.into_iter().collect::<Vec<_>>(), vec![pull(231)]);
    }

    #[test]
    fn one_open_named_pull_request_keeps_the_row() {
        let doc = document(
            "- ### 🙋 follow-up: human (maintainer): approve https://github.com/acme/widgets/pull/1 and https://github.com/acme/widgets/pull/2. [operator-verify] [#q-both]\n",
        );
        let state = |upstream: &GithubUpstream| match upstream.number {
            1 => Some(UpstreamState::Merged),
            2 => Some(UpstreamState::Open),
            _ => None,
        };
        assert!(
            reevaluate_operator_verify_prs(&doc, &state)
                .unwrap()
                .is_none()
        );
    }
}
