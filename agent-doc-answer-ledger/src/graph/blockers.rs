//! External blocker ids on the decision graph.
//!
//! A stays-open clause can name an id that is not a backlog, todo, or review
//! row (`#unrealpr7`, `#bossroomcopyrightowner`). That id is still a gate.
//! This module records three links:
//!
//! - the blocker id blocks the work item;
//! - a GitHub pull request or issue settles that id when it closes;
//! - a decision that names the id and says the wait is over settles it.
//!
//! The queue reads the durable result back from a
//! `<!-- agent-doc:blocker-settled ... -->` marker. Tracked open items are
//! never settled by that marker.

use std::collections::{BTreeMap, BTreeSet};

use super::upstream::{GithubUpstream, explicit_upstreams};
use super::{DecisionGraph, EdgeSource, GraphEdge, Node, NodeKind};
use crate::EdgeKind;
use crate::extract::Observations;

/// `upstream_of` clause: the artifact settles the blocker id when it closes.
pub const SETTLES_CLAUSE: &str = "settles";

/// `upstream_of` clause: the blocker id blocks the work item.
pub const BLOCKS_CLAUSE: &str = "blocks";

/// Document marker the queue derivation reads. Ids are lowercase and separated
/// by spaces. The marker only releases external blockers.
pub const SETTLEMENT_MARKER_PREFIX: &str = "<!-- agent-doc:blocker-settled";

const CUES: &[&str] = &[
    "no longer blocked",
    "not a blocker",
    "not a real blocker",
    "not a real wait",
    "already answered",
    "is moot",
    "are moot",
];

impl DecisionGraph {
    /// Record blocker ids cited by open tracked items, the items they gate,
    /// any single pull request those items name, and any decision already in
    /// `document` that says the wait is over.
    pub fn link_external_blockers(&mut self, observations: &Observations, document: &str) {
        let known = known_ids(observations);
        let mut gates: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut texts: BTreeMap<String, String> = BTreeMap::new();
        for item in observations.tracked.iter().filter(|item| item.open) {
            let external = external_blocker_ids(&item.text, &known);
            if external.is_empty() {
                continue;
            }
            let slot = texts.entry(item.id.clone()).or_default();
            if !slot.is_empty() {
                slot.push('\n');
            }
            slot.push_str(&item.text);
            for blocker in external {
                gates.entry(blocker).or_default().insert(item.id.clone());
            }
        }
        let pulls = prose_pulls(document);
        for (blocker, items) in &gates {
            let blocker_node = blocker_node_id(blocker);
            self.node(Node {
                id: blocker_node.clone(),
                kind: NodeKind::Fact,
                label: format!("#{blocker}"),
                open: None,
                location: None,
                first_seen_at: None,
                in_document: false,
            });
            for item in items {
                self.node(Node {
                    id: item.clone(),
                    kind: NodeKind::Item,
                    label: String::new(),
                    open: Some(true),
                    location: None,
                    first_seen_at: None,
                    in_document: true,
                });
                self.edge(GraphEdge {
                    from: blocker_node.clone(),
                    kind: EdgeKind::UpstreamOf,
                    to: item.clone(),
                    source: EdgeSource::Derived,
                    clause: Some(BLOCKS_CLAUSE.to_string()),
                });
            }
            let item_text = items
                .iter()
                .filter_map(|id| texts.get(id))
                .cloned()
                .collect::<Vec<_>>()
                .join("\n");
            for upstream in upstreams_for_blocker(blocker, &item_text, &pulls) {
                self.node(Node {
                    id: upstream.node_id(),
                    kind: NodeKind::Evidence,
                    label: upstream.url(),
                    open: None,
                    location: None,
                    first_seen_at: None,
                    in_document: false,
                });
                self.edge(GraphEdge {
                    from: upstream.node_id(),
                    kind: EdgeKind::UpstreamOf,
                    to: blocker_node.clone(),
                    source: EdgeSource::Derived,
                    clause: Some(SETTLES_CLAUSE.to_string()),
                });
                if prose_says_closed(&item_text, upstream.number) {
                    self.record_settling_decision(
                        blocker,
                        &format!("pull request {} closed", upstream.number),
                    );
                }
            }
            if text_settles_blocker(&item_text, blocker) || document_line_settles(document, blocker)
            {
                let label = settling_label(document, &item_text, blocker);
                self.record_settling_decision(blocker, &label);
            }
        }
    }

    /// GitHub artifacts recorded as settling some blocker id.
    pub fn blocker_upstreams(&self) -> Vec<GithubUpstream> {
        let mut out = Vec::new();
        for edge in &self.edges {
            if edge.kind != EdgeKind::UpstreamOf || edge.clause.as_deref() != Some(SETTLES_CLAUSE) {
                continue;
            }
            if let Some(upstream) = super::upstream::upstream_from_node_id(&edge.from) {
                out.push(upstream);
            }
        }
        out.sort();
        out.dedup();
        out
    }

    /// Blocker ids whose gate is over: a settling decision is on the graph, or
    /// a linked GitHub artifact is in `closed`.
    pub fn settled_external_blocker_ids(&self, closed: &[GithubUpstream]) -> BTreeSet<String> {
        let closed_ids: BTreeSet<String> = closed.iter().map(GithubUpstream::node_id).collect();
        let mut settled = BTreeSet::new();
        for edge in &self.edges {
            let Some(blocker) = edge.to.strip_prefix("blocker:") else {
                continue;
            };
            if edge.kind == EdgeKind::AppliesTo && edge.clause.as_deref() == Some(SETTLES_CLAUSE) {
                let decision_closed = self
                    .nodes
                    .get(&edge.from)
                    .is_some_and(|node| node.kind == NodeKind::Decision && node.open != Some(true));
                if decision_closed {
                    settled.insert(blocker.to_string());
                }
            }
            if edge.kind == EdgeKind::UpstreamOf
                && edge.clause.as_deref() == Some(SETTLES_CLAUSE)
                && closed_ids.contains(&edge.from)
            {
                settled.insert(blocker.to_string());
            }
        }
        settled
    }

    fn record_settling_decision(&mut self, blocker: &str, label: &str) {
        let decision = format!("dec-blocker-{blocker}");
        let blocker_node = blocker_node_id(blocker);
        self.node(Node {
            id: decision.clone(),
            kind: NodeKind::Decision,
            label: label.to_string(),
            open: Some(false),
            location: Some("blocker settlement".to_string()),
            first_seen_at: None,
            in_document: false,
        });
        self.edge(GraphEdge {
            from: decision,
            kind: EdgeKind::AppliesTo,
            to: blocker_node,
            source: EdgeSource::Derived,
            clause: Some(SETTLES_CLAUSE.to_string()),
        });
    }
}

/// Insert or extend the settlement marker. `None` when every settled id is
/// already recorded. Existing ids stay; a tracked row is unaffected.
pub fn merge_settlement_marker(content: &str, settled: &BTreeSet<String>) -> Option<String> {
    if settled.is_empty() {
        return None;
    }
    let mut ids = marker_ids(content);
    let before = ids.len();
    ids.extend(settled.iter().cloned());
    if ids.len() == before {
        return None;
    }
    let line = format!(
        "{SETTLEMENT_MARKER_PREFIX} {} -->",
        ids.into_iter().collect::<Vec<_>>().join(" ")
    );
    Some(rewrite_marker(content, &line))
}

pub fn marker_ids(content: &str) -> BTreeSet<String> {
    let mut ids = BTreeSet::new();
    for line in content.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix(SETTLEMENT_MARKER_PREFIX) else {
            continue;
        };
        let Some(body) = rest.split("-->").next() else {
            continue;
        };
        for id in body.split_whitespace() {
            let id = id.trim_matches('#').to_ascii_lowercase();
            if id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric())
                && !id.chars().all(|c| c.is_ascii_digit())
            {
                ids.insert(id);
            }
        }
    }
    ids
}

fn rewrite_marker(content: &str, line: &str) -> String {
    if let Some(start) = content.find(SETTLEMENT_MARKER_PREFIX)
        && let Some(end_rel) = content[start..].find("-->")
    {
        let end = start + end_rel + 3;
        let mut out = String::new();
        out.push_str(&content[..start]);
        out.push_str(line);
        out.push_str(&content[end..]);
        return out;
    }
    if let Some(rest) = content.strip_prefix("---\n")
        && let Some(close) = rest.find("\n---\n")
    {
        let at = 4 + close + 5;
        let mut out = String::new();
        out.push_str(&content[..at]);
        out.push('\n');
        out.push_str(line);
        out.push('\n');
        out.push_str(&content[at..]);
        return out;
    }
    format!("{line}\n{content}")
}

fn blocker_node_id(id: &str) -> String {
    format!("blocker:{id}")
}

fn known_ids(observations: &Observations) -> BTreeSet<String> {
    let mut known = observations.open_item_ids.clone();
    for item in &observations.tracked {
        known.insert(item.id.clone());
    }
    known
}

fn external_blocker_ids(text: &str, known: &BTreeSet<String>) -> Vec<String> {
    let mut out = Vec::new();
    for clause in stays_open_blocker_clauses(text) {
        if clause.iter().any(|id| known.contains(id)) {
            continue;
        }
        for id in clause {
            if !id.chars().all(|c| c.is_ascii_digit()) && !out.contains(&id) {
                out.push(id);
            }
        }
    }
    out
}

fn stays_open_blocker_clauses(text: &str) -> Vec<Vec<String>> {
    let lower = text.to_ascii_lowercase();
    let mut clauses = Vec::new();
    let mut from = 0;
    while let Some(rel) = lower[from..].find("(stays open:") {
        let body_start = from + rel + 1;
        let mut depth = 1usize;
        let mut body_end = text.len();
        for (offset, ch) in text[body_start..].char_indices() {
            match ch {
                '(' => depth += 1,
                ')' => {
                    depth -= 1;
                    if depth == 0 {
                        body_end = body_start + offset;
                        break;
                    }
                }
                _ => {}
            }
        }
        let phrase_end = (from + rel + "(stays open:".len()).min(body_end);
        let body = &text[phrase_end..body_end];
        if body.to_ascii_lowercase().contains("block") {
            clauses.push(cited_hash_ids(body));
        }
        from = body_end.max(body_start);
    }
    clauses
}

fn cited_hash_ids(text: &str) -> Vec<String> {
    let mut ids = Vec::new();
    let mut previous: Option<char> = None;
    for (index, ch) in text.char_indices() {
        if ch == '#' && !previous.is_some_and(|prev| prev.is_alphanumeric()) {
            let id: String = text[index + 1..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_'))
                .collect();
            let id = id.trim_end_matches(['-', '_']).to_ascii_lowercase();
            if id.chars().next().is_some_and(|c| c.is_ascii_alphanumeric()) && !ids.contains(&id) {
                ids.push(id);
            }
        }
        previous = Some(ch);
    }
    ids
}

fn pr_suffix(id: &str) -> Option<u64> {
    let lower = id.to_ascii_lowercase();
    let digits = lower
        .rfind("pr")
        .map(|at| &lower[at + 2..])
        .map(|rest| rest.trim_start_matches('-'))?;
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    // The suffix must be the tail, so `preview1` does not match.
    if !lower.ends_with(digits) {
        return None;
    }
    digits.parse().ok()
}

struct ProsePull {
    upstream: GithubUpstream,
    number: u64,
}

fn prose_pulls(document: &str) -> Vec<ProsePull> {
    let mut out = Vec::new();
    let lower = document.to_ascii_lowercase();
    for needle in ["pull request ", "pull ", "pr #", "pr "] {
        let mut rest = lower.as_str();
        let mut base = 0usize;
        while let Some(at) = rest.find(needle) {
            let number_at = base + at + needle.len();
            let after = &lower[number_at..];
            let digits: String = after.chars().take_while(|c| c.is_ascii_digit()).collect();
            if let Ok(number) = digits.parse::<u64>()
                && let Some(upstream) = repo_before(&lower, base + at)
                    .map(|(owner, repo)| GithubUpstream::pull_request(&owner, &repo, number))
            {
                out.push(ProsePull { upstream, number });
            }
            let next = at + needle.len();
            base += next;
            rest = &rest[next..];
        }
    }
    out
}

fn repo_before(text: &str, at: usize) -> Option<(String, String)> {
    let token = text[..at].split_whitespace().last()?;
    let token = token
        .trim_matches(|c: char| !(c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '/' | '.')));
    let (owner, repo) = token.split_once('/')?;
    if owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some((owner.to_string(), repo.to_string()))
}

fn upstreams_for_blocker(
    blocker: &str,
    item_text: &str,
    pulls: &[ProsePull],
) -> Vec<GithubUpstream> {
    let mut found = explicit_upstreams(item_text);
    let suffix = pr_suffix(blocker);
    for pull in pulls {
        let in_item = item_text.to_ascii_lowercase().contains(&pull.upstream.repo)
            && item_text.contains(&pull.number.to_string());
        let unique_suffix = suffix == Some(pull.number)
            && pulls
                .iter()
                .filter(|other| other.number == pull.number)
                .count()
                == 1;
        if (in_item && suffix == Some(pull.number)) || unique_suffix {
            found.push(pull.upstream.clone());
        }
    }
    if found.is_empty() && explicit_upstreams(item_text).len() == 1 {
        found = explicit_upstreams(item_text);
    }
    found.sort();
    found.dedup();
    found
}

fn prose_says_closed(text: &str, number: u64) -> bool {
    let lower = text.to_ascii_lowercase();
    let token = number.to_string();
    let mut rest = lower.as_str();
    let mut base = 0usize;
    while let Some(at) = rest.find(&token) {
        let abs = base + at;
        let before_ok = abs == 0 || !lower.as_bytes()[abs - 1].is_ascii_digit();
        let end = abs + token.len();
        let after_ok = end >= lower.len() || !lower.as_bytes()[end].is_ascii_digit();
        if before_ok && after_ok {
            let window_end = (end + 48).min(lower.len());
            let window = &lower[end..window_end];
            if window.contains("merged") || window.contains("closed") || window.contains("moot") {
                return true;
            }
        }
        let next = at + token.len();
        base += next;
        rest = &rest[next..];
    }
    false
}

fn text_settles_blocker(text: &str, blocker: &str) -> bool {
    let lower = text.to_ascii_lowercase();
    if !lower.contains(blocker) {
        return false;
    }
    cue_in(&lower)
}

fn document_line_settles(document: &str, blocker: &str) -> bool {
    document
        .lines()
        .any(|line| text_settles_blocker(line, blocker))
}

fn cue_in(lower: &str) -> bool {
    if lower.contains("not unblock")
        || lower.contains("do not unblock")
        || lower.contains("don't unblock")
    {
        return CUES.iter().any(|cue| lower.contains(cue));
    }
    CUES.iter().any(|cue| lower.contains(cue)) || lower.contains("unblock")
}

fn settling_label(document: &str, item_text: &str, blocker: &str) -> String {
    document
        .lines()
        .chain(item_text.lines())
        .find(|line| text_settles_blocker(line, blocker))
        .unwrap_or(item_text)
        .trim()
        .chars()
        .take(240)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::observe;

    fn analyze(document: &str, closed: &[GithubUpstream]) -> (DecisionGraph, BTreeSet<String>) {
        let observations = observe(document, &[]);
        let mut graph = DecisionGraph::default();
        graph.link_external_blockers(&observations, document);
        let settled = graph.settled_external_blocker_ids(closed);
        (graph, settled)
    }

    #[test]
    fn a_closed_linked_pull_request_settles_its_blocker_id() {
        let document = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#removehaivensoftware] Remove the sentence. ",
            "(stays open: blocked behind #unrealpr7; ",
            "https://github.com/haiven-dev/haiven-sdk-unreal/pull/7)\n",
            "<!-- /agent:backlog -->\n",
        );
        let pull = GithubUpstream::pull_request("haiven-dev", "haiven-sdk-unreal", 7);
        let (open_graph, open_settled) = analyze(document, &[]);
        assert!(open_settled.is_empty(), "{open_settled:?}");
        assert!(open_graph.blocker_upstreams().contains(&pull));
        let (_graph, settled) = analyze(document, &[pull]);
        assert!(settled.contains("unrealpr7"));
        assert!(!settled.contains("removehaivensoftware"));
    }

    #[test]
    fn prose_that_says_the_pull_request_is_closed_settles_without_a_lookup() {
        let document = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#removehaivensoftware] Remove the sentence. ",
            "(stays open: blocked behind #unrealpr7; ",
            "haiven-dev/haiven-sdk-unreal pull request 7 is closed)\n",
            "<!-- /agent:backlog -->\n",
        );
        let (_graph, settled) = analyze(document, &[]);
        assert!(settled.contains("unrealpr7"), "{settled:?}");
    }

    #[test]
    fn an_answered_blocker_id_settles_from_the_line_that_names_it() {
        let document = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#haivensdkunity] Set the licenses. ",
            "(stays open: blocked behind #bossroomcopyrightowner)\n",
            "<!-- /agent:backlog -->\n",
            "Since bossroomcopyrightowner is moot, the item is no longer blocked\n",
        );
        let (graph, settled) = analyze(document, &[]);
        assert!(settled.contains("bossroomcopyrightowner"), "{settled:?}");
        assert!(
            graph
                .nodes
                .contains_key("dec-blocker-bossroomcopyrightowner")
        );
        assert!(graph.nodes.contains_key("blocker:bossroomcopyrightowner"));
    }

    #[test]
    fn a_real_wait_with_no_decision_stays_open() {
        let document = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#lyra] Fab content. (stays open: blocked behind #lyrafabcontent)\n",
            "<!-- /agent:backlog -->\n",
        );
        let (_graph, settled) = analyze(document, &[]);
        assert!(settled.is_empty(), "{settled:?}");
    }

    #[test]
    fn the_marker_gains_settled_ids_and_keeps_ones_already_written() {
        let document = "<!-- agent-doc:blocker-settled unrealpr7 -->\nbody\n";
        let mut settled = BTreeSet::new();
        settled.insert("bossroomcopyrightowner".to_string());
        let next = merge_settlement_marker(document, &settled).expect("marker grows");
        let ids = marker_ids(&next);
        assert!(ids.contains("unrealpr7"));
        assert!(ids.contains("bossroomcopyrightowner"));
        assert!(merge_settlement_marker(&next, &settled).is_none());
    }

    #[test]
    fn a_pull_request_number_alone_is_not_a_blocker() {
        let document = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#bump] Land it. (stays open: blocked behind #749)\n",
            "<!-- /agent:backlog -->\n",
        );
        let (graph, settled) = analyze(document, &[]);
        assert!(settled.is_empty());
        assert!(!graph.nodes.keys().any(|id| id.contains("749")));
    }
}
