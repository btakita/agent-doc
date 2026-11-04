//! `#managedlistnodrift`: `agent:active` is rendered from claims, release
//! receipts, and lineage, never from the queue markers it would read back.

use std::collections::HashMap;

use agent_doc_element_queue::QueueItemIdentity;
use agent_doc_queue::claim_release_receipt::ClaimReleaseReceiptLedger;
use agent_doc_queue::document_queue::QueueEntry;
use agent_doc_queue::queue_claim::QueueClaimLedger;
use agent_doc_work_projection::active_lifecycle::{ActiveLifecycle, LineageFacts};
use agent_doc_work_projection::managed_list::{ManagedList, component_bodies, laws};
use proptest::prelude::*;
use serde_json::json;

const FIXTURE: &str = include_str!("fixtures/active_ghosts_session.md");
/// 2026-11-04T04:00:00Z
const NOW: u64 = 1_793_764_800;

/// The work identity the queue projection joins for the row containing
/// `needle`, computed exactly as the projection does.
fn identity_of(doc: &str, needle: &str) -> QueueItemIdentity {
    let components = agent_doc_element::element::parse(doc).unwrap();
    let set = agent_doc_queue::queue_set::QueueSet::parse(doc, &components).unwrap();
    for block in set.blocks() {
        for (index, entry) in block.entries.iter().enumerate() {
            if let QueueEntry::Prompt(prompt) = entry
                && prompt.text.contains(needle)
            {
                return agent_doc_queue::queue_claim::claim_identity_at(&block.entries, index)
                    .unwrap_or_else(|| agent_doc_queue::queue_claim::claim_identity(&prompt.text));
            }
        }
    }
    panic!("no queue row contains {needle:?}");
}

fn claim(doc: &str, needle: &str, owner: &str, state: &str, at: u64) -> serde_json::Value {
    json!({
        "identity": identity_of(doc, needle),
        "item_text": needle,
        "owner": owner,
        "generation": 1,
        "state": state,
        "worker": {"worker_id": format!("w-{owner}"), "started_at_secs": at + 60},
        "claimed_at_secs": at,
        "expires_at_secs": NOW + 3600,
    })
}

fn receipt(doc: &str, needle: &str, owner: &str, released: u64) -> serde_json::Value {
    json!({
        "identity": identity_of(doc, needle),
        "item_text": needle,
        "owner": owner,
        "generation": 1,
        "completion_recovered_after_loss": false,
        "claimed_at_secs": released - 600,
        "released_at_secs": released,
        "expires_at_secs": NOW + 86_400,
    })
}

/// The durable records behind the live 2026-11-03 document: 5 claims and 5
/// receipts, of which only the claims and one receipt back real queue work.
/// The rest were dispatched onto response-packet rows (offered Choices and
/// operator replies under Waiting-on-you items), and one `☑️` row has no
/// record at all.
fn lifecycle(doc: &str) -> ActiveLifecycle {
    let claims: QueueClaimLedger = serde_json::from_value(json!({
        "claims": [
            claim(doc, "#adversarial-pr ⟦effect:1⟧ [#q-run834]", "subagent:c497api834", "running", NOW - 3600),
            claim(doc, "#adversarial-pr [#q-unity1]", "subagent:c768unity", "reported", NOW - 30 * 3600),
            claim(doc, "#rebase [#q-rebase9]", "subagent:c505rebase", "reported", NOW - 6 * 3600),
            claim(doc, "What are the next steps for the docs websites?", "subagent:c497docs", "reported", NOW - 2 * 3600),
            claim(doc, "Draft a consolidated list of open questions", "subagent:openquestions", "reported", NOW - 4 * 86_400),
        ],
        "cancellations": [],
    }))
    .unwrap();
    let receipts: ClaimReleaseReceiptLedger = serde_json::from_value(json!({
        "receipts": [
            receipt(doc, "**UI role**", "subagent:0f24a54c", NOW - 50 * 3600),
            receipt(doc, "**Grant both**", "subagent:78f0f743", NOW - 50 * 3600),
            receipt(doc, "do [#q-release]", "subagent:c252releaseapp", NOW - 40 * 3600),
            receipt(doc, "leave it [#q-leave]", "subagent:contracts-suite-leave", NOW - 30 * 3600),
            receipt(doc, "What are the next steps for the docs websites?", "subagent:c497docs", NOW - 2 * 3600),
        ],
    }))
    .unwrap();
    let lineage = HashMap::from([(
        ("subagent:c505rebase".to_string(), 1),
        LineageFacts {
            latest: Some(("integrated".to_string(), NOW - 5 * 3600)),
            integrated: true,
            ended: false,
        },
    )]);
    ActiveLifecycle::from_ledgers(&claims, &receipts, &lineage, NOW)
}

fn active_body(doc: &str) -> String {
    component_bodies(doc, ManagedList::Active).unwrap()[0].to_string()
}

/// Regression for the 2026-11-03 `agent:active` ghosts: 10 "reported/running"
/// entries, most of them leftover `☑️` text on response-packet rows and one
/// with no durable record at all. Rendered from durable state, the list holds
/// exactly the real queue work, split into Running and Needs integration with
/// each needs-integration entry's age.
#[test]
fn active_ghosts_come_from_markers_and_vanish_when_rendered_from_durable_state() {
    let before = active_body(FIXTURE);
    // The marker-derived projection keeps a `☑️` row that no claim, receipt,
    // or lineage record backs: the visible marker is its only evidence. (Since
    // `#waitingpacketowner`, packet rows are no longer executable, so the
    // Choices/reply ghosts already drop out of it.)
    let marker_derived = agent_doc_work_projection::reconcile_managed_active_items(FIXTURE)
        .unwrap()
        .map(|doc| active_body(&doc))
        .unwrap_or_else(|| before.clone());
    assert!(
        marker_derived.contains("[#tdhandoffrelock]"),
        "{marker_derived}"
    );
    assert!(
        !marker_derived.contains("**Needs integration**"),
        "{marker_derived}"
    );

    let durable = lifecycle(FIXTURE);
    let after_doc = agent_doc_work_projection::reconcile_managed_active_items_from_read(
        FIXTURE,
        &agent_doc_work_projection::active_lifecycle::ActiveLifecycleRead::Loaded(durable.clone()),
    )
    .unwrap()
    .expect("the ghosted list is drift");
    let after = active_body(&after_doc);
    assert_eq!(
        after,
        concat!(
            "- **Running** (1)\n",
            "  - `[#q-run834]` — #adversarial-pr ⟦effect:1⟧\n",
            "    - **Status:** running\n",
            "- **Needs integration** (4)\n",
            "  - `[#q-unity1]` — #adversarial-pr [#q-unity1]\n",
            "    - **Status:** reported\n",
            "    - **Age:** 29h (since started 2026-11-02T22:01Z)\n",
            "  - `[#q-rebase9]` — #rebase [#q-rebase9]\n",
            "    - **Status:** reported (integrated; closeout pending)\n",
            "    - **Age:** 5h (since integrated 2026-11-03T23:00Z)\n",
            "  - `queue-free:0091285b994a` — What are the next steps for the docs websites?\n",
            "    - **Status:** reported\n",
            "    - **Age:** 1h (since started 2026-11-04T02:01Z)\n",
            "  - `[#q-questions]` — Draft a consolidated list of open questions in a md file.\n",
            "    - **Status:** reported\n",
            "    - **Age:** 3d (since started 2026-10-31T04:01Z)\n",
            "    - **Sources:** backlog (open)\n",
        ),
        "before:\n{before}\nafter:\n{after}"
    );
    // A second render is byte-identical.
    assert!(
        agent_doc_work_projection::reconcile_managed_active_items_with(&after_doc, &durable)
            .unwrap()
            .is_none()
    );
}

/// The projection never consults the visible marker: stripping or swapping
/// every lifecycle prefix in the queues leaves the rendered list unchanged.
#[test]
fn active_render_ignores_visible_queue_markers() {
    let durable = lifecycle(FIXTURE);
    let render = |doc: &str| {
        let projected =
            agent_doc_work_projection::reconcile_managed_active_items_with(doc, &durable)
                .unwrap()
                .unwrap_or_else(|| doc.to_string());
        active_body(&projected)
    };
    let baseline = render(FIXTURE);
    let swapped = FIXTURE
        .replace("☑️ [#subagent", "🚧 [#subagent")
        .replace("🚧 [#subagent:c497api834", "☑️ [#subagent:c497api834");
    let bare = FIXTURE.replace("☑️ ", "").replace("🚧 ", "");
    assert_eq!(render(&swapped), baseline);
    assert_eq!(render(&bare), baseline);
}

#[derive(Debug, Clone)]
enum Event {
    Claim(usize),
    Start(usize),
    Report(usize),
    Release(usize),
    Integrate(usize),
    Cancel(usize),
    Expire(usize),
}

/// A model durable store for a fixed set of queue rows.
#[derive(Debug, Clone, PartialEq)]
struct Store {
    doc: String,
    rows: Vec<&'static str>,
    claims: Vec<Option<(String, u64, bool)>>, // (state, at, expired)
    receipts: Vec<Option<u64>>,
    integrated: Vec<bool>,
}

const ROWS: [&str; 4] = [
    "alpha [#q-a]",
    "beta [#q-b]",
    "gamma [#q-c]",
    "delta [#q-d]",
];

fn model_doc() -> String {
    let mut doc = String::from(
        "## Active <!-- agent:active -->\n<!-- /agent:active -->\n## Run <!-- agent:run id=\"main\" -->\n",
    );
    for row in ROWS {
        doc.push_str(&format!("- {row}\n"));
    }
    doc.push_str("<!-- /agent:run -->\n");
    doc
}

fn model_lifecycle(store: &Store) -> ActiveLifecycle {
    let mut claims = Vec::new();
    let mut receipts = Vec::new();
    let mut lineage = HashMap::new();
    for (index, row) in store.rows.iter().enumerate() {
        let owner = format!("subagent:w{index}");
        if let Some((state, at, expired)) = &store.claims[index] {
            let mut value = claim(&store.doc, row, &owner, state, *at);
            if *expired {
                value["expires_at_secs"] = json!(at + 1);
            }
            if state == "claimed" {
                value.as_object_mut().unwrap().remove("worker");
            }
            claims.push(value);
        }
        if let Some(released) = store.receipts[index] {
            receipts.push(receipt(&store.doc, row, &owner, released));
        }
        if store.integrated[index] {
            lineage.insert(
                (owner, 1),
                LineageFacts {
                    latest: Some(("integrated".into(), NOW - 10)),
                    integrated: true,
                    ended: false,
                },
            );
        }
    }
    let claims: QueueClaimLedger =
        serde_json::from_value(json!({"claims": claims, "cancellations": []})).unwrap();
    let receipts: ClaimReleaseReceiptLedger =
        serde_json::from_value(json!({"receipts": receipts})).unwrap();
    ActiveLifecycle::from_ledgers(&claims, &receipts, &lineage, NOW)
}

fn apply(store: &Store, event: &Event) -> Store {
    let mut next = store.clone();
    match *event {
        Event::Claim(i) if next.claims[i].is_none() => {
            next.claims[i] = Some(("claimed".into(), NOW - 100, false))
        }
        Event::Start(i) => {
            if let Some((state, _, _)) = next.claims[i].as_mut().filter(|c| c.0 == "claimed") {
                *state = "running".into();
            }
        }
        Event::Report(i) => {
            if let Some((state, _, _)) = next.claims[i].as_mut().filter(|c| c.0 == "running") {
                *state = "reported".into();
            }
        }
        Event::Release(i) => {
            if next.claims[i].as_ref().is_some_and(|c| c.0 == "reported") {
                next.claims[i] = None;
                next.receipts[i] = Some(NOW - 50);
            }
        }
        Event::Integrate(i) => next.integrated[i] = true,
        Event::Cancel(i) => {
            next.claims[i] = None;
            next.receipts[i] = None;
        }
        Event::Expire(i) => {
            if let Some((state, _, expired)) = next.claims[i].as_mut().filter(|c| c.0 != "reported")
            {
                let _ = state;
                *expired = true;
            }
        }
        _ => {}
    }
    next
}

fn render(store: &Store) -> Store {
    let mut next = store.clone();
    if let Some(doc) = agent_doc_work_projection::reconcile_managed_active_items_with(
        &store.doc,
        &model_lifecycle(store),
    )
    .unwrap()
    {
        next.doc = doc;
    }
    next
}

fn event() -> impl Strategy<Value = Event> {
    let row = 0usize..ROWS.len();
    prop_oneof![
        row.clone().prop_map(Event::Claim),
        row.clone().prop_map(Event::Start),
        row.clone().prop_map(Event::Report),
        row.clone().prop_map(Event::Release),
        row.clone().prop_map(Event::Integrate),
        row.clone().prop_map(Event::Cancel),
        row.prop_map(Event::Expire),
    ]
}

proptest! {
    #[test]
    fn active_render_is_history_free_and_idempotent(events in proptest::collection::vec(event(), 0..30)) {
        let initial = Store {
            doc: model_doc(),
            rows: ROWS.to_vec(),
            claims: vec![None; ROWS.len()],
            receipts: vec![None; ROWS.len()],
            integrated: vec![false; ROWS.len()],
        };
        let rendered = laws::incremental_equals_from_scratch(&initial, &events, apply, render)
            .map_err(TestCaseError::fail)?;
        // Every entry is backed by a durable attempt, and every live attempt
        // is listed exactly once.
        let body = active_body(&rendered.doc);
        for (index, row) in ROWS.iter().enumerate() {
            let id = row.split("[#").nth(1).unwrap().trim_end_matches(']');
            let listed = body.matches(&format!("`[#{id}]`")).count();
            let live = rendered.claims[index].is_some() || rendered.receipts[index].is_some();
            prop_assert_eq!(listed, usize::from(live), "{}", body);
        }
    }
}
