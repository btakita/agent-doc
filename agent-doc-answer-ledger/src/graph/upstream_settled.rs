//! Tracked work settled by an observed GitHub state (`#operatorshouldnneed`).
//!
//! The decision graph already joins a pull request or issue to the todo or
//! backlog item it closes (`upstream_of`, clause [`CLOSES_CLAUSE`]), whichever
//! side was written first. What it lacked was the artifact's *state*: an
//! operator had to ask for an `#audit-stale` pass, and an agent then checked
//! every cited pull request against GitHub by hand.
//!
//! The state is an external observation the caller supplies (a cache of
//! `gh` lookups the background maintenance refreshes). This module is pure:
//!
//! - [`DecisionGraph::work_upstreams`] lists, per open tracked item, the
//!   artifacts it is waiting on: the graph's `closes` edges for todo and
//!   backlog rows, and the URLs a review or icebox row's own text cites.
//!   [`DecisionGraph::work_upstream_set`] adds the queue heads and done trees
//!   (`upstream_watch`) and is what the background observer keeps fresh.
//! - [`DecisionGraph::work_settled_by_upstream`] returns the open items every
//!   such artifact of which is observed terminal (merged, or closed), so the
//!   item is very likely done or moot. One still-open or unobserved artifact
//!   keeps the item off the list, and so does a live queue row or busy
//!   waiting tree that still names it.
//!
//! Findings are flag-only: a cited pull request can be the work itself or
//! only a prerequisite, so the agent verifies before striking.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::DecisionGraph;
use super::upstream::{CLOSES_CLAUSE, GithubUpstream, GithubUpstreamKind};
use super::upstream::{explicit_upstreams, upstream_from_node_id};
use crate::EdgeKind;
use crate::extract::{Observations, TrackedComponent};

/// GitHub state of one issue or pull request, as last observed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamState {
    Open,
    /// A pull request that was merged.
    Merged,
    /// An issue that was closed, or a pull request closed without merging.
    Closed,
}

impl UpstreamState {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Merged => "merged",
            Self::Closed => "closed",
        }
    }

    /// Merged and closed never reopen on their own; a terminal observation
    /// needs no refresh.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Merged | Self::Closed)
    }

    /// Parse `gh ... --json state` output (`OPEN`, `MERGED`, `CLOSED`, any
    /// case, bare or inside the JSON object).
    pub fn parse_gh_state(text: &str) -> Option<Self> {
        let lower = text.to_ascii_lowercase();
        if lower.contains("merged") {
            Some(Self::Merged)
        } else if lower.contains("closed") {
            Some(Self::Closed)
        } else if lower.contains("open") {
            Some(Self::Open)
        } else {
            None
        }
    }
}

/// Why the open item is settled: every artifact it waits on is terminal.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UpstreamSettledKind {
    /// At least one cited pull request merged; the rest are terminal too.
    /// The item is likely DONE.
    UpstreamMerged,
    /// Every cited artifact is closed and none merged. The item is likely
    /// MOOT (or done, for a closed issue).
    UpstreamClosed,
}

impl UpstreamSettledKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::UpstreamMerged => "upstream_merged",
            Self::UpstreamClosed => "upstream_closed",
        }
    }
}

/// One open tracked item whose every cited artifact is observed terminal.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct UpstreamSettledItem {
    pub item_id: String,
    pub component: TrackedComponent,
    pub kind: UpstreamSettledKind,
    /// Each artifact with its observed state, sorted.
    pub upstreams: Vec<(GithubUpstream, UpstreamState)>,
    pub action: String,
}

/// The artifacts one open tracked item waits on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkUpstreams {
    pub item_id: String,
    pub component: TrackedComponent,
    pub upstreams: BTreeSet<GithubUpstream>,
}

impl DecisionGraph {
    /// Per open tracked item, the GitHub artifacts it waits on: incoming
    /// `upstream_of` edges with clause `closes` (todo and backlog rows,
    /// recorded by [`DecisionGraph::link_closing_upstreams`]) plus every
    /// explicit URL the item's own text cites (all four components). Items
    /// with none are omitted. Sorted by item id.
    pub fn work_upstreams(&self, observations: &Observations) -> Vec<WorkUpstreams> {
        let mut out: BTreeMap<String, WorkUpstreams> = BTreeMap::new();
        for item in observations.tracked.iter().filter(|item| item.open) {
            let mut upstreams: BTreeSet<GithubUpstream> =
                explicit_upstreams(&item.text).into_iter().collect();
            if matches!(
                item.component,
                TrackedComponent::Todo | TrackedComponent::Backlog
            ) {
                upstreams.extend(
                    self.incoming(&item.id, EdgeKind::UpstreamOf)
                        .filter(|edge| edge.clause.as_deref() == Some(CLOSES_CLAUSE))
                        .filter_map(|edge| upstream_from_node_id(&edge.from)),
                );
            }
            if upstreams.is_empty() {
                continue;
            }
            out.entry(item.id.clone())
                .and_modify(|entry| entry.upstreams.extend(upstreams.iter().cloned()))
                .or_insert(WorkUpstreams {
                    item_id: item.id.clone(),
                    component: item.component,
                    upstreams,
                });
        }
        out.into_values().collect()
    }

    /// Every artifact the document's work cites: what the background
    /// observer keeps fresh. The artifacts open tracked items wait on, those
    /// live queue heads cite ([`DecisionGraph::queue_head_upstreams`]), and
    /// the pull requests the kept done entries cite
    /// ([`DecisionGraph::done_tree_upstreams`]).
    pub fn work_upstream_set(&self, observations: &Observations) -> BTreeSet<GithubUpstream> {
        let mut set = self.live_work_upstream_set(observations);
        set.extend(
            self.done_tree_upstreams(observations)
                .into_iter()
                .flat_map(|tree| tree.upstreams),
        );
        set
    }

    /// Open tracked items whose every artifact `state` reports terminal.
    /// An artifact `state` does not know (never observed, or the lookup
    /// failed) keeps its item open, as does a still-open one. An item a live
    /// queue row or a busy waiting tree still names is being worked and is
    /// skipped. Read-only; sorted by item id.
    pub fn work_settled_by_upstream(
        &self,
        observations: &Observations,
        state: &dyn Fn(&GithubUpstream) -> Option<UpstreamState>,
    ) -> Vec<UpstreamSettledItem> {
        let mut out = Vec::new();
        for work in self.work_upstreams(observations) {
            if observations.live_queue_ids.contains(&work.item_id)
                || observations.busy_waiting_ids.contains(&work.item_id)
            {
                continue;
            }
            let mut observed = Vec::new();
            for upstream in &work.upstreams {
                match state(upstream) {
                    Some(found) if found.is_terminal() => observed.push((upstream.clone(), found)),
                    _ => {
                        observed.clear();
                        break;
                    }
                }
            }
            if observed.is_empty() {
                continue;
            }
            let kind = if observed
                .iter()
                .any(|(_, found)| *found == UpstreamState::Merged)
            {
                UpstreamSettledKind::UpstreamMerged
            } else {
                UpstreamSettledKind::UpstreamClosed
            };
            out.push(UpstreamSettledItem {
                action: settled_instruction(&work, kind, &observed),
                item_id: work.item_id,
                component: work.component,
                kind,
                upstreams: observed,
            });
        }
        out
    }
}

fn settled_instruction(
    work: &WorkUpstreams,
    kind: UpstreamSettledKind,
    observed: &[(GithubUpstream, UpstreamState)],
) -> String {
    let listed = observed
        .iter()
        .map(|(upstream, found)| format!("{} ({})", upstream.url(), found.as_str()))
        .collect::<Vec<_>>()
        .join(", ");
    let verdict = match kind {
        UpstreamSettledKind::UpstreamMerged => "likely DONE",
        UpstreamSettledKind::UpstreamClosed => "likely MOOT",
    };
    let flag = match work.component {
        TrackedComponent::Todo => "`--todo-done`",
        TrackedComponent::Backlog => "`--done`",
        TrackedComponent::Review => "`--review-resolve`",
        TrackedComponent::Icebox => "",
    };
    let close = if flag.is_empty() {
        "decide whether to drop it from the icebox".to_string()
    } else {
        format!("close it with {flag} in this closeout and cite the artifact")
    };
    let noun = if observed
        .iter()
        .all(|(upstream, _)| upstream.kind == GithubUpstreamKind::Issue)
    {
        "issue"
    } else {
        "pull request"
    };
    format!(
        "The open {} item [#{id}] is {verdict}: every GitHub artifact it waits on is terminal: {listed} (#operatorshouldnneed). If the {noun} was this item's work, {close}; if it only unblocked the item, do the remaining work or rewrite the item. Do not wait for an `#audit-stale` request.",
        work.component.as_str(),
        id = work.item_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::TrackedItemObs;
    use crate::graph::KnownUpstream;

    fn item(component: TrackedComponent, id: &str, text: &str) -> TrackedItemObs {
        TrackedItemObs {
            component,
            id: id.to_string(),
            text: text.to_string(),
            open: true,
        }
    }

    fn observations(items: Vec<TrackedItemObs>) -> Observations {
        Observations {
            tracked: items,
            ..Observations::default()
        }
    }

    fn states(
        known: &[(GithubUpstream, UpstreamState)],
    ) -> impl Fn(&GithubUpstream) -> Option<UpstreamState> + '_ {
        move |upstream| {
            known
                .iter()
                .find(|(candidate, _)| candidate == upstream)
                .map(|(_, state)| *state)
        }
    }

    #[test]
    fn gh_state_output_parses_in_any_case() {
        assert_eq!(
            UpstreamState::parse_gh_state("{\"state\":\"MERGED\"}"),
            Some(UpstreamState::Merged)
        );
        assert_eq!(
            UpstreamState::parse_gh_state("CLOSED\n"),
            Some(UpstreamState::Closed)
        );
        assert_eq!(
            UpstreamState::parse_gh_state("{\"state\":\"OPEN\"}"),
            Some(UpstreamState::Open)
        );
        assert_eq!(UpstreamState::parse_gh_state("{}"), None);
        assert!(UpstreamState::Merged.is_terminal());
        assert!(!UpstreamState::Open.is_terminal());
    }

    #[test]
    fn a_merged_pull_request_settles_the_backlog_item_that_cites_it() {
        let pull = GithubUpstream::pull_request("btakita", "agent-doc", 12);
        let obs = observations(vec![item(
            TrackedComponent::Backlog,
            "fixit",
            "Fix it https://github.com/btakita/agent-doc/pull/12",
        )]);
        let mut graph = DecisionGraph::default();
        graph.link_closing_upstreams(&obs, &[]);
        let known = [(pull.clone(), UpstreamState::Merged)];
        let settled = graph.work_settled_by_upstream(&obs, &states(&known));
        assert_eq!(settled.len(), 1);
        assert_eq!(settled[0].item_id, "fixit");
        assert_eq!(settled[0].kind, UpstreamSettledKind::UpstreamMerged);
        assert!(
            settled[0].action.contains("`--done`"),
            "{}",
            settled[0].action
        );
        assert!(settled[0].action.contains(&pull.url()));
    }

    #[test]
    fn an_open_or_unobserved_artifact_keeps_the_item_open() {
        let merged = GithubUpstream::pull_request("o", "r", 1);
        let open = GithubUpstream::pull_request("o", "r", 2);
        let obs = observations(vec![
            item(
                TrackedComponent::Todo,
                "both",
                "https://github.com/o/r/pull/1 and https://github.com/o/r/pull/2",
            ),
            item(
                TrackedComponent::Todo,
                "unseen",
                "https://github.com/o/r/pull/3",
            ),
        ]);
        let graph = DecisionGraph::default();
        let known = [(merged, UpstreamState::Merged), (open, UpstreamState::Open)];
        assert!(
            graph
                .work_settled_by_upstream(&obs, &states(&known))
                .is_empty()
        );
    }

    #[test]
    fn closed_without_merge_is_moot_and_one_merge_makes_it_done() {
        let closed = GithubUpstream::pull_request("o", "r", 1);
        let merged = GithubUpstream::pull_request("o", "r", 2);
        let obs = observations(vec![
            item(
                TrackedComponent::Review,
                "abandoned",
                "https://github.com/o/r/pull/1",
            ),
            item(
                TrackedComponent::Backlog,
                "superseded",
                "https://github.com/o/r/pull/1 then https://github.com/o/r/pull/2",
            ),
        ]);
        let graph = DecisionGraph::default();
        let known = [
            (closed, UpstreamState::Closed),
            (merged, UpstreamState::Merged),
        ];
        let settled = graph.work_settled_by_upstream(&obs, &states(&known));
        let kinds: Vec<_> = settled
            .iter()
            .map(|found| (found.item_id.as_str(), found.kind))
            .collect();
        assert_eq!(
            kinds,
            vec![
                ("abandoned", UpstreamSettledKind::UpstreamClosed),
                ("superseded", UpstreamSettledKind::UpstreamMerged),
            ]
        );
        assert!(settled[0].action.contains("`--review-resolve`"));
    }

    #[test]
    fn a_closing_edge_from_document_text_counts_without_a_url_on_the_item() {
        let pull = GithubUpstream::pull_request("haiven-dev", "haiven-api", 852);
        let obs = observations(vec![item(
            TrackedComponent::Todo,
            "publishtelemetry",
            "Publish telemetry",
        )]);
        let mut graph = DecisionGraph::default();
        graph.link_closing_upstreams(
            &obs,
            &[KnownUpstream {
                upstream: pull.clone(),
                text:
                    "https://github.com/haiven-dev/haiven-api/pull/852 closes [#publishtelemetry]"
                        .to_string(),
                in_document: true,
            }],
        );
        assert_eq!(
            graph
                .work_upstream_set(&obs)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![pull.clone()]
        );
        let known = [(pull, UpstreamState::Merged)];
        let settled = graph.work_settled_by_upstream(&obs, &states(&known));
        assert_eq!(settled.len(), 1);
        assert!(settled[0].action.contains("`--todo-done`"));
    }

    #[test]
    fn live_queue_work_closed_items_and_uncited_items_are_skipped() {
        let pull = GithubUpstream::pull_request("o", "r", 1);
        let mut closed = item(
            TrackedComponent::Todo,
            "closed",
            "https://github.com/o/r/pull/1",
        );
        closed.open = false;
        let mut obs = observations(vec![
            item(
                TrackedComponent::Todo,
                "queued",
                "https://github.com/o/r/pull/1",
            ),
            closed,
            item(TrackedComponent::Todo, "plain", "no artifact here"),
        ]);
        obs.live_queue_ids.insert("queued".to_string());
        let graph = DecisionGraph::default();
        let known = [(pull.clone(), UpstreamState::Merged)];
        assert!(
            graph
                .work_settled_by_upstream(&obs, &states(&known))
                .is_empty()
        );
        assert_eq!(
            graph
                .work_upstream_set(&obs)
                .into_iter()
                .collect::<Vec<_>>(),
            vec![pull],
            "the queued item's artifact is still observed"
        );
    }

    #[test]
    fn an_icebox_item_is_only_flagged() {
        let issue = GithubUpstream::issue("o", "r", 9);
        let obs = observations(vec![item(
            TrackedComponent::Icebox,
            "someday",
            "https://github.com/o/r/issues/9",
        )]);
        let known = [(issue, UpstreamState::Closed)];
        let settled = DecisionGraph::default().work_settled_by_upstream(&obs, &states(&known));
        assert_eq!(settled.len(), 1);
        assert!(settled[0].action.contains("icebox"));
        assert!(settled[0].action.contains("If the issue"));
    }
}
