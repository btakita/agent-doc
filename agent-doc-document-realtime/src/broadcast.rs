/// The conflict-free CRDT union two open editors should converge on after one of
/// them edits a shared document, plus an echo-suppress signal for the originator.
#[derive(Debug, Clone)]
pub struct BroadcastMerge {
    /// The CRDT union of both editor buffers over the shared on-disk base.
    pub merged: String,
    /// `true` when the originating editor's buffer already equals `merged` (the
    /// peer contributed nothing new), so re-delivering the merge back to the
    /// originator would be a redundant self-echo and must be suppressed.
    pub originator_echo_suppressed: bool,
}

/// One peer editor buffer participating in a multi-editor broadcast merge.
#[derive(Debug, Clone)]
pub struct BroadcastPeer {
    pub editor_id: String,
    pub content: String,
}

impl BroadcastPeer {
    pub fn new(editor_id: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            editor_id: editor_id.into(),
            content: content.into(),
        }
    }
}

/// A target editor that should receive a merged document update.
#[derive(Debug, Clone)]
pub struct BroadcastTarget {
    pub editor_id: String,
    pub merged: String,
}

/// `#rtwbcast` Option C - the merge-only multi-editor broadcast policy.
///
/// Given the shared on-disk `base` and two open editor buffers (`originator`
/// typed the change that triggered this broadcast; `peer` is another editor's
/// buffer), produce the conflict-free CRDT union both editors should converge on,
/// plus the originator echo-suppress flag. This is a pure function: no I/O, no
/// clock, and deliberately no delivery. It is the convergence math only.
pub fn compute_broadcast(
    base: &str,
    originator: &str,
    peer: &str,
) -> anyhow::Result<BroadcastMerge> {
    if text_delta_included(base, peer, originator) {
        return Ok(BroadcastMerge {
            merged: originator.to_string(),
            originator_echo_suppressed: true,
        });
    }
    let base_state = agent_doc_merge::crdt::CrdtDoc::from_text(base).encode_state();
    let (merged, _state) =
        agent_doc_merge::merge_contents_crdt(Some(&base_state), originator, peer)?;
    let originator_echo_suppressed = merged == originator;
    Ok(BroadcastMerge {
        merged,
        originator_echo_suppressed,
    })
}

fn text_delta_included(base: &str, changed: &str, candidate: &str) -> bool {
    if changed == base || changed == candidate {
        return true;
    }
    let base_counts = line_counts(base);
    let changed_counts = line_counts(changed);
    let candidate_counts = line_counts(candidate);
    let mut saw_delta = false;
    for line in base_counts.keys().chain(changed_counts.keys()) {
        let base_count = *base_counts.get(line).unwrap_or(&0);
        let changed_count = *changed_counts.get(line).unwrap_or(&0);
        let candidate_count = *candidate_counts.get(line).unwrap_or(&0);
        if changed_count != base_count {
            saw_delta = true;
        }
        if changed_count > base_count && candidate_count < changed_count {
            return false;
        }
        if changed_count < base_count && candidate_count > changed_count {
            return false;
        }
    }
    saw_delta
}

fn line_counts(text: &str) -> std::collections::BTreeMap<&str, usize> {
    let mut counts = std::collections::BTreeMap::new();
    for line in text.split_inclusive('\n') {
        *counts.entry(line).or_insert(0) += 1;
    }
    counts
}

/// Production-shaped N-buffer broadcast planner.
///
/// The originator is never returned as a target. Each peer receives a CRDT union
/// of the originator buffer and that peer's current buffer over the shared disk
/// base, unless the peer already equals that merged result.
pub fn compute_broadcast_plan(
    base: &str,
    originator_editor_id: &str,
    originator: &str,
    peers: &[BroadcastPeer],
) -> anyhow::Result<Vec<BroadcastTarget>> {
    let mut targets = Vec::new();
    for peer in peers {
        if peer.editor_id == originator_editor_id {
            continue;
        }
        let merge = compute_broadcast(base, originator, &peer.content)?;
        if merge.merged == peer.content {
            continue;
        }
        targets.push(BroadcastTarget {
            editor_id: peer.editor_id.clone(),
            merged: merge.merged,
        });
    }
    Ok(targets)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_broadcast_unions_two_editor_buffers_conflict_free() {
        // #rtwbcast Option C: two editors edit a shared base; the policy
        // produces the conflict-free union, and the originator is not
        // echo-suppressed because the peer contributed a genuine new edit.
        let base = "shared line one\nshared line two\n";
        let originator = "shared line one\noriginator edit\nshared line two\n";
        let peer = "shared line one\nshared line two\npeer edit\n";
        let result = compute_broadcast(base, originator, peer).unwrap();
        assert!(result.merged.contains("originator edit"));
        assert!(result.merged.contains("peer edit"));
        for marker in ["<<<<<<<", "=======", ">>>>>>>"] {
            assert!(
                !result.merged.contains(marker),
                "broadcast merge must be conflict-free; found `{marker}`"
            );
        }
        assert!(
            !result.originator_echo_suppressed,
            "a peer edit means the merge differs from the originator"
        );
    }

    #[test]
    fn compute_broadcast_suppresses_echo_when_peer_unchanged() {
        // When the peer buffer equals the base, the merge equals the
        // originator's buffer, so re-delivering to the originator is redundant.
        let base = "shared line\n";
        let originator = "shared line\noriginator edit\n";
        let peer = base;
        let result = compute_broadcast(base, originator, peer).unwrap();
        assert_eq!(result.merged, originator);
        assert!(
            result.originator_echo_suppressed,
            "an unchanged peer must suppress the redundant echo back to the originator"
        );
    }

    #[test]
    fn compute_broadcast_rebroadcast_preserves_component_boundaries() {
        let base = concat!(
            "# Doc\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#base] existing\n",
            "<!-- /agent:backlog -->\n"
        );
        let editor_a = base.replace(
            "- [ ] [#base] existing\n",
            "- [ ] [#base] existing\n- [ ] [#edit-A] queued in editor A\n",
        );
        let editor_b = base.replace(
            "- [ ] [#base] existing\n",
            "- [ ] [#base] existing\n- [ ] [#edit-B] queued in editor B\n",
        );

        let merged = compute_broadcast(base, &editor_a, &editor_b)
            .unwrap()
            .merged;
        assert!(merged.contains("#edit-A"));
        assert!(merged.contains("#edit-B"));
        agent_doc_element::element::parse(&merged).unwrap();

        let rebroadcast = compute_broadcast(base, &merged, &editor_a).unwrap();
        assert_eq!(
            rebroadcast.merged, merged,
            "rebroadcasting an already-converged buffer to a stale peer must not re-merge component markers"
        );
        assert!(rebroadcast.originator_echo_suppressed);
        agent_doc_element::element::parse(&rebroadcast.merged).unwrap();
    }

    #[test]
    fn compute_broadcast_plan_skips_originator_and_unchanged_peer() {
        let base = "one\n";
        let origin = "one\norigin\n";
        let peer_changed = BroadcastPeer::new("peer-a", "one\npeer\n");
        let peer_unchanged = BroadcastPeer::new("peer-b", origin);
        let origin_peer = BroadcastPeer::new("origin", "one\norigin peer should skip\n");

        let targets = compute_broadcast_plan(
            base,
            "origin",
            origin,
            &[peer_changed, peer_unchanged, origin_peer],
        )
        .unwrap();

        assert_eq!(targets.len(), 1);
        assert_eq!(targets[0].editor_id, "peer-a");
        assert!(targets[0].merged.contains("origin"));
        assert!(targets[0].merged.contains("peer"));
    }
}
