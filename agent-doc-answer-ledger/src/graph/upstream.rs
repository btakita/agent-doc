//! GitHub issue and pull-request causality on the decision graph
//! (`#decisiongraph`).
//!
//! A cited issue or pull request is an evidence node. `upstream_of` records
//! that it is upstream of a decision: the decision exists because the
//! upstream is open. Closing the upstream moots that decision.
//!
//! The same edge, with clause [`CLOSES_CLAUSE`], records that the artifact
//! closes a todo or backlog item. The item does not become a decision, and
//! the document is not rewritten. The association is recorded from either
//! side: the item's text cites the artifact, or the artifact's text (or a
//! document line that names it) cites the item. Whichever side appears
//! second still joins, including across [`DecisionGraph::merge_prior`]. An
//! open item that does not yet name the artifact yields an annotation
//! instruction for the agent.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::{DecisionGraph, EdgeSource, GraphEdge, Node, NodeKind};
use crate::extract::{Observations, TrackedComponent, TrackedItemObs, bracket_ids};
use crate::{EdgeKind, text};

/// Clause on `upstream_of` when the artifact closes a todo or backlog item.
/// Decision citations keep clause `open` and are what
/// [`DecisionGraph::moot_by_closed_upstream`] returns.
pub const CLOSES_CLAUSE: &str = "closes";

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

/// Pull request or issue text the caller already has. `in_document` is false
/// when the text is a pull-request body that the session document does not
/// contain.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownUpstream {
    pub upstream: GithubUpstream,
    pub text: String,
    pub in_document: bool,
}

/// An open todo or backlog item the agent should annotate with the artifact
/// that closes it. The graph records the edge either way; this is only the
/// missing visible citation.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorkUpstreamAnnotation {
    pub item_id: String,
    pub component: TrackedComponent,
    pub upstream: GithubUpstream,
    pub instruction: String,
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
            // A todo/backlog close is not a decision to moot.
            if edge.clause.as_deref() == Some(CLOSES_CLAUSE) {
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

    /// Record `upstream_of` (clause [`CLOSES_CLAUSE`]) from a pull request or
    /// issue to the todo or backlog item it closes.
    ///
    /// Either observation is enough, and the later one still joins:
    /// - the tracked item's own text cites the artifact (the item was written
    ///   while the pull request already existed, or it was annotated);
    /// - `known` text cites the item id (the pull request was written while
    ///   the item already existed, or the item is added later and a prior
    ///   call's edge is restored with [`DecisionGraph::merge_prior`]).
    ///
    /// An id that is not a tracked item yet is a placeholder item node so the
    /// edge survives until the item appears. Review and icebox rows are not
    /// associated. Does not rewrite the document.
    pub fn link_closing_upstreams(&mut self, observations: &Observations, known: &[KnownUpstream]) {
        link_work(self, observations, known);
        self.finish();
    }

    /// Open todo and backlog items associated with a closing artifact whose
    /// own text does not yet cite it. Each [`WorkUpstreamAnnotation::instruction`]
    /// tells the agent to add the URL. Already-cited items stay associated
    /// and are not returned.
    pub fn work_upstream_annotations(
        &self,
        observations: &Observations,
    ) -> Vec<WorkUpstreamAnnotation> {
        let mut out = Vec::new();
        for item in work_items(observations) {
            if !item.open {
                continue;
            }
            for edge in self.incoming(&item.id, EdgeKind::UpstreamOf) {
                if edge.clause.as_deref() != Some(CLOSES_CLAUSE) {
                    continue;
                }
                let Some(upstream) = upstream_from_node_id(&edge.from) else {
                    continue;
                };
                if item_names_upstream(&item.text, &upstream) {
                    continue;
                }
                out.push(WorkUpstreamAnnotation {
                    instruction: annotation_instruction(item, &upstream),
                    item_id: item.id.clone(),
                    component: item.component,
                    upstream,
                });
            }
        }
        out.sort();
        out.dedup();
        out
    }
}

/// Explicit `github.com/<owner>/<repo>/issues|pull|pulls/<n>` URLs in
/// `document`, with the item ids named on that line or on a following
/// non-list continuation line. The same artifact's texts are concatenated.
pub fn known_upstreams_in_document(document: &str) -> Vec<KnownUpstream> {
    let lines: Vec<&str> = document.lines().collect();
    let mut grouped: BTreeMap<GithubUpstream, String> = BTreeMap::new();
    for (index, line) in lines.iter().enumerate() {
        let urls = explicit_upstreams(line);
        if urls.is_empty() {
            continue;
        }
        let mut text = (*line).to_string();
        if let Some(next) = lines.get(index + 1)
            && continuation_cites_work(next)
        {
            text.push('\n');
            text.push_str(next);
        }
        for upstream in urls {
            let entry = grouped.entry(upstream).or_default();
            if !entry.is_empty() {
                entry.push('\n');
            }
            entry.push_str(&text);
        }
    }
    grouped
        .into_iter()
        .map(|(upstream, text)| KnownUpstream {
            upstream,
            text,
            in_document: true,
        })
        .collect()
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
    } else {
        let rest = rest.strip_prefix("pulls/")?;
        (GithubUpstreamKind::PullRequest, rest)
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

fn work_items(observations: &Observations) -> impl Iterator<Item = &TrackedItemObs> {
    observations.tracked.iter().filter(|item| {
        matches!(
            item.component,
            TrackedComponent::Todo | TrackedComponent::Backlog
        )
    })
}

/// Item self-citations and `known` texts. A cited id with no tracked row yet
/// is a placeholder so a later item joins the edge already stored.
pub(super) fn link_work(
    graph: &mut DecisionGraph,
    observations: &Observations,
    known: &[KnownUpstream],
) {
    let tracked: Vec<&TrackedItemObs> = work_items(observations).collect();
    for item in &tracked {
        graph.node(Node {
            id: item.id.clone(),
            kind: NodeKind::Item,
            label: item.text.clone(),
            open: Some(item.open),
            location: None,
            first_seen_at: None,
            in_document: true,
        });
        for upstream in explicit_upstreams(&item.text) {
            note_closes(graph, &upstream, &item.id, true);
        }
    }
    let tracked_ids: BTreeSet<&str> = tracked.iter().map(|item| item.id.as_str()).collect();
    for known in known {
        let mut cited = cited_work_ids(&known.text);
        cited.retain(|id| {
            tracked_ids.contains(id.as_str()) || !is_tracked_elsewhere(observations, id)
        });
        for id in cited {
            if !tracked_ids.contains(id.as_str()) {
                graph.node(Node {
                    id: id.clone(),
                    kind: NodeKind::Item,
                    label: String::new(),
                    open: None,
                    location: None,
                    first_seen_at: None,
                    in_document: false,
                });
            }
            note_closes(graph, &known.upstream, &id, known.in_document);
        }
    }
}

/// Review and icebox rows are tracked, but they are not this association.
fn is_tracked_elsewhere(observations: &Observations, id: &str) -> bool {
    observations.tracked.iter().any(|item| {
        item.id == id
            && !matches!(
                item.component,
                TrackedComponent::Todo | TrackedComponent::Backlog
            )
    })
}

fn note_closes(
    graph: &mut DecisionGraph,
    upstream: &GithubUpstream,
    item_id: &str,
    in_document: bool,
) {
    let id = upstream.node_id();
    graph.node(Node {
        id: id.clone(),
        kind: NodeKind::Evidence,
        label: upstream.url(),
        open: None,
        location: None,
        first_seen_at: None,
        in_document,
    });
    graph.edge(GraphEdge {
        from: id,
        kind: EdgeKind::UpstreamOf,
        to: item_id.to_string(),
        source: EdgeSource::Derived,
        clause: Some(CLOSES_CLAUSE.to_string()),
    });
}

fn annotation_instruction(item: &TrackedItemObs, upstream: &GithubUpstream) -> String {
    let noun = match upstream.kind {
        GithubUpstreamKind::PullRequest => "pull request",
        GithubUpstreamKind::Issue => "issue",
    };
    let flag = match item.component {
        TrackedComponent::Todo => "--todo-edit",
        TrackedComponent::Backlog => "--backlog-edit",
        TrackedComponent::Review | TrackedComponent::Icebox => "--backlog-edit",
    };
    format!(
        "Annotate the open {} item [#{id}] with the {noun} that closes it: {url}. Add that URL to the item text with {flag} in this closeout. Do not wait for the operator to ask, and do not check the item off only because the {noun} exists.",
        item.component.as_str(),
        id = item.id,
        url = upstream.url(),
    )
}

/// The item already shows this artifact: a `github.com/.../pull|issues/<n>`
/// URL or `owner/repo#<n>`. A bare `PR 852` does not.
fn item_names_upstream(text: &str, upstream: &GithubUpstream) -> bool {
    let lower = text.to_ascii_lowercase();
    let host = format!(
        "github.com/{}/{}/{}/{}",
        upstream.owner,
        upstream.repo,
        upstream.kind.path(),
        upstream.number
    );
    if lower.contains(&host) {
        return true;
    }
    if upstream.kind == GithubUpstreamKind::PullRequest {
        let pulls = format!(
            "github.com/{}/{}/pulls/{}",
            upstream.owner, upstream.repo, upstream.number
        );
        if lower.contains(&pulls) {
            return true;
        }
    }
    lower.contains(&format!(
        "{}/{}#{}",
        upstream.owner, upstream.repo, upstream.number
    ))
}

fn cited_work_ids(text: &str) -> Vec<String> {
    let mut out = bracket_ids(text);
    let chars: Vec<char> = text.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        if chars[index] != '#' || (index > 0 && chars[index - 1] == '[') {
            index += 1;
            continue;
        }
        let preceded = index == 0 || chars[index - 1].is_whitespace() || chars[index - 1] == '(';
        let start = index + 1;
        let mut end = start;
        while end < chars.len() && is_work_id_char(chars[end]) {
            end += 1;
        }
        if preceded && end > start {
            let id: String = chars[start..end]
                .iter()
                .flat_map(|ch| ch.to_lowercase())
                .collect();
            if id.chars().next().is_some_and(|ch| ch.is_ascii_alphabetic())
                && !id.starts_with("subagent-operator-request")
            {
                out.push(id);
            }
        }
        index = end.max(index + 1);
    }
    out.sort();
    out.dedup();
    out
}

fn is_work_id_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '-' || ch == '_'
}

fn continuation_cites_work(line: &str) -> bool {
    let trimmed = line.trim();
    if trimmed.is_empty()
        || trimmed.starts_with(['-', '*', '+', '#', '<', '>'])
        || !explicit_upstreams(trimmed).is_empty()
    {
        return false;
    }
    !cited_work_ids(trimmed).is_empty()
}

/// `github:{owner}/{repo}/issues|pull/{n}` back to the artifact. Anything else
/// is not a closing upstream this module recorded.
fn upstream_from_node_id(id: &str) -> Option<GithubUpstream> {
    let rest = id.strip_prefix("github:")?;
    let (owner, rest) = rest.split_once('/')?;
    let (repo, rest) = rest.split_once('/')?;
    let (kind, number) = rest.split_once('/')?;
    let kind = match kind {
        "issues" => GithubUpstreamKind::Issue,
        "pull" => GithubUpstreamKind::PullRequest,
        _ => return None,
    };
    let number = number.parse().ok()?;
    if number == 0 || owner.is_empty() || repo.is_empty() {
        return None;
    }
    Some(GithubUpstream {
        owner: owner.to_string(),
        repo: repo.to_string(),
        kind,
        number,
    })
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
        let cited = citations(text, &[], std::slice::from_ref(&pull));
        assert!(cited.contains(&pull));
        let alien = GithubUpstream::issue("other", "repo", 395);
        let bare = "choices: Wait on issue 395";
        assert!(citations(bare, &[], &[issue.clone(), alien]).is_empty());
        assert_eq!(
            citations(bare, &[], std::slice::from_ref(&issue)),
            vec![issue]
        );
    }

    #[test]
    fn document_lines_keep_a_citation_on_its_own_line() {
        let url = "https://github.com/haiven-dev/haiven-api/pull/852";
        let pull = GithubUpstream::pull_request("haiven-dev", "haiven-api", 852);
        let same_line = format!("Ship telemetry. {url} closes [#publishtelemetryannotati].");
        let known = known_upstreams_in_document(&same_line);
        assert_eq!(known.len(), 1);
        assert_eq!(known[0].upstream, pull);
        assert!(known[0].in_document);
        assert_eq!(
            cited_work_ids(&known[0].text),
            vec!["publishtelemetryannotati".to_string()]
        );

        let other = format!(
            "{url} closes [#publishtelemetryannotati].\n- [ ] [#otheritem] leave this alone\n"
        );
        let known = known_upstreams_in_document(&other);
        let ids = cited_work_ids(&known[0].text);
        assert_eq!(ids, vec!["publishtelemetryannotati".to_string()]);

        let wrapped = format!("{url}\ncloses [#publishtelemetryannotati]\n");
        let known = known_upstreams_in_document(&wrapped);
        assert_eq!(
            cited_work_ids(&known[0].text),
            vec!["publishtelemetryannotati".to_string()]
        );

        let checklist = format!("{url}\n- [ ] [#publishtelemetryannotati] not joined\n");
        let known = known_upstreams_in_document(&checklist);
        assert!(cited_work_ids(&known[0].text).is_empty(), "{known:?}");
    }

    #[test]
    fn cited_work_ids_skip_numbers_fragments_and_operator_requests() {
        let text = "see https://github.com/haiven-dev/haiven-api/pull/852#issuecomment-1 \
                    and haiven-dev/haiven-api#852 and #852 and (#publishtelemetryannotati)";
        assert_eq!(
            cited_work_ids(text),
            vec!["publishtelemetryannotati".to_string()]
        );
        assert!(cited_work_ids("#subagent-operator-request-prannotate").is_empty());
    }
}
