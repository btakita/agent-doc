//! GitHub issue and pull-request causality on the decision graph
//! (`#decisiongraph`).
//!
//! A cited issue or pull request is an evidence node. `upstream_of` records
//! that it is upstream of a decision: the decision exists because the
//! upstream is open. Closing the upstream moots that decision. There is no
//! second causality graph.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{DecisionGraph, EdgeSource, GraphEdge, Node, NodeKind};
use crate::extract::Observations;
use crate::{EdgeKind, text};

/// Issue or pull request. GitHub shares one number space, but the URL path
/// distinguishes them, and a bare `issue N` does not cite a pull request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GithubUpstreamKind {
    Issue,
    PullRequest,
}

impl GithubUpstreamKind {
    const fn path(self) -> &'static str {
        match self {
            Self::Issue => "issues",
            Self::PullRequest => "pull",
        }
    }
}

/// One GitHub issue or pull request, normalized to lowercase owner and repo.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GithubUpstream {
    pub owner: String,
    pub repo: String,
    pub kind: GithubUpstreamKind,
    pub number: u64,
}

impl GithubUpstream {
    pub fn issue(owner: &str, repo: &str, number: u64) -> Self {
        Self {
            owner: owner.to_ascii_lowercase(),
            repo: repo.to_ascii_lowercase(),
            kind: GithubUpstreamKind::Issue,
            number,
        }
    }

    pub fn pull_request(owner: &str, repo: &str, number: u64) -> Self {
        Self {
            owner: owner.to_ascii_lowercase(),
            repo: repo.to_ascii_lowercase(),
            kind: GithubUpstreamKind::PullRequest,
            number,
        }
    }

    /// Evidence-node id on the decision graph.
    pub fn node_id(&self) -> String {
        format!(
            "github:{}/{}/{}/{}",
            self.owner,
            self.repo,
            self.kind.path(),
            self.number
        )
    }

    pub fn url(&self) -> String {
        format!(
            "https://github.com/{}/{}/{}/{}",
            self.owner,
            self.repo,
            self.kind.path(),
            self.number
        )
    }
}

/// A decision mooted because an upstream it cites is closed.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MootDecision {
    pub decision_id: String,
    pub upstream: GithubUpstream,
}

impl DecisionGraph {
    /// Record `upstream_of` for citations of `closed`, then return every open
    /// decision that cites one of them. A decision that does not cite a
    /// closed upstream is left alone.
    pub fn moot_by_closed_upstream(
        &mut self,
        observations: &Observations,
        closed: &[GithubUpstream],
    ) -> Vec<MootDecision> {
        link(self, observations, closed);
        let closed_ids: BTreeSet<String> = closed.iter().map(GithubUpstream::node_id).collect();
        let mut out = Vec::new();
        for edge in &self.edges {
            if edge.kind != EdgeKind::UpstreamOf || !closed_ids.contains(&edge.from) {
                continue;
            }
            let Some(node) = self.nodes.get(&edge.to) else {
                continue;
            };
            if node.kind != NodeKind::Decision || node.open != Some(true) {
                continue;
            }
            let Some(upstream) = closed
                .iter()
                .find(|upstream| upstream.node_id() == edge.from)
            else {
                continue;
            };
            out.push(MootDecision {
                decision_id: edge.to.clone(),
                upstream: upstream.clone(),
            });
        }
        out.sort();
        out.dedup();
        out
    }
}

/// Project citations onto `upstream_of` edges. `closed` binds a bare
/// `issue N` / `PR N` when exactly one closed upstream has that kind and
/// number and the decision does not already name a repo for it. Explicit
/// URLs are linked even when `closed` is empty.
pub(super) fn link(
    graph: &mut DecisionGraph,
    observations: &Observations,
    closed: &[GithubUpstream],
) {
    let mut document_explicit = Vec::new();
    for question in &observations.questions {
        document_explicit.extend(explicit_upstreams(&question.text));
    }
    document_explicit.sort();
    document_explicit.dedup();

    let mut texts: BTreeMap<String, String> = BTreeMap::new();
    for question in &observations.questions {
        let Some(id) = question.id.as_deref() else {
            continue;
        };
        let Some(decision) = decision_of_question(graph, id) else {
            continue;
        };
        let entry = texts.entry(decision).or_default();
        entry.push('\n');
        entry.push_str(&question.text);
    }

    for (decision, text) in &texts {
        for upstream in citations(text, &document_explicit, closed) {
            let id = upstream.node_id();
            graph.node(Node {
                id: id.clone(),
                kind: NodeKind::Evidence,
                label: upstream.url(),
                open: None,
                location: None,
                first_seen_at: None,
                in_document: true,
            });
            graph.edge(GraphEdge {
                from: id,
                kind: EdgeKind::UpstreamOf,
                to: decision.clone(),
                source: EdgeSource::Derived,
                // The premise is that the upstream is still open.
                clause: Some("open".to_string()),
            });
        }
    }
}

fn decision_of_question(graph: &DecisionGraph, id: &str) -> Option<String> {
    graph
        .outgoing(id, EdgeKind::CopyOf)
        .next()
        .map(|edge| edge.to.clone())
        .filter(|decision| {
            graph
                .nodes
                .get(decision)
                .is_some_and(|node| node.kind == NodeKind::Decision)
        })
}

/// Upstreams `text` cites.
///
/// A `github.com/<owner>/<repo>/issues|pull/<n>` URL cites that artifact.
/// `owner/repo#<n>` cites every known upstream with that owner, repo, and
/// number. A bare `issue <n>` or `PR <n>` cites an upstream only when the
/// text does not name a different repo for that number and exactly one
/// explicit URL in the document, or else exactly one entry of `closed`,
/// has that kind and number.
fn citations(
    text: &str,
    document_explicit: &[GithubUpstream],
    closed: &[GithubUpstream],
) -> Vec<GithubUpstream> {
    let mut out = explicit_upstreams(text);
    let known = || document_explicit.iter().chain(closed.iter());
    for (owner, repo, number) in shorthand_refs(text) {
        let mut matches: Vec<GithubUpstream> = known()
            .filter(|upstream| {
                upstream.owner == owner && upstream.repo == repo && upstream.number == number
            })
            .cloned()
            .collect();
        matches.sort();
        matches.dedup();
        out.extend(matches);
    }
    for (kind, number) in bare_mentions(text) {
        let named_here: Vec<GithubUpstream> = explicit_upstreams(text)
            .into_iter()
            .filter(|upstream| upstream.kind == kind && upstream.number == number)
            .collect();
        if !named_here.is_empty() {
            out.extend(named_here);
            continue;
        }
        let mut in_document: Vec<GithubUpstream> = document_explicit
            .iter()
            .filter(|upstream| upstream.kind == kind && upstream.number == number)
            .cloned()
            .collect();
        in_document.sort();
        in_document.dedup();
        if in_document.len() == 1 {
            out.extend(in_document);
            continue;
        }
        if !in_document.is_empty() {
            continue;
        }
        let mut in_closed: Vec<GithubUpstream> = closed
            .iter()
            .filter(|upstream| upstream.kind == kind && upstream.number == number)
            .cloned()
            .collect();
        in_closed.sort();
        in_closed.dedup();
        if in_closed.len() == 1 {
            out.extend(in_closed);
        }
    }
    out.sort();
    out.dedup();
    out
}

fn explicit_upstreams(text: &str) -> Vec<GithubUpstream> {
    let lower = text.to_ascii_lowercase();
    let mut out = Vec::new();
    let mut rest = lower.as_str();
    while let Some(at) = rest.find("github.com/") {
        let after_host = &rest[at + "github.com/".len()..];
        if let Some(parsed) = parse_github_path(after_host) {
            out.push(parsed.upstream);
            rest = parsed.rest;
        } else if after_host.is_empty() {
            break;
        } else {
            rest = &after_host[1..];
        }
    }
    out.sort();
    out.dedup();
    out
}

struct Parsed<'a> {
    upstream: GithubUpstream,
    rest: &'a str,
}

fn parse_github_path(text: &str) -> Option<Parsed<'_>> {
    let (owner, rest) = take_name(text)?;
    let rest = rest.strip_prefix('/')?;
    let (repo, rest) = take_name(rest)?;
    let rest = rest.strip_prefix('/')?;
    let (kind, rest) = if let Some(rest) = rest.strip_prefix("issues/") {
        (GithubUpstreamKind::Issue, rest)
    } else if let Some(rest) = rest.strip_prefix("pull/") {
        (GithubUpstreamKind::PullRequest, rest)
    } else if let Some(rest) = rest.strip_prefix("pulls/") {
        (GithubUpstreamKind::PullRequest, rest)
    } else {
        return None;
    };
    let (number, rest) = take_number(rest)?;
    Some(Parsed {
        upstream: GithubUpstream {
            owner,
            repo,
            kind,
            number,
        },
        rest,
    })
}

fn shorthand_refs(text: &str) -> Vec<(String, String, u64)> {
    let lower = text.to_ascii_lowercase();
    let chars: Vec<char> = lower.chars().collect();
    let mut out = Vec::new();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '#' {
            index += 1;
            continue;
        }
        let number_start = index + 1;
        let mut number_end = number_start;
        while number_end < chars.len() && chars[number_end].is_ascii_digit() {
            number_end += 1;
        }
        let Some(number) = number_from(&chars[number_start..number_end]) else {
            index += 1;
            continue;
        };
        let Some((owner, repo)) = owner_repo_before(&chars, index) else {
            index = number_end;
            continue;
        };
        out.push((owner, repo, number));
        index = number_end;
    }
    out
}

fn owner_repo_before(chars: &[char], hash: usize) -> Option<(String, String)> {
    if hash == 0 {
        return None;
    }
    let repo_end = hash;
    let mut repo_start = repo_end;
    while repo_start > 0 && is_name_char(chars[repo_start - 1]) {
        repo_start -= 1;
    }
    if repo_start == repo_end || repo_start == 0 || chars[repo_start - 1] != '/' {
        return None;
    }
    let owner_end = repo_start - 1;
    let mut owner_start = owner_end;
    while owner_start > 0 && is_name_char(chars[owner_start - 1]) {
        owner_start -= 1;
    }
    if owner_start == owner_end {
        return None;
    }
    if owner_start > 0 && is_name_char(chars[owner_start - 1]) {
        return None;
    }
    let owner: String = chars[owner_start..owner_end].iter().collect();
    let repo: String = chars[repo_start..repo_end].iter().collect();
    (owner != "github.com" && !owner.is_empty() && !repo.is_empty()).then_some((owner, repo))
}

fn bare_mentions(text: &str) -> Vec<(GithubUpstreamKind, u64)> {
    let words = text::tokens(text);
    let mut out = Vec::new();
    for (index, word) in words.iter().enumerate() {
        let (kind, number_at) = match word.as_str() {
            "issue" | "issues" => (GithubUpstreamKind::Issue, index + 1),
            "pr" => (GithubUpstreamKind::PullRequest, index + 1),
            "pull" if words.get(index + 1).map(String::as_str) == Some("request") => {
                (GithubUpstreamKind::PullRequest, index + 2)
            }
            "pull" => (GithubUpstreamKind::PullRequest, index + 1),
            _ => continue,
        };
        let Some(number) = words.get(number_at).and_then(|word| word.parse().ok()) else {
            continue;
        };
        if number == 0 {
            continue;
        }
        out.push((kind, number));
    }
    out.sort();
    out.dedup();
    out
}

fn take_name(text: &str) -> Option<(String, &str)> {
    let len = text.bytes().take_while(|byte| is_name_byte(*byte)).count();
    if len == 0 {
        return None;
    }
    Some((text[..len].to_string(), &text[len..]))
}

fn take_number(text: &str) -> Option<(u64, &str)> {
    let len = text.bytes().take_while(u8::is_ascii_digit).count();
    let number = text[..len].parse().ok()?;
    (number > 0).then_some((number, &text[len..]))
}

fn number_from(chars: &[char]) -> Option<u64> {
    if chars.is_empty() {
        return None;
    }
    let text: String = chars.iter().collect();
    let number = text.parse().ok()?;
    (number > 0).then_some(number)
}

fn is_name_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.'
}

fn is_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'-' || byte == b'_' || byte == b'.'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn urls_and_bare_mentions_cite_the_named_upstream() {
        let issue = GithubUpstream::issue("btakita", "agent-doc", 395);
        let pull = GithubUpstream::pull_request("btakita", "agent-doc", 12);
        let text = "see https://github.com/btakita/agent-doc/issues/395#issuecomment-1 and PR #12";
        let cited = citations(text, &[], &[]);
        assert_eq!(cited, vec![issue.clone()]);
        let cited = citations(text, &[], &[pull.clone()]);
        assert!(cited.contains(&pull));
        let alien = GithubUpstream::issue("other", "repo", 395);
        let bare = "choices: Wait on issue 395";
        assert!(citations(bare, &[], &[issue.clone(), alien]).is_empty());
        assert_eq!(citations(bare, &[], &[issue.clone()]), vec![issue]);
    }
}
