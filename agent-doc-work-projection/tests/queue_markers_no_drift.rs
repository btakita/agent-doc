//! `#managedlistnodrift` property test for queue lifecycle markers: the
//! marker on each row is render(durable attempt), never read back.

use agent_doc_document::queue_projection::{QueueLifecycleMarker, QueueOwnerProjection};
use agent_doc_queue::document_queue::{QueueRowProjection, project_prompt_runtime_projections};
use agent_doc_work_projection::managed_list::laws;
use proptest::prelude::*;

const ROWS: [&str; 3] = ["alpha task", "beta task", "gamma task"];

#[derive(Debug, Clone, PartialEq)]
struct Store {
    body: String,
    /// Per row: the durable attempt (marker, generation), if any.
    attempts: Vec<Option<(QueueLifecycleMarker, u64)>>,
}

#[derive(Debug, Clone)]
enum Event {
    Claim(usize),
    Start(usize),
    Report(usize),
    Lose(usize),
    /// A new attempt on work whose last attempt reported.
    Redispatch(usize),
    Cancel(usize),
}

fn targets(store: &Store) -> Vec<QueueRowProjection> {
    ROWS.iter()
        .zip(&store.attempts)
        .map(|(row, attempt)| match attempt {
            Some((marker, generation)) => QueueRowProjection {
                text: row.to_string(),
                marker: *marker,
                owner: Some(QueueOwnerProjection {
                    owner: "subagent:w".to_string(),
                    generation: *generation,
                }),
                blockers: Vec::new(),
            },
            None => QueueRowProjection {
                text: row.to_string(),
                marker: QueueLifecycleMarker::Pending,
                owner: None,
                blockers: Vec::new(),
            },
        })
        .collect()
}

fn apply(store: &Store, event: &Event) -> Store {
    use QueueLifecycleMarker::*;
    let mut next = store.clone();
    let slot = |i: usize| i % ROWS.len();
    match *event {
        Event::Claim(i) if next.attempts[slot(i)].is_none() => {
            next.attempts[slot(i)] = Some((Claimed, 1))
        }
        Event::Start(i) => {
            if let Some((marker @ Claimed, _)) = next.attempts[slot(i)].as_mut() {
                *marker = Running;
            }
        }
        Event::Report(i) => {
            if let Some((marker @ Running, _)) = next.attempts[slot(i)].as_mut() {
                *marker = Reported;
            }
        }
        Event::Lose(i) => {
            if let Some((marker @ Running, _)) = next.attempts[slot(i)].as_mut() {
                *marker = LostRunning;
            }
        }
        Event::Redispatch(i) => {
            if let Some((Reported, generation)) = next.attempts[slot(i)] {
                next.attempts[slot(i)] = Some((Claimed, generation + 1));
            }
        }
        // Cancelling an unreported attempt clears it; a reported row keeps
        // its record until the coordinator closes it.
        Event::Cancel(i) if !matches!(next.attempts[slot(i)], Some((Reported, _))) => {
            next.attempts[slot(i)] = None;
        }
        _ => {}
    }
    next
}

fn render(store: &Store) -> Store {
    let mut next = store.clone();
    if let Some((body, _)) =
        project_prompt_runtime_projections(&store.body, &targets(store)).unwrap()
    {
        next.body = body;
    }
    next
}

fn event() -> impl Strategy<Value = Event> {
    let row = 0usize..ROWS.len();
    prop_oneof![
        row.clone().prop_map(Event::Claim),
        row.clone().prop_map(Event::Start),
        row.clone().prop_map(Event::Report),
        row.clone().prop_map(Event::Lose),
        row.clone().prop_map(Event::Redispatch),
        row.prop_map(Event::Cancel),
    ]
}

proptest! {
    #[test]
    fn queue_markers_are_history_free_and_idempotent(events in proptest::collection::vec(event(), 0..30)) {
        let body = ROWS.iter().map(|row| format!("- {row}\n")).collect::<String>();
        let initial = Store { body, attempts: vec![None; ROWS.len()] };
        let rendered = laws::incremental_equals_from_scratch(&initial, &events, apply, render)
            .map_err(TestCaseError::fail)?;
        // Each row shows exactly its durable attempt's marker.
        for (line, attempt) in rendered.body.lines().zip(&rendered.attempts) {
            let shown = agent_doc_document::queue_projection::lifecycle_marker(line.trim_start_matches("- "));
            let expected = attempt.map(|(marker, _)| marker).unwrap_or(QueueLifecycleMarker::Pending);
            prop_assert_eq!(shown, Some(expected), "{}", rendered.body);
        }
    }
}
