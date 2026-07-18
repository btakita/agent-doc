//! Pure backlog/icebox/pending to queue-sync projection policy.
//!
//! This module owns content-only collection for backlog-backed queue sync,
//! priority ranks, and auto-DAG dependencies. Callers own file IO, snapshots,
//! done archives, tombstones, and queue body mutation.

use std::collections::{HashMap, HashSet};

use agent_doc_element::element;
use agent_doc_element_backlog::backlog;

use crate::document_queue::{self, BacklogQueueSyncMode, QueueEntry};

/// Effective backlog-to-queue sync request collected from tracked-work
/// components.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogQueueSyncRequest {
    pub mode: BacklogQueueSyncMode,
    pub ids: Vec<String>,
    pub enqueue_ids: Vec<String>,
    pub priority: bool,
}

/// Effective one-shot queue-sync request collected for the explicit
/// `agent-doc queue sync` command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OneShotBacklogQueueSyncRequest {
    pub mode: BacklogQueueSyncMode,
    pub ids: Vec<String>,
}

/// Operator-facing report facts for a completed backlog-to-queue sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BacklogQueueSyncReport {
    pub prompt_count: usize,
    pub already_present: Vec<String>,
    pub newly_materialized: Vec<String>,
}

/// Active-queue policy branch applied while planning automatic backlog sync.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AutoBacklogQueueSyncPolicy {
    FreshActivation,
    ExplicitlyStopped,
    HoldFreshIds,
    GoModeAppend,
}

/// Pure id-selection result for automatic backlog-to-queue sync.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutoBacklogQueueSyncPlan {
    pub ids: Vec<String>,
    pub completed_excluded_count: usize,
    pub tombstone_suppressed_count: usize,
    pub active_policy: AutoBacklogQueueSyncPolicy,
    pub active_held_count: usize,
}

/// IO-derived evidence for automatic backlog-to-queue sync planning.
pub struct AutoBacklogQueueSyncInput<'a> {
    pub requested_ids: &'a [String],
    pub enqueue_ids: &'a [String],
    pub done_ids: &'a HashSet<String>,
    pub tombstones: &'a HashSet<String>,
    pub entries: &'a [QueueEntry],
    pub persisted_active_incoming: bool,
    pub persisted_active_before_binding: bool,
    pub queue_go_mode: bool,
    pub queue_explicitly_stopped: bool,
}

/// Select which tracked-work ids may be mirrored into the queue this pass.
///
/// Callers provide IO-derived facts such as done ids, tombstones, and persisted
/// queue state. This function owns the queue policy: completed ids and
/// operator-deleted tombstones never remirror; a plain already-active queue
/// holds fresh backlog ids out of the loop unless they were explicitly
/// enqueued; explicit `go` mode opts into appending fresh backlog ids.
pub fn plan_auto_backlog_queue_sync_ids(
    input: AutoBacklogQueueSyncInput<'_>,
) -> AutoBacklogQueueSyncPlan {
    let AutoBacklogQueueSyncInput {
        requested_ids,
        enqueue_ids,
        done_ids,
        tombstones,
        entries,
        persisted_active_incoming,
        persisted_active_before_binding,
        queue_go_mode,
        queue_explicitly_stopped,
    } = input;
    let mut ids = requested_ids.to_vec();

    let done_lower: HashSet<String> = done_ids
        .iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .collect();
    let before_done = ids.len();
    ids.retain(|id| !done_lower.contains(&normalize_id(id)));
    let completed_excluded_count = before_done - ids.len();

    let tombstone_lower: HashSet<String> = tombstones
        .iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .collect();
    let before_tombstones = ids.len();
    ids.retain(|id| !tombstone_lower.contains(&normalize_id(id)));
    let tombstone_suppressed_count = before_tombstones - ids.len();

    let (active_policy, active_held_count) = if queue_explicitly_stopped {
        let held = ids.len();
        ids.clear();
        (AutoBacklogQueueSyncPolicy::ExplicitlyStopped, held)
    } else if persisted_active_incoming && persisted_active_before_binding && !queue_go_mode {
        let existing_queue_ids: HashSet<String> = entries
            .iter()
            .filter_map(crate::queue_projection::queue_entry_do_id)
            .map(|id| id.to_ascii_lowercase())
            .collect();
        let enqueue_ids: HashSet<String> = enqueue_ids.iter().map(|id| normalize_id(id)).collect();
        let before_hold = ids.len();
        ids.retain(|id| {
            let key = normalize_id(id);
            existing_queue_ids.contains(&key) || enqueue_ids.contains(&key)
        });
        (
            AutoBacklogQueueSyncPolicy::HoldFreshIds,
            before_hold - ids.len(),
        )
    } else if persisted_active_incoming && queue_go_mode {
        (AutoBacklogQueueSyncPolicy::GoModeAppend, 0)
    } else {
        (AutoBacklogQueueSyncPolicy::FreshActivation, 0)
    };

    AutoBacklogQueueSyncPlan {
        ids,
        completed_excluded_count,
        tombstone_suppressed_count,
        active_policy,
        active_held_count,
    }
}

fn normalize_id(id: &str) -> String {
    id.trim().to_ascii_lowercase()
}

/// Collect queue sync source ids from tracked-work components.
///
/// `backlog` and the legacy `pending` alias can opt into component-level sync
/// with a `queue` attribute. `icebox` intentionally cannot opt into
/// component-level sync so parked work is not auto-promoted, but explicit
/// per-item enqueue markers are honored for all tracked-work components.
pub fn collect_backlog_queue_sync(
    components: &[element::Component],
    content: &str,
) -> Option<BacklogQueueSyncRequest> {
    let mut mode: Option<BacklogQueueSyncMode> = None;
    let mut ids: Vec<String> = Vec::new();
    let mut enqueue_ids: Vec<String> = Vec::new();
    let mut priority = false;
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        enqueue_ids.extend(backlog::active_enqueue_item_ids(body));
        if comp.name == "icebox" {
            continue;
        }
        let Some(value) = comp.attrs.get("queue") else {
            continue;
        };
        let comp_priority = comp.attrs.contains_key("priority");
        priority |= comp_priority;
        let Some(comp_mode) = BacklogQueueSyncMode::parse(value) else {
            continue;
        };
        // `#backlogqueueprepend`: a `priority` backlog with a *bare* `queue`
        // token mirrors in backlog order, and the backlog-capture contract puts
        // new follow-up items at the FRONT ("what you read is what you get").
        // Appending them to the queue tail contradicts that — a freshly filed
        // follow-up landed behind dozens of older heads and never surfaced, so
        // operators re-added `do [#id]` by hand.
        //
        // Only the bare token is reinterpreted: an explicit `queue="append"`
        // remains an operator override and still appends.
        let comp_mode = if comp_priority && value.trim().is_empty() {
            BacklogQueueSyncMode::Prepend
        } else {
            comp_mode
        };
        if mode.is_none() {
            mode = Some(comp_mode);
        }
        ids.extend(backlog::active_item_ids(body));
    }
    if mode.is_none() && !enqueue_ids.is_empty() {
        mode = Some(BacklogQueueSyncMode::Append);
    }
    ids.extend(enqueue_ids.iter().cloned());
    mode.map(|m| BacklogQueueSyncRequest {
        mode: m,
        ids,
        enqueue_ids,
        priority,
    })
}

/// Collect the explicit one-shot queue-sync request.
///
/// Unlike preflight's automatic collector, the explicit command honors a
/// component-level `queue` attribute on `agent:icebox`. That keeps parked work
/// inert by default while preserving an operator-authored one-shot promotion
/// path for parked items.
pub fn collect_one_shot_backlog_queue_sync(
    components: &[element::Component],
    content: &str,
) -> Option<OneShotBacklogQueueSyncRequest> {
    let mut mode: Option<BacklogQueueSyncMode> = None;
    let mut ids: Vec<String> = Vec::new();
    let mut enqueue_ids: Vec<String> = Vec::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        enqueue_ids.extend(backlog::active_enqueue_item_ids(body));
        let Some(value) = comp.attrs.get("queue") else {
            continue;
        };
        let Some(comp_mode) = BacklogQueueSyncMode::parse(value) else {
            continue;
        };
        if mode.is_none() {
            mode = Some(comp_mode);
        }
        ids.extend(backlog::active_item_ids(body));
    }
    if mode.is_none() && !enqueue_ids.is_empty() {
        mode = Some(BacklogQueueSyncMode::Append);
    }
    ids.extend(enqueue_ids);
    mode.map(|m| OneShotBacklogQueueSyncRequest { mode: m, ids })
}

/// Build report facts for a backlog-to-queue sync result.
pub fn backlog_queue_sync_report(
    before: &[QueueEntry],
    requested_ids: &[String],
    after: &[QueueEntry],
) -> BacklogQueueSyncReport {
    let existing_ids: HashSet<String> = before
        .iter()
        .flat_map(document_queue::queue_entry_reference_ids)
        .collect();
    let mut seen_existing = HashSet::new();
    let already_present = requested_ids
        .iter()
        .map(|id| id.trim().to_ascii_lowercase())
        .filter(|id| existing_ids.contains(id))
        .filter(|id| seen_existing.insert(id.clone()))
        .collect();

    let prompt_count = after
        .iter()
        .filter(|entry| matches!(entry, QueueEntry::Prompt(_)))
        .count();

    let mut seen_new = HashSet::new();
    let newly_materialized = after
        .iter()
        .flat_map(document_queue::queue_entry_reference_ids)
        .filter(|id| !existing_ids.contains(id))
        .filter(|id| seen_new.insert(id.clone()))
        .collect();

    BacklogQueueSyncReport {
        prompt_count,
        already_present,
        newly_materialized,
    }
}

/// Reconcile operator-delete tombstones for the backlog-to-queue mirror.
///
/// Inputs are normalized id sets. An id active in the committed snapshot but
/// fully absent from the live queue was deleted by the operator, so it becomes
/// tombstoned. An id active in the live queue clears any prior tombstone because
/// the operator re-added it.
pub fn reconcile_queue_tombstones(
    existing_tombstones: &HashSet<String>,
    snapshot_active_ids: &HashSet<String>,
    current_all_ids: &HashSet<String>,
    current_active_ids: &HashSet<String>,
) -> HashSet<String> {
    let mut tombstones = existing_tombstones.clone();
    for id in snapshot_active_ids.difference(current_all_ids) {
        tombstones.insert(id.clone());
    }
    for id in current_active_ids {
        tombstones.remove(id);
    }
    tombstones
}

/// Format normalized queue ids for operator-facing CLI messages.
pub fn format_queue_ids(ids: &[String]) -> String {
    ids.iter()
        .map(|id| format!("#{id}"))
        .collect::<Vec<_>>()
        .join(", ")
}

/// Build an id->priority-rank map from active tracked-work items.
///
/// First-seen rank wins on duplicate ids across components.
pub fn collect_backlog_priority_ranks(
    components: &[element::Component],
    content: &str,
) -> HashMap<String, u8> {
    let mut rank = HashMap::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, r) in backlog::active_item_priorities(body) {
            rank.entry(id).or_insert(r);
        }
    }
    rank
}

/// Build an id->`after=#id` dependency map from active tracked-work items.
///
/// First-seen deps win on duplicate ids across components; items with no
/// dependency tokens are omitted.
pub fn collect_after_deps(
    components: &[element::Component],
    content: &str,
) -> HashMap<String, Vec<String>> {
    let mut deps = HashMap::new();
    for comp in components {
        if !matches!(comp.name.as_str(), "backlog" | "icebox" | "pending") {
            continue;
        }
        let body = &content[comp.open_end..comp.close_start];
        for (id, d) in backlog::active_item_after_deps(body) {
            if !d.is_empty() {
                deps.entry(id).or_insert(d);
            }
        }
    }
    deps
}

/// Where a follow-up item created this cycle lands in the queue.
///
/// `Prepend` is the default: a follow-up filed by the turn that just ran is the
/// most task-relevant work in the document, so it belongs at the head rather
/// than behind the whole existing backlog mirror.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FollowUpQueuePlacement {
    #[default]
    Prepend,
    Append,
}

impl FollowUpQueuePlacement {
    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "prepend" | "front" | "top" => Some(Self::Prepend),
            "append" | "back" | "tail" | "bottom" => Some(Self::Append),
            _ => None,
        }
    }

    pub fn sync_mode(self) -> BacklogQueueSyncMode {
        match self {
            Self::Prepend => BacklogQueueSyncMode::Prepend,
            Self::Append => BacklogQueueSyncMode::Append,
        }
    }

    /// Operator-facing verb for closeout logs.
    pub fn log_verb(self) -> &'static str {
        match self {
            Self::Prepend => "prepended",
            Self::Append => "appended",
        }
    }
}

/// `#queueatcreate`: enqueue backlog ids created THIS cycle, from content.
///
/// Companion to `sync_same_cycle_pending_adds_into_go_queue`, but pure: the
/// caller owns persistence, so the enqueue can ride the SAME write path as the
/// backlog mutation that created the ids. That matters because the queue
/// maintenance persistence path hard-requires a ready editor model and discards
/// its work otherwise, while the backlog write path converges and succeeds — so
/// sharing the backlog's path is what keeps the two components in step.
///
/// Opt-in is unchanged: the backlog component must carry the `queue` attribute.
/// `agent:icebox` never participates — parked work is not auto-promoted, matching
/// `collect_backlog_queue_sync`.
///
/// Returns `None` when nothing changes (no opt-in, no queue component, or every
/// id is already queued).
pub fn enqueue_created_ids_in_content(
    content: &str,
    created_ids: &[String],
    placement: FollowUpQueuePlacement,
) -> anyhow::Result<Option<String>> {
    if created_ids.is_empty() {
        return Ok(None);
    }
    let components = element::parse(content)?;

    let backlog_opts_in = components
        .iter()
        .filter(|comp| matches!(comp.name.as_str(), "backlog" | "pending"))
        .any(|comp| comp.attrs.contains_key("queue"));
    if !backlog_opts_in {
        return Ok(None);
    }

    let Some(queue_comp) = components.iter().find(|comp| comp.name == "queue") else {
        return Ok(None);
    };

    // Only ids that are actually open BACKLOG items may be queued: an icebox id,
    // or an id that deduped into an existing item, must not become a queue head.
    let open_backlog: HashSet<String> = components
        .iter()
        .filter(|comp| matches!(comp.name.as_str(), "backlog" | "pending"))
        .flat_map(|comp| backlog::active_item_ids(&content[comp.open_end..comp.close_start]))
        .map(|id| id.trim().to_ascii_lowercase())
        .collect();
    let queueable: Vec<String> = created_ids
        .iter()
        .map(|id| id.trim().trim_start_matches('#').to_ascii_lowercase())
        .filter(|id| !id.is_empty() && open_backlog.contains(id))
        .collect();
    if queueable.is_empty() {
        return Ok(None);
    }

    let body = &content[queue_comp.open_end..queue_comp.close_start];

    // Read-only parse: only to learn which ids are already queued. We must NOT
    // render the parsed entries back over the body — a parse/render round-trip
    // normalizes operator-authored lines (observed: `- /goal ...` came back as
    // `/goal ...`, silently dropping the list marker). Operator-visible document
    // text is authoritative, and this is an insert-only operation, so splice the
    // new lines in textually and leave every existing byte untouched.
    let entries = document_queue::parse(body)?;
    let existing: HashSet<String> = entries
        .iter()
        .flat_map(document_queue::queue_entry_reference_ids)
        .collect();

    let mut seen = HashSet::new();
    let mut fresh: Vec<String> = queueable
        .iter()
        .filter(|id| !existing.contains(*id) && seen.insert((*id).clone()))
        .cloned()
        .collect();

    // `#queueingressorder`: the `priority` graph is ALWAYS active — apply it at
    // ingress, not only in a later preflight maintenance pass. Ordering here is
    // fully deterministic and derives from metadata the agent already declares on
    // the backlog item: `priority=1..9` (1 highest) and `after=#id`.
    //
    // Within the inserted block: prerequisites first (a declared `after=` edge
    // wins over a numerically better priority, since a dependency is a hard
    // constraint and priority is only a preference), then priority rank, then the
    // order the items were filed as a stable tie-break. Cycles degrade to the
    // priority/filed order rather than dropping anything.
    let ranks = collect_backlog_priority_ranks(&components, content);
    let deps = collect_after_deps(&components, content);
    let block: HashSet<String> = fresh.iter().cloned().collect();
    fresh.sort_by_key(|id| {
        (
            ranks.get(id).copied().unwrap_or(u8::MAX),
            id.clone(),
        )
    });
    let mut ordered: Vec<String> = Vec::with_capacity(fresh.len());
    let mut placed: HashSet<String> = HashSet::new();
    // Insert each id after any in-block prerequisite it declares. Bounded to
    // `fresh.len()` passes, so a dependency cycle cannot loop forever.
    for _ in 0..fresh.len() {
        let mut progressed = false;
        for id in &fresh {
            if placed.contains(id) {
                continue;
            }
            let ready = deps
                .get(id)
                .map(|d| {
                    d.iter().all(|dep| {
                        let dep = dep.trim().trim_start_matches('#').to_ascii_lowercase();
                        !block.contains(&dep) || placed.contains(&dep)
                    })
                })
                .unwrap_or(true);
            if ready {
                ordered.push(id.clone());
                placed.insert(id.clone());
                progressed = true;
            }
        }
        if !progressed {
            break;
        }
    }
    // Anything left is part of a cycle: keep it, in priority/filed order.
    for id in &fresh {
        if !placed.contains(id) {
            ordered.push(id.clone());
        }
    }

    let new_lines: Vec<String> = ordered.iter().map(|id| format!("- do [#{id}]")).collect();
    if new_lines.is_empty() {
        return Ok(None);
    }
    let block = new_lines.join("\n");

    // Strictly insert-only: splice the block at the boundary and copy the
    // existing body through byte-for-byte.
    //
    // The component body does NOT carry a leading newline — `open_end` sits past
    // it. An earlier version prefixed one, which produced exactly the reported
    // regression: a blank first line in the queue, and the spliced block running
    // straight into the first existing entry. Trimming the body instead (an even
    // earlier version used `trim_matches('\n')`) silently normalized the
    // operator's own blank lines — the same class of damage as the `- /goal`
    // marker regression this splice exists to avoid. So: touch nothing, insert
    // at the edge.
    let new_body = if body.trim().is_empty() {
        format!("{block}\n")
    } else {
        match placement {
            FollowUpQueuePlacement::Prepend => format!("{block}\n{body}"),
            FollowUpQueuePlacement::Append => {
                let sep = if body.ends_with('\n') { "" } else { "\n" };
                format!("{body}{sep}{block}\n")
            }
        }
    };
    Ok(Some(queue_comp.replace_content(content, &new_body)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> HashSet<String> {
        items.iter().map(|item| item.to_string()).collect()
    }

    fn prompt(text: &str) -> QueueEntry {
        QueueEntry::Prompt(crate::document_queue::QueuePrompt {
            text: text.to_string(),
            multiline: false,
        })
    }

    #[test]
    fn plan_auto_backlog_queue_sync_filters_done_and_tombstoned_ids() {
        let requested_ids = vec![
            "done".to_string(),
            "open".to_string(),
            "deleted".to_string(),
            "mixedcase".to_string(),
        ];
        let done_ids = set(&["DONE"]);
        let tombstones = set(&["Deleted"]);

        let plan = plan_auto_backlog_queue_sync_ids(AutoBacklogQueueSyncInput {
            requested_ids: &requested_ids,
            enqueue_ids: &[],
            done_ids: &done_ids,
            tombstones: &tombstones,
            entries: &[],
            persisted_active_incoming: false,
            persisted_active_before_binding: false,
            queue_go_mode: false,
            queue_explicitly_stopped: false,
        });

        assert_eq!(
            plan,
            AutoBacklogQueueSyncPlan {
                ids: vec!["open".to_string(), "mixedcase".to_string()],
                completed_excluded_count: 1,
                tombstone_suppressed_count: 1,
                active_policy: AutoBacklogQueueSyncPolicy::FreshActivation,
                active_held_count: 0,
            }
        );
    }

    #[test]
    fn plan_auto_backlog_queue_sync_holds_fresh_ids_in_plain_active_queue() {
        let requested_ids = vec![
            "existing".to_string(),
            "fresh".to_string(),
            "manual".to_string(),
        ];
        let enqueue_ids = vec!["manual".to_string()];
        let entries = vec![prompt("do [#existing]")];

        let done_ids = HashSet::new();
        let tombstones = HashSet::new();
        let plan = plan_auto_backlog_queue_sync_ids(AutoBacklogQueueSyncInput {
            requested_ids: &requested_ids,
            enqueue_ids: &enqueue_ids,
            done_ids: &done_ids,
            tombstones: &tombstones,
            entries: &entries,
            persisted_active_incoming: true,
            persisted_active_before_binding: true,
            queue_go_mode: false,
            queue_explicitly_stopped: false,
        });

        assert_eq!(plan.ids, vec!["existing".to_string(), "manual".to_string()]);
        assert_eq!(plan.active_policy, AutoBacklogQueueSyncPolicy::HoldFreshIds);
        assert_eq!(plan.active_held_count, 1);
    }

    #[test]
    fn plan_auto_backlog_queue_sync_go_mode_keeps_fresh_ids() {
        let requested_ids = vec!["existing".to_string(), "fresh".to_string()];
        let entries = vec![prompt("do [#existing]")];

        let done_ids = HashSet::new();
        let tombstones = HashSet::new();
        let plan = plan_auto_backlog_queue_sync_ids(AutoBacklogQueueSyncInput {
            requested_ids: &requested_ids,
            enqueue_ids: &[],
            done_ids: &done_ids,
            tombstones: &tombstones,
            entries: &entries,
            persisted_active_incoming: true,
            persisted_active_before_binding: true,
            queue_go_mode: true,
            queue_explicitly_stopped: false,
        });

        assert_eq!(plan.ids, requested_ids);
        assert_eq!(plan.active_policy, AutoBacklogQueueSyncPolicy::GoModeAppend);
        assert_eq!(plan.active_held_count, 0);
    }

    #[test]
    fn plan_auto_backlog_queue_sync_explicit_stop_clears_remaining_ids() {
        let requested_ids = vec!["a".to_string(), "b".to_string()];

        let done_ids = HashSet::new();
        let tombstones = HashSet::new();
        let plan = plan_auto_backlog_queue_sync_ids(AutoBacklogQueueSyncInput {
            requested_ids: &requested_ids,
            enqueue_ids: &[],
            done_ids: &done_ids,
            tombstones: &tombstones,
            entries: &[],
            persisted_active_incoming: false,
            persisted_active_before_binding: false,
            queue_go_mode: false,
            queue_explicitly_stopped: true,
        });

        assert!(plan.ids.is_empty());
        assert_eq!(
            plan.active_policy,
            AutoBacklogQueueSyncPolicy::ExplicitlyStopped
        );
        assert_eq!(plan.active_held_count, 2);
    }

    #[test]
    fn collect_backlog_queue_sync_reads_mode_and_active_ids() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue=sync -->\n",
            "- [ ] [#a] one\n",
            "- [/] [#g] gated\n",
            "- [ ] [#b] two\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        let request = collect_backlog_queue_sync(&components, content)
            .expect("backlog with queue attr should produce a sync request");
        assert_eq!(request.mode, BacklogQueueSyncMode::Sync);
        assert_eq!(request.ids, vec!["a".to_string(), "b".to_string()]);
        assert!(request.enqueue_ids.is_empty());
    }

    #[test]
    fn collect_backlog_queue_sync_reads_enqueue_markers_without_attr() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] :inbox_tray: one\n",
            "- [/] [#g] :inbox_tray: gated\n",
            "- [ ] [#b] unmarked\n",
            "- [ ] [#c] **enqueue** marked\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        let request = collect_backlog_queue_sync(&components, content)
            .expect("enqueue markers should produce an append request");
        assert_eq!(request.mode, BacklogQueueSyncMode::Append);
        assert_eq!(request.ids, vec!["a".to_string(), "c".to_string()]);
        assert_eq!(request.enqueue_ids, vec!["a".to_string(), "c".to_string()]);
    }

    /// `#backlogqueueprepend`: `<!-- agent:backlog priority queue -->` is the
    /// live shape operators use. The backlog-capture contract files new
    /// follow-ups at the FRONT, so the queue mirror must prepend them too —
    /// appending buried a freshly filed item behind dozens of older heads, and
    /// operators re-added `do [#id]` by hand.
    #[test]
    fn priority_backlog_with_bare_queue_token_prepends() {
        let content = concat!(
            "<!-- agent:queue priority go -->\n",
            "- 📌 do [#old]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#fresh] newly filed follow-up\n",
            "- [ ] [#old] older item\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        let request = collect_backlog_queue_sync(&components, content)
            .expect("priority backlog with bare queue attr should produce a sync request");
        assert_eq!(
            request.mode,
            BacklogQueueSyncMode::Prepend,
            "a priority backlog must mirror new items to the queue head"
        );
        assert!(request.priority);
    }

    /// The reinterpretation is scoped to the bare token: an explicit
    /// `queue=append` stays an operator override.
    #[test]
    fn priority_backlog_with_explicit_append_still_appends() {
        let content = concat!(
            "<!-- agent:queue priority go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue=append -->\n",
            "- [ ] [#fresh] newly filed follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        let request = collect_backlog_queue_sync(&components, content)
            .expect("explicit append should produce a sync request");
        assert_eq!(request.mode, BacklogQueueSyncMode::Append);
    }

    /// A bare `queue` token WITHOUT `priority` keeps its documented append
    /// default, so non-priority backlogs are unaffected.
    #[test]
    fn non_priority_backlog_with_bare_queue_token_still_appends() {
        let content = concat!(
            "<!-- agent:queue go -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog queue -->\n",
            "- [ ] [#fresh] newly filed follow-up\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        let request = collect_backlog_queue_sync(&components, content)
            .expect("bare queue attr should produce a sync request");
        assert_eq!(request.mode, BacklogQueueSyncMode::Append);
        assert!(!request.priority);
    }

    #[test]
    fn collect_one_shot_backlog_queue_sync_honors_icebox_queue_attr() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:icebox queue=sync -->\n",
            "- [ ] [#parked] parked work\n",
            "- [/] [#gated] gated parked work\n",
            "<!-- /agent:icebox -->\n",
        );
        let components = element::parse(content).unwrap();
        assert!(
            collect_backlog_queue_sync(&components, content).is_none(),
            "automatic preflight sync must not promote icebox component attrs"
        );

        let request = collect_one_shot_backlog_queue_sync(&components, content)
            .expect("explicit one-shot sync should honor icebox queue attr");
        assert_eq!(request.mode, BacklogQueueSyncMode::Sync);
        assert_eq!(request.ids, vec!["parked".to_string()]);
    }

    #[test]
    fn collect_one_shot_backlog_queue_sync_reads_enqueue_markers_without_attr() {
        let content = concat!(
            "<!-- agent:queue -->\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#parked] /enqueue parked work\n",
            "- [ ] [#cold] parked future work\n",
            "<!-- /agent:icebox -->\n",
        );
        let components = element::parse(content).unwrap();
        let request = collect_one_shot_backlog_queue_sync(&components, content)
            .expect("enqueue marker should create a one-shot append request");
        assert_eq!(request.mode, BacklogQueueSyncMode::Append);
        assert_eq!(request.ids, vec!["parked".to_string()]);
    }

    #[test]
    fn reconcile_queue_tombstones_adds_deleted_active_ids_and_clears_readded_ids() {
        let existing = set(&["old"]);

        let tombstones =
            reconcile_queue_tombstones(&existing, &set(&["a", "b"]), &set(&["b"]), &set(&["b"]));

        assert!(tombstones.contains("a"));
        assert!(tombstones.contains("old"));
        assert!(!tombstones.contains("b"));

        let cleared = reconcile_queue_tombstones(
            &tombstones,
            &set(&["b"]),
            &set(&["a", "b"]),
            &set(&["a", "b"]),
        );

        assert!(!cleared.contains("a"));
        assert!(cleared.contains("old"));
    }

    #[test]
    fn reconcile_queue_tombstones_does_not_treat_struck_ids_as_deleted() {
        let tombstones =
            reconcile_queue_tombstones(&HashSet::new(), &set(&["a"]), &set(&["a"]), &set(&[]));

        assert!(!tombstones.contains("a"));
    }

    #[test]
    fn backlog_queue_sync_report_is_multi_id_aware_and_deduped() {
        let before = crate::document_queue::parse("- do [#existing] [#multi]\n").unwrap();
        let after =
            crate::document_queue::parse("- do [#existing] [#multi]\n- do [#new]\n- do [#new]\n")
                .unwrap();
        let requested_ids = vec![
            "existing".to_string(),
            "multi".to_string(),
            "existing".to_string(),
            "new".to_string(),
        ];

        let report = backlog_queue_sync_report(&before, &requested_ids, &after);
        assert_eq!(
            report.already_present,
            vec!["existing".to_string(), "multi".to_string()]
        );
        assert_eq!(report.newly_materialized, vec!["new".to_string()]);
        assert_eq!(report.prompt_count, 3);
        assert_eq!(
            format_queue_ids(&report.already_present),
            "#existing, #multi"
        );
    }

    #[test]
    fn collect_backlog_queue_sync_none_without_attr() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] one\n",
            "<!-- /agent:backlog -->\n",
        );
        let components = element::parse(content).unwrap();
        assert!(collect_backlog_queue_sync(&components, content).is_none());
    }

    #[test]
    fn collect_backlog_priority_ranks_reads_tracked_work_components() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] priority=5 one\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#b] priority=1 parked\n",
            "<!-- /agent:icebox -->\n",
        );
        let components = element::parse(content).unwrap();
        let ranks = collect_backlog_priority_ranks(&components, content);
        assert_eq!(ranks.get("a"), Some(&5));
        assert_eq!(ranks.get("b"), Some(&1));
    }

    #[test]
    fn collect_after_deps_reads_tracked_work_components() {
        let content = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#a] after=#root one\n",
            "- [ ] [#b] no deps\n",
            "<!-- /agent:backlog -->\n\n",
            "<!-- agent:icebox -->\n",
            "- [ ] [#c] after=#a parked\n",
            "<!-- /agent:icebox -->\n",
        );
        let components = element::parse(content).unwrap();
        let deps = collect_after_deps(&components, content);
        assert_eq!(deps.get("a"), Some(&vec!["root".to_string()]));
        assert_eq!(deps.get("c"), Some(&vec!["a".to_string()]));
        assert!(!deps.contains_key("b"));
    }

    /// `#queueatcreate`: enqueueing must not rewrite operator-authored lines.
    ///
    /// The first implementation round-tripped the queue body through
    /// parse -> render, which normalized `- /goal complete the entire queue`
    /// into `/goal complete the entire queue`, silently dropping the operator's
    /// list marker. Operator-visible document text is authoritative, and this is
    /// an insert-only operation, so every pre-existing byte must survive.
    #[test]
    fn enqueue_preserves_operator_authored_lines_verbatim() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- /goal complete the entire queue\n",
            "- Fix the contributing causes of the queue wedge.\n",
            "- ~~do [#struck]~~\n",
            "- \u{23ed}\u{fe0f} do [#skipped]\n",
            "- do [#old]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            "- [ ] [#fresh] a new follow-up\n",
            "- [ ] [#old] older item\n",
            "<!-- /agent:backlog -->\n",
        );

        let updated = enqueue_created_ids_in_content(
            content,
            &["fresh".into()],
            FollowUpQueuePlacement::Prepend,
        )
        .unwrap()
        .expect("the fresh id must enqueue");

        for preserved in [
            "- /goal complete the entire queue",
            "- Fix the contributing causes of the queue wedge.",
            "- ~~do [#struck]~~",
            "- \u{23ed}\u{fe0f} do [#skipped]",
            "- do [#old]",
        ] {
            assert!(
                updated.contains(preserved),
                "existing queue line must survive byte-for-byte: {preserved:?}\n{updated}"
            );
        }
        let fresh = updated.find("- do [#fresh]").expect("fresh head present");
        let goal = updated.find("- /goal").unwrap();
        assert!(fresh < goal, "the follow-up goes to the head:\n{updated}");
    }

    /// `#queueingressorder`: the priority graph is active at ingress, derived
    /// deterministically from `priority=` and `after=` metadata on the item.
    #[test]
    fn enqueue_orders_new_block_by_dependency_then_priority() {
        let content = concat!(
            "---\nagent_doc_session: test\n---\n\n",
            "<!-- agent:queue go -->\n",
            "- do [#existing]\n",
            "<!-- /agent:queue -->\n\n",
            "<!-- agent:backlog priority queue -->\n",
            // Filed order is c, b, a — none of which is the correct run order.
            "- [ ] [#c] priority=1 fast but depends on b\n",
            "- [ ] [#b] priority=5 depends on a\n",
            "- [ ] [#a] priority=9 lowest priority but a prerequisite\n",
            "- [ ] [#existing] already queued\n",
            "<!-- /agent:backlog -->\n",
        );
        // Declare the edges: c after b, b after a.
        let content = content
            .replace("[#c] priority=1 fast", "[#c] priority=1 after=#b fast")
            .replace("[#b] priority=5 depends", "[#b] priority=5 after=#a depends");

        let updated = enqueue_created_ids_in_content(
            &content,
            &["c".into(), "b".into(), "a".into()],
            FollowUpQueuePlacement::Prepend,
        )
        .unwrap()
        .expect("all three enqueue");

        let a = updated.find("- do [#a]").unwrap();
        let b = updated.find("- do [#b]").unwrap();
        let c = updated.find("- do [#c]").unwrap();
        assert!(
            a < b && b < c,
            "a declared `after=` edge is a hard constraint and must beat a better \
             priority= rank and the filed order:\n{updated}"
        );
    }

    /// Splicing must never introduce a blank line or disturb existing spacing.
    #[test]
    fn enqueue_never_introduces_blank_lines() {
        let cases = [
            ("normal", "- do [#old]\n"),
            ("empty", ""),
            ("operator trailing blank", "- do [#old]\n\n"),
        ];
        for (label, queue_body) in cases {
            let mut content = String::new();
            content.push_str("---\nagent_doc_session: test\n---\n\n");
            content.push_str("<!-- agent:queue go -->\n");
            content.push_str(queue_body);
            content.push_str("<!-- /agent:queue -->\n\n");
            content.push_str("<!-- agent:backlog priority queue -->\n");
            content.push_str("- [ ] [#fresh] new\n- [ ] [#old] old\n");
            content.push_str("<!-- /agent:backlog -->\n");

            for placement in [
                FollowUpQueuePlacement::Prepend,
                FollowUpQueuePlacement::Append,
            ] {
                let updated =
                    enqueue_created_ids_in_content(&content, &["fresh".into()], placement)
                        .unwrap()
                        .unwrap_or_else(|| panic!("{label}/{placement:?}: expected an enqueue"));
                let queue: Vec<&str> = updated
                    .lines()
                    .skip_while(|l| !l.starts_with("<!-- agent:queue"))
                    .skip(1)
                    .take_while(|l| !l.starts_with("<!-- /agent:queue"))
                    .collect();
                // Byte-preservation cuts both ways: a blank line the OPERATOR
                // authored must survive. Assert we INTRODUCE none, and never at
                // the head (the reported regression).
                let before_blanks = queue_body.lines().filter(|l| l.trim().is_empty()).count();
                let after_blanks = queue.iter().filter(|l| l.trim().is_empty()).count();
                assert_eq!(
                    after_blanks, before_blanks,
                    "{label}/{placement:?}: blank-line count changed: {queue:?}"
                );
                assert!(
                    !queue.first().is_some_and(|l| l.trim().is_empty()),
                    "{label}/{placement:?}: blank line at queue head: {queue:?}"
                );
                assert!(
                    queue.contains(&"- do [#fresh]"),
                    "{label}/{placement:?}: missing head: {queue:?}"
                );
            }
        }
    }

    #[test]
    fn placement_parses_operator_spellings() {
        assert_eq!(
            FollowUpQueuePlacement::parse("prepend"),
            Some(FollowUpQueuePlacement::Prepend)
        );
        assert_eq!(
            FollowUpQueuePlacement::parse("TOP"),
            Some(FollowUpQueuePlacement::Prepend)
        );
        assert_eq!(
            FollowUpQueuePlacement::parse("append"),
            Some(FollowUpQueuePlacement::Append)
        );
        assert_eq!(FollowUpQueuePlacement::parse("sideways"), None);
        assert_eq!(
            FollowUpQueuePlacement::default(),
            FollowUpQueuePlacement::Prepend
        );
    }
}
