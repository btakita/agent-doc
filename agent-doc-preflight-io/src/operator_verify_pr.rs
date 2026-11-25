//! Admission strike for a `🙋` `[operator-verify]` pull-request gate
//! (`#prverify`).
//!
//! The watch set is only the pull requests those rows name
//! ([`operator_verify_pr_watches`]). A full document graph is not built, and
//! a pull request that no such row names is not looked up. Terminal cache
//! entries are not refreshed. One pass makes at most one batched `gh` call,
//! for the due subset, then strikes from the cache: merged is done, closed
//! without merging is moot.

use std::collections::BTreeSet;
use std::path::Path;
use std::time::Duration;

use agent_doc_answer_ledger::graph::{
    GithubUpstream, OperatorVerifyPrStrike, UpstreamState, operator_verify_pr_watches,
    reevaluate_operator_verify_prs,
};
use anyhow::Result;

use crate::upstream_state::{self, UpstreamStateCache};

/// Due named pull requests: never-observed, or an open or failed lookup past
/// its TTL. A merged or closed entry is not due.
pub fn operator_verify_pulls_due(
    watches: &BTreeSet<GithubUpstream>,
    cache: &UpstreamStateCache,
    now: u64,
) -> BTreeSet<GithubUpstream> {
    watches
        .iter()
        .filter(|upstream| cache.is_due(upstream, now))
        .cloned()
        .collect()
}

/// Refresh the due named pull requests, then strike settled rows.
///
/// `lookup` is the batched GitHub read. It is not called when nothing is due
/// or observation is disabled. The document is not read again: `content` is
/// the admitted cut.
pub fn run_operator_verify_pr_on_admitted_cut(
    file: &Path,
    content: &str,
    fence: &dyn crate::PendingMaintenanceEffectFence,
) -> Result<Option<String>> {
    settle_operator_verify_prs(file, content, Some(fence), &upstream_state::github_states)
}

pub(crate) fn settle_operator_verify_prs(
    file: &Path,
    content: &str,
    fence: Option<&dyn crate::PendingMaintenanceEffectFence>,
    lookup: &upstream_state::BatchLookup<'_>,
) -> Result<Option<String>> {
    if !content.contains("[operator-verify]") || !content.contains('🙋') {
        return Ok(None);
    }
    let watches = match operator_verify_pr_watches(content) {
        Ok(watches) if !watches.is_empty() => watches,
        Ok(_) => return Ok(None),
        Err(err) => {
            eprintln!("[preflight] operator-verify pr: not applied (queue unreadable): {err:#}");
            return Ok(None);
        }
    };
    let root = agent_doc_project_root_io::project_root_containing(file);
    if let Some(root) = root.as_deref()
        && !upstream_state::disabled()
    {
        let now = upstream_state::now_secs();
        let due = operator_verify_pulls_due(&watches, &upstream_state::load(root), now);
        if !due.is_empty() {
            // One call. A larger due set waits for the next admission rather
            // than polling the rest of the document's pull requests.
            if let Err(err) = upstream_state::observe(
                root,
                &due,
                now,
                1,
                upstream_state::ARTIFACTS_PER_CALL,
                Duration::from_secs(10),
                lookup,
            ) {
                eprintln!("[preflight] operator-verify pr: lookup not recorded: {err:#}");
            }
        }
    }
    let cache = root
        .as_deref()
        .map(upstream_state::load)
        .unwrap_or_default();
    let Some(OperatorVerifyPrStrike {
        content: next,
        struck,
    }) = reevaluate_operator_verify_prs(content, &|upstream| cache.state_of(upstream))?
    else {
        return Ok(None);
    };
    if let Some(fence) = fence {
        fence.check(file)?;
    }
    let source = "preflight_operator_verify_pr";
    let admitted = fence.map(|_| agent_doc_hash::content_hash(content));
    let persisted = super::persist_queue_maintenance_doc_with_admitted_authority(
        file,
        &next,
        content,
        root.as_deref(),
        source,
        admitted.as_deref(),
        fence,
        None,
    )?;
    eprintln!(
        "[preflight] operator-verify pr: struck {struck} row{} (#prverify)",
        if struck == 1 { "" } else { "s" }
    );
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "operator_verify_pr_reeval file={} struck={struck} wanted={} (#prverify)",
            file.display(),
            watches.len(),
        ),
    );
    Ok(Some(persisted))
}

/// Test helper: the states a cache would report, without a lookup.
#[cfg(test)]
fn cached_state(cache: &UpstreamStateCache, upstream: &GithubUpstream) -> Option<UpstreamState> {
    cache.state_of(upstream)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::Path;

    fn document(queue: &str) -> String {
        format!("<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n")
    }

    fn project(content: &str) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/state")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, content).unwrap();
        (dir, doc)
    }

    fn write_merged(root: &Path, number: u64) {
        let cache = format!(
            "{{\n  \"upstreams\": {{\n    \"github:acme/widgets/pull/{number}\": {{\n      \"state\": \"merged\",\n      \"observed_at\": 1\n    }}\n  }}\n}}\n"
        );
        std::fs::write(
            root.join(".agent-doc/state/github-upstream-state.json"),
            cache,
        )
        .unwrap();
    }

    struct Allow;

    impl crate::PendingMaintenanceEffectFence for Allow {
        fn check(&self, _file: &Path) -> Result<()> {
            Ok(())
        }
    }

    #[test]
    fn a_terminal_cache_entry_is_not_due_and_a_missing_one_is() {
        let pull = |number| GithubUpstream::pull_request("acme", "widgets", number);
        let mut cache = UpstreamStateCache::default();
        cache.upstreams.insert(
            pull(1).node_id(),
            upstream_state::CachedObservation {
                state: Some(UpstreamState::Merged),
                observed_at: 1,
            },
        );
        cache.upstreams.insert(
            pull(2).node_id(),
            upstream_state::CachedObservation {
                state: Some(UpstreamState::Open),
                observed_at: 1_000,
            },
        );
        let watches = BTreeSet::from([pull(1), pull(2), pull(3)]);
        let due = operator_verify_pulls_due(&watches, &cache, 1_100);
        let numbers: Vec<u64> = due.iter().map(|upstream| upstream.number).collect();
        assert_eq!(numbers, vec![3], "{due:?}");
        assert!(cached_state(&cache, &pull(1)).is_some());
    }

    #[test]
    fn a_cached_merge_strikes_without_a_lookup() {
        let doc = document(
            "- ### 🙋 [#q-origin] follow-up: human (maintainer): approve https://github.com/acme/widgets/pull/231. `reviewDecision` is still REVIEW_REQUIRED, and this preset cannot approve or merge. [operator-verify] [#q-gate]\n- ### 🙋 follow-up: operator: decide whether to keep the check. [operator-verify] [#q-decide]\n",
        );
        let (dir, file) = project(&doc);
        write_merged(dir.path(), 231);
        let calls = RefCell::new(0usize);
        let lookup = |batch: &[GithubUpstream]| {
            *calls.borrow_mut() += 1;
            assert!(
                batch.is_empty(),
                "a terminal cache entry is not looked up: {batch:?}"
            );
            Vec::new()
        };
        let persisted = settle_operator_verify_prs(&file, &doc, Some(&Allow), &lookup)
            .unwrap()
            .expect("the merged pull request strikes the gate");
        assert_eq!(*calls.borrow(), 0);
        assert!(persisted.contains("(#prverify)"), "{persisted}");
        assert!(persisted.contains("decide whether"), "{persisted}");
        assert_eq!(std::fs::read_to_string(&file).unwrap(), persisted);
    }

    #[test]
    fn a_missing_state_looks_up_only_the_named_pull_request() {
        let doc = document(concat!(
            "- ### https://github.com/acme/widgets/pull/214\n",
            "- ### https://github.com/acme/widgets/pull/232\n",
            "- ### 🙋 follow-up: human (maintainer): approve PR 231. [operator-verify] [#q-gate]\n",
        ));
        let (_dir, file) = project(&doc);
        let seen = RefCell::new(Vec::new());
        let lookup = |batch: &[GithubUpstream]| {
            seen.borrow_mut()
                .extend(batch.iter().map(|upstream| upstream.number));
            batch.iter().map(|_| Some(UpstreamState::Merged)).collect()
        };
        let persisted = settle_operator_verify_prs(&file, &doc, Some(&Allow), &lookup)
            .unwrap()
            .expect("the lookup's merged state strikes the row");
        assert_eq!(seen.borrow().as_slice(), &[231]);
        assert!(
            persisted.contains("https://github.com/acme/widgets/pull/231 merged"),
            "{persisted}"
        );
        assert!(
            persisted.contains("https://github.com/acme/widgets/pull/214"),
            "{persisted}"
        );
        assert!(
            !persisted.contains("~~") || persisted.contains("(#prverify)"),
            "{persisted}"
        );
    }
}
