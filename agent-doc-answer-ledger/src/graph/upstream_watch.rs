//! The upstream watch beyond tracked items: queue heads and done trees
//! (`#operatorshouldnneed`).
//!
//! [`DecisionGraph::work_upstreams`] covers the todo, backlog, review and
//! icebox rows. Two more parts of a session document cite GitHub artifacts
//! and drift when those artifacts change state:
//!
//! - **Queue heads.** A live `agent:run` / `agent:queue` head, often a bare
//!   pull-request heading with `%% title %%` notes, stays queued after the
//!   pull request merged. [`DecisionGraph::queue_head_upstreams`] lists the
//!   artifacts each head's live tree cites (struck rows and their subtrees
//!   excluded), and [`DecisionGraph::queue_heads_settled_by_upstream`] flags a
//!   head every such artifact of which is terminal. A head a worker holds (an
//!   attempt handle on the row) is being worked and is not flagged.
//! - **Done trees.** A dated `agent:done` entry says its work is done.
//!   [`DecisionGraph::done_tree_upstreams`] lists the pull requests each entry
//!   cites, and [`DecisionGraph::done_trees_unmerged_by_upstream`] flags an
//!   entry every one of which was closed without merging: the work it records
//!   may never have landed. Issues are not watched here: a closed issue agrees
//!   with the entry, and an issue left open says nothing about whether the
//!   work landed. The caller bounds which entries are observed
//!   ([`crate::extract::Observations::retain_done_trees_since`]), so the
//!   archive's whole history is not looked up.
//!
//! [`DecisionGraph::work_upstream_set`] is the union of all three sources;
//! [`DecisionGraph::live_work_upstream_set`] leaves out the done trees so the
//! observer can refresh live work first. Every finding is flag-only.

use std::collections::BTreeSet;

use super::DecisionGraph;
use super::upstream::{GithubUpstream, GithubUpstreamKind, explicit_upstreams};
use super::upstream_settled::{UpstreamSettledKind, UpstreamState};
use crate::extract::Observations;

/// The artifacts one live queue head's tree cites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueueHeadUpstreams {
    pub queue: String,
    /// 1-based document line of the head row.
    pub line: usize,
    /// The head's `--ack-text` selector.
    pub text: String,
    pub ids: Vec<String>,
    /// A worker attempt handle is on the row.
    pub claimed: bool,
    pub upstreams: BTreeSet<GithubUpstream>,
}

/// The pull requests one done entry cites.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DoneTreeUpstreams {
    pub item_id: String,
    pub date: String,
    pub upstreams: BTreeSet<GithubUpstream>,
}

/// A live queue head every artifact of which is observed terminal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamSettledQueueHead {
    pub queue: String,
    pub line: usize,
    pub text: String,
    pub ids: Vec<String>,
    pub kind: UpstreamSettledKind,
    /// Each artifact with its observed state, sorted.
    pub upstreams: Vec<(GithubUpstream, UpstreamState)>,
    pub action: String,
}

/// A done entry every pull request of which was closed without merging.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UpstreamUnmergedDoneItem {
    pub item_id: String,
    pub date: String,
    pub upstreams: Vec<(GithubUpstream, UpstreamState)>,
    pub action: String,
}

impl DecisionGraph {
    /// Per live queue head, the GitHub artifacts its live tree cites. Heads
    /// citing none are omitted. In document order.
    pub fn queue_head_upstreams(&self, observations: &Observations) -> Vec<QueueHeadUpstreams> {
        observations
            .queue_heads
            .iter()
            .filter_map(|head| {
                let upstreams: BTreeSet<GithubUpstream> =
                    explicit_upstreams(&head.tree_text).into_iter().collect();
                (!upstreams.is_empty()).then(|| QueueHeadUpstreams {
                    queue: head.queue.clone(),
                    line: head.line,
                    text: head.text.clone(),
                    ids: head.ids.clone(),
                    claimed: head.claimed,
                    upstreams,
                })
            })
            .collect()
    }

    /// Per done entry the caller kept, the pull requests it cites. Entries
    /// citing none are omitted. Sorted by item id.
    pub fn done_tree_upstreams(&self, observations: &Observations) -> Vec<DoneTreeUpstreams> {
        let mut out: Vec<DoneTreeUpstreams> = observations
            .done_trees
            .iter()
            .filter_map(|tree| {
                let upstreams: BTreeSet<GithubUpstream> = explicit_upstreams(&tree.tree_text)
                    .into_iter()
                    .filter(|upstream| upstream.kind == GithubUpstreamKind::PullRequest)
                    .collect();
                (!upstreams.is_empty()).then(|| DoneTreeUpstreams {
                    item_id: tree.id.clone(),
                    date: tree.date.clone(),
                    upstreams,
                })
            })
            .collect();
        out.sort_by(|left, right| left.item_id.cmp(&right.item_id));
        out
    }

    /// The artifacts open tracked items and live queue heads wait on: the
    /// part of the watch set the observer refreshes first.
    pub fn live_work_upstream_set(&self, observations: &Observations) -> BTreeSet<GithubUpstream> {
        let mut set: BTreeSet<GithubUpstream> = self
            .work_upstreams(observations)
            .into_iter()
            .flat_map(|work| work.upstreams)
            .collect();
        set.extend(
            self.queue_head_upstreams(observations)
                .into_iter()
                .flat_map(|head| head.upstreams),
        );
        set
    }

    /// Live queue heads whose every cited artifact `state` reports terminal.
    /// An unknown or still-open artifact keeps the head unflagged, and so
    /// does a worker attempt handle on the row. Read-only; document order.
    pub fn queue_heads_settled_by_upstream(
        &self,
        observations: &Observations,
        state: &dyn Fn(&GithubUpstream) -> Option<UpstreamState>,
    ) -> Vec<UpstreamSettledQueueHead> {
        let mut out = Vec::new();
        for head in self.queue_head_upstreams(observations) {
            if head.claimed {
                continue;
            }
            let Some(observed) = all_terminal(&head.upstreams, state) else {
                continue;
            };
            let kind = settled_kind(&observed);
            out.push(UpstreamSettledQueueHead {
                action: queue_head_instruction(&head, kind, &observed),
                queue: head.queue,
                line: head.line,
                text: head.text,
                ids: head.ids,
                kind,
                upstreams: observed,
            });
        }
        out
    }

    /// Done entries whose every cited pull request `state` reports closed
    /// without merging. One merged, open or unknown pull request keeps the
    /// entry unflagged. Read-only; sorted by item id.
    pub fn done_trees_unmerged_by_upstream(
        &self,
        observations: &Observations,
        state: &dyn Fn(&GithubUpstream) -> Option<UpstreamState>,
    ) -> Vec<UpstreamUnmergedDoneItem> {
        let mut out = Vec::new();
        for tree in self.done_tree_upstreams(observations) {
            let Some(observed) = all_terminal(&tree.upstreams, state) else {
                continue;
            };
            if observed
                .iter()
                .any(|(_, found)| *found != UpstreamState::Closed)
            {
                continue;
            }
            out.push(UpstreamUnmergedDoneItem {
                action: done_tree_instruction(&tree, &observed),
                item_id: tree.item_id,
                date: tree.date,
                upstreams: observed,
            });
        }
        out
    }
}

/// Every artifact with its terminal state, or `None` when one is open or
/// unknown.
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

fn settled_kind(observed: &[(GithubUpstream, UpstreamState)]) -> UpstreamSettledKind {
    if observed
        .iter()
        .any(|(_, found)| *found == UpstreamState::Merged)
    {
        UpstreamSettledKind::UpstreamMerged
    } else {
        UpstreamSettledKind::UpstreamClosed
    }
}

fn listed(observed: &[(GithubUpstream, UpstreamState)]) -> String {
    observed
        .iter()
        .map(|(upstream, found)| format!("{} ({})", upstream.url(), found.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

fn shell_quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}

fn queue_head_instruction(
    head: &QueueHeadUpstreams,
    kind: UpstreamSettledKind,
    observed: &[(GithubUpstream, UpstreamState)],
) -> String {
    let verdict = match kind {
        UpstreamSettledKind::UpstreamMerged => "likely DONE",
        UpstreamSettledKind::UpstreamClosed => "likely MOOT",
    };
    let noun = if observed
        .iter()
        .all(|(upstream, _)| upstream.kind == GithubUpstreamKind::Issue)
    {
        "issue"
    } else {
        "pull request"
    };
    let item = head
        .ids
        .iter()
        .find(|id| !id.starts_with("q-") && !id.starts_with("r-"));
    let close = match item {
        Some(id) => format!(
            "close its item [#{id}] in this closeout with the flag the item needs, citing the artifact"
        ),
        None => format!(
            "strike it with `agent-doc queue consume <FILE> --ack-text {} --summary '<evidence citing the artifact>'`",
            shell_quote(&head.text)
        ),
    };
    format!(
        "The live queue head on line {line} of queue `{queue}` (“{text}”) is {verdict}: every GitHub artifact its live tree cites is terminal: {listed} (#operatorshouldnneed). `agent-doc queue tree-status <FILE> --line {line}` shows what the tree still holds. If the {noun} was this head's work, {close}; if it only unblocked the head, do the remaining work or rewrite the head. Do not wait for an `#audit-stale` request.",
        line = head.line,
        queue = head.queue,
        text = head.text,
        listed = listed(observed),
    )
}

fn done_tree_instruction(
    tree: &DoneTreeUpstreams,
    observed: &[(GithubUpstream, UpstreamState)],
) -> String {
    format!(
        "The done entry [#{id}] (archived {date}) cites only pull requests closed without merging: {listed} (#operatorshouldnneed). If one was this item's work and the work did not land through another change, reopen it with `agent-doc backlog <FILE> reopen {id}`; if it landed another way, the entry stands. Nothing is reopened automatically.",
        id = tree.item_id,
        date = tree.date,
        listed = listed(observed),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extract::{DoneTreeObs, QueueHeadObs, TrackedComponent, TrackedItemObs};

    fn head(line: usize, text: &str, tree: &str) -> QueueHeadObs {
        QueueHeadObs {
            queue: "prs".to_string(),
            line,
            text: text.to_string(),
            ids: crate::extract::bracket_ids(tree.lines().next().unwrap_or_default()),
            claimed: false,
            tree_text: tree.to_string(),
        }
    }

    fn done(id: &str, tree: &str) -> DoneTreeObs {
        DoneTreeObs {
            id: id.to_string(),
            date: "2026-11-20".to_string(),
            text: tree.to_string(),
            tree_text: tree.to_string(),
        }
    }

    fn pull(number: u64) -> GithubUpstream {
        GithubUpstream::pull_request("o", "r", number)
    }

    fn states(
        known: &[(u64, UpstreamState)],
    ) -> impl Fn(&GithubUpstream) -> Option<UpstreamState> + '_ {
        move |upstream| {
            known
                .iter()
                .find(|(number, _)| *number == upstream.number)
                .map(|(_, state)| *state)
        }
    }

    #[test]
    fn the_watch_set_covers_tracked_items_queue_heads_and_done_pull_requests() {
        let obs = Observations {
            tracked: vec![TrackedItemObs {
                component: TrackedComponent::Backlog,
                id: "fixit".to_string(),
                text: "https://github.com/o/r/pull/1".to_string(),
                open: true,
            }],
            queue_heads: vec![head(
                5,
                "https://github.com/o/r/pull/2",
                "- ### https://github.com/o/r/pull/2\n  - %% title %%\n",
            )],
            done_trees: vec![done(
                "shipped",
                "- 2026-11-20 [#shipped] https://github.com/o/r/pull/3 for https://github.com/o/r/issues/4\n",
            )],
            ..Observations::default()
        };
        let graph = DecisionGraph::default();
        let numbers = |set: BTreeSet<GithubUpstream>| -> Vec<u64> {
            set.into_iter().map(|upstream| upstream.number).collect()
        };
        assert_eq!(numbers(graph.work_upstream_set(&obs)), vec![1, 2, 3]);
        assert_eq!(numbers(graph.live_work_upstream_set(&obs)), vec![1, 2]);
    }

    #[test]
    fn a_merged_pull_request_heading_is_flagged_with_its_ack_text_strike() {
        let obs = Observations {
            queue_heads: vec![
                head(
                    7,
                    "https://github.com/o/r/pull/403",
                    "- ### https://github.com/o/r/pull/403\n  - %% ci: setup %%\n",
                ),
                head(
                    9,
                    "https://github.com/o/r/pull/404",
                    "- ### https://github.com/o/r/pull/404\n",
                ),
                head(
                    11,
                    "https://github.com/o/r/pull/405",
                    "- ### https://github.com/o/r/pull/405\n",
                ),
            ],
            ..Observations::default()
        };
        let known = [
            (403, UpstreamState::Merged),
            (404, UpstreamState::Open),
            (405, UpstreamState::Closed),
        ];
        let flagged =
            DecisionGraph::default().queue_heads_settled_by_upstream(&obs, &states(&known));
        let found: Vec<(usize, UpstreamSettledKind)> =
            flagged.iter().map(|head| (head.line, head.kind)).collect();
        assert_eq!(
            found,
            vec![
                (7, UpstreamSettledKind::UpstreamMerged),
                (11, UpstreamSettledKind::UpstreamClosed),
            ]
        );
        let action = &flagged[0].action;
        assert!(
            action.contains("--ack-text 'https://github.com/o/r/pull/403'"),
            "{action}"
        );
        assert!(action.contains("tree-status <FILE> --line 7"), "{action}");
        assert_eq!(
            flagged[0].upstreams,
            vec![(pull(403), UpstreamState::Merged)]
        );
    }

    #[test]
    fn an_open_child_artifact_a_claim_or_an_id_backed_head_changes_the_verdict() {
        let mut claimed = head(
            3,
            "https://github.com/o/r/pull/2",
            "- 🚧 [#subagent:x#1] https://github.com/o/r/pull/2\n",
        );
        claimed.claimed = true;
        let obs = Observations {
            queue_heads: vec![
                head(
                    1,
                    "https://github.com/o/r/pull/1",
                    "- ### https://github.com/o/r/pull/1\n  - follow-up https://github.com/o/r/pull/9\n",
                ),
                claimed,
                head(
                    5,
                    "do [#shipit] https://github.com/o/r/pull/3",
                    "- do [#shipit] https://github.com/o/r/pull/3\n",
                ),
            ],
            ..Observations::default()
        };
        let known = [
            (1, UpstreamState::Merged),
            (2, UpstreamState::Merged),
            (3, UpstreamState::Merged),
            (9, UpstreamState::Open),
        ];
        let flagged =
            DecisionGraph::default().queue_heads_settled_by_upstream(&obs, &states(&known));
        assert_eq!(flagged.len(), 1, "{flagged:?}");
        assert_eq!(flagged[0].line, 5);
        assert!(
            flagged[0].action.contains("close its item [#shipit]"),
            "{}",
            flagged[0].action
        );
        assert!(!flagged[0].action.contains("--ack-text"));
    }

    #[test]
    fn a_done_entry_is_flagged_only_when_every_pull_request_closed_unmerged() {
        let obs = Observations {
            done_trees: vec![
                done(
                    "abandoned",
                    "- 2026-11-20 [#abandoned] https://github.com/o/r/pull/1\n",
                ),
                done(
                    "superseded",
                    "- 2026-11-20 [#superseded] https://github.com/o/r/pull/1 then https://github.com/o/r/pull/2\n",
                ),
                done(
                    "pending",
                    "- 2026-11-20 [#pending] https://github.com/o/r/pull/3\n",
                ),
                done(
                    "unseen",
                    "- 2026-11-20 [#unseen] https://github.com/o/r/pull/4\n",
                ),
            ],
            ..Observations::default()
        };
        let known = [
            (1, UpstreamState::Closed),
            (2, UpstreamState::Merged),
            (3, UpstreamState::Open),
        ];
        let flagged =
            DecisionGraph::default().done_trees_unmerged_by_upstream(&obs, &states(&known));
        let ids: Vec<&str> = flagged.iter().map(|item| item.item_id.as_str()).collect();
        assert_eq!(ids, vec!["abandoned"]);
        assert!(
            flagged[0]
                .action
                .contains("agent-doc backlog <FILE> reopen abandoned"),
            "{}",
            flagged[0].action
        );
    }
}
