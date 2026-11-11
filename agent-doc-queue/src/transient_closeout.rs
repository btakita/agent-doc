//! Normal-operation queue closeout (`#transientcloseout`).
//!
//! Every live row in the transient set must leave that state before a normal
//! turn ends. Resting rows may remain. `📥` is the inbox glyph preflight
//! paints on work this turn did not run: the next queue head, a sibling left
//! for later, and a standing `agent:loop` program (`#queueschedule`). That
//! inbox is resting, the same as a bare row. A committed response's
//! session-check also leaves a plain `🚧` worker and a `☑️` handoff in
//! place. A genuine turn end still refuses those two. An interrupted or
//! crashed turn
//! may leave transient rows in the document; that leftover is not a successful
//! closeout, and the next turn owes the same transitions. This module does not
//! mark a running claim Lost, does not delete
//! [`crate::queue_claim::QueueClaimState::Lost`], and does not change
//! [`crate::queue_claim::QueueClaim::mark_lost`] or
//! [`crate::queue_claim::QueueClaim::project_expiry`].

use anyhow::Result;

use agent_doc_document::queue_projection::QueueLifecycleMarker;

use crate::document_queue::{QueueEntry, parse as parse_queue_body};

/// One live queue row that still owes a transition before a normal closeout.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransientCloseoutDebt {
    /// The live row text, including its lifecycle prefix.
    pub row: String,
    pub marker: QueueLifecycleMarker,
    /// The transition a normal turn still owes for this row.
    pub owed: &'static str,
}

/// Whether `marker` blocks a genuine turn end (`#transientcloseout`).
///
/// The turn-end set is `🟡` `🚧` `☑️` `⚠️` `⚠️🟡` `⚠️🚧` `⚠️☑️` `🔄` `❔`.
/// `📥` is resting inbox. A committed response's session-check is narrower:
/// `🚧` and `☑️` stay up between turns (a worker in flight, a handoff not
/// yet integrated) and do not refuse that check. `⛔`, `⏳`, and `⏭️` are
/// not transient. `🙋` is an orthogonal resting annotation, not a lifecycle
/// marker.
pub fn is_transient_closeout_marker(marker: QueueLifecycleMarker) -> bool {
    marker_owes_closeout(marker, DebtScope::TurnEnd)
}

/// A committed response may leave inbox, in-flight, and reported rows.
/// Ending the turn may not leave a worker running or a report unintegrated.
#[derive(Clone, Copy)]
enum DebtScope {
    Session,
    TurnEnd,
}

fn marker_owes_closeout(marker: QueueLifecycleMarker, scope: DebtScope) -> bool {
    match marker {
        QueueLifecycleMarker::Pending
        | QueueLifecycleMarker::Blocked
        | QueueLifecycleMarker::AwaitingOperator
        | QueueLifecycleMarker::Skipped => false,
        QueueLifecycleMarker::Running | QueueLifecycleMarker::Reported => {
            matches!(scope, DebtScope::TurnEnd)
        }
        QueueLifecycleMarker::Claimed
        | QueueLifecycleMarker::Lost
        | QueueLifecycleMarker::LostClaimed
        | QueueLifecycleMarker::LostRunning
        | QueueLifecycleMarker::LostReported
        | QueueLifecycleMarker::Reconciling
        | QueueLifecycleMarker::Unknown => true,
    }
}

/// The transition a normal turn still owes for `marker`.
///
/// Resting markers return `None`.
pub fn owed_closeout_transition(marker: QueueLifecycleMarker) -> Option<&'static str> {
    Some(match marker {
        QueueLifecycleMarker::Claimed => {
            "start the claimed worker or otherwise leave claimed; a head that is only claimed means the turn is not over"
        }
        QueueLifecycleMarker::Running => {
            "finish the running worker without converting the claim to Lost; a worker that is still running means the turn is not over; a frozen 🚧 whose continue already failed authorizes a new generation in the same turn"
        }
        QueueLifecycleMarker::Reported => {
            "integrate the report or otherwise leave the reported-but-open state"
        }
        QueueLifecycleMarker::Lost
        | QueueLifecycleMarker::LostClaimed
        | QueueLifecycleMarker::LostRunning
        | QueueLifecycleMarker::LostReported => {
            "continue the same generation and worker; if continue fails, retry a new generation in this turn so the row leaves Lost"
        }
        QueueLifecycleMarker::Reconciling => {
            "reconcile the row or close it; a strike retires the completion cell"
        }
        QueueLifecycleMarker::Unknown => "diagnose the contradiction and clear it",
        QueueLifecycleMarker::Pending
        | QueueLifecycleMarker::Blocked
        | QueueLifecycleMarker::AwaitingOperator
        | QueueLifecycleMarker::Skipped => return None,
    })
}

/// Live, unstruck queue rows whose lifecycle marker is still transient
/// for a committed response's session-check.
///
/// `📥` is not debt. `🚧` and `☑️` are not debt here either: a worker may
/// still be in flight, and a reported handoff may still be waiting for the
/// coordinator. A genuine turn end uses [`live_turn_end_debts`].
pub fn live_transient_debts(content: &str) -> Result<Vec<TransientCloseoutDebt>> {
    live_debts(content, DebtScope::Session)
}

/// Live rows that block ending the turn. Includes `🚧` and `☑️`.
pub fn live_turn_end_debts(content: &str) -> Result<Vec<TransientCloseoutDebt>> {
    live_debts(content, DebtScope::TurnEnd)
}

fn live_debts(content: &str, scope: DebtScope) -> Result<Vec<TransientCloseoutDebt>> {
    let components = agent_doc_element::element::parse(content)?;
    let mut debts = Vec::new();
    for component in components
        .iter()
        .filter(|component| component.name == "queue")
    {
        for entry in parse_queue_body(component.content(content))? {
            let QueueEntry::Prompt(prompt) = entry else {
                continue;
            };
            let Some(marker) = agent_doc_document::queue_projection::lifecycle_marker(&prompt.text)
            else {
                continue;
            };
            if !marker_owes_closeout(marker, scope) {
                continue;
            }
            let Some(owed) = owed_closeout_transition(marker) else {
                continue;
            };
            debts.push(TransientCloseoutDebt {
                row: prompt.text.trim().to_string(),
                marker,
                owed,
            });
        }
    }
    Ok(debts)
}

fn format_failure(header: &str, debts: &[TransientCloseoutDebt]) -> Option<String> {
    if debts.is_empty() {
        return None;
    }
    let mut message = header.to_string();
    for debt in debts {
        message.push_str("\n- row ");
        message.push_str(&debt.row);
        message.push_str(": ");
        message.push_str(debt.owed);
    }
    Some(message)
}

/// Failure for a committed response's session-check. `None` when every live
/// row is resting for that check, including inbox `📥`, in-flight `🚧`, and
/// reported `☑️`.
pub fn normal_closeout_failure(content: &str) -> Result<Option<String>> {
    Ok(format_failure(
        "normal closeout refused while live queue rows are still transient (#transientcloseout)",
        &live_transient_debts(content)?,
    ))
}

/// Failure for a genuine turn end (Stop / `end_turn`). A running worker or
/// an unintegrated report still owes a transition.
pub fn turn_end_closeout_failure(content: &str) -> Result<Option<String>> {
    Ok(format_failure(
        "normal closeout refused while live queue rows are still transient (#transientcloseout)",
        &live_turn_end_debts(content)?,
    ))
}

/// Crash recovery may leave the document transient. It must not report that
/// leftover as a successful closeout. The next turn finishes the transitions.
pub fn crash_recovery_closeout_failure(content: &str) -> Result<Option<String>> {
    Ok(format_failure(
        "crash recovery refused a successful closeout (#transientcloseout): leftover transient queue rows remain; the next turn finishes these transitions and this recovery does not treat them as closed",
        &live_turn_end_debts(content)?,
    ))
}

/// Choose the normal or crash-recovery failure for an otherwise successful
/// closeout message. Crash recovery is the path that recovered a missing
/// commit boundary. `None` when the document has no transient live row.
pub fn closeout_failure_for_success_message(
    success_message: &str,
    content: &str,
) -> Result<Option<String>> {
    if success_message.contains("recovered the missing commit boundary") {
        crash_recovery_closeout_failure(content)
    } else {
        normal_closeout_failure(content)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(queue: &str) -> String {
        format!("<!-- agent:queue -->\n{queue}<!-- /agent:queue -->\n")
    }

    #[test]
    fn transient_rows_name_the_owed_transition_and_resting_rows_do_not() {
        let content = doc("\
- ~~integrated history~~
- ⛔ blocked on other open work
- ⏳ awaiting operator
- 🙋 operator action
- ⏭️ skipped earlier
- bare pending text is not a transient glyph
- 📥 still pending
- 🟡 only claimed
- 🚧 still running
- ☑️ reported but open
- ⚠️ lost
- ⚠️🟡 lost claimed
- ⚠️🚧 lost running
- ⚠️☑️ lost worker done
- 🔄 needs reconcile
- ❔ unknown contradiction
");
        let failure = turn_end_closeout_failure(&content)
            .unwrap()
            .expect("transient rows refuse a turn end");
        assert!(failure.starts_with(
            "normal closeout refused while live queue rows are still transient (#transientcloseout)"
        ));
        for row in [
            "🟡 only claimed",
            "🚧 still running",
            "☑️ reported but open",
            "⚠️ lost",
            "⚠️🟡 lost claimed",
            "⚠️🚧 lost running",
            "⚠️☑️ lost worker done",
            "🔄 needs reconcile",
            "❔ unknown contradiction",
        ] {
            assert!(failure.contains(row), "missing {row} in {failure}");
        }
        assert!(failure.contains("finish the running worker without converting the claim to Lost"));
        assert!(failure.contains(
            "a frozen 🚧 whose continue already failed authorizes a new generation in the same turn"
        ));
        assert!(failure.contains(
            "continue the same generation and worker; if continue fails, retry a new generation in this turn so the row leaves Lost"
        ));
        assert!(
            failure.contains("integrate the report or otherwise leave the reported-but-open state")
        );
        assert!(
            failure.contains("reconcile the row or close it; a strike retires the completion cell")
        );
        assert!(failure.contains("diagnose the contradiction and clear it"));
        for resting in [
            "integrated history",
            "blocked on other open work",
            "awaiting operator",
            "operator action",
            "skipped earlier",
            "bare pending text",
            "📥 still pending",
        ] {
            assert!(
                !failure.contains(resting),
                "resting row {resting} was named in {failure}"
            );
        }

        let crash = crash_recovery_closeout_failure(&content)
            .unwrap()
            .expect("crash recovery is not success");
        assert!(crash.contains("crash recovery refused a successful closeout"));
        assert!(crash.contains("next turn finishes these transitions"));
        assert!(crash.contains("🚧 still running"));
        assert!(!crash.contains("normal closeout refused"));
    }

    #[test]
    fn a_resting_queue_does_not_refuse_closeout() {
        let content = doc("\
- ~~integrated history~~
- ⛔ blocked on other open work
- ⏳ awaiting operator
- 🙋 operator action
");
        assert!(normal_closeout_failure(&content).unwrap().is_none());
        assert!(crash_recovery_closeout_failure(&content).unwrap().is_none());
        assert!(
            closeout_failure_for_success_message(
                "[session-check] ok — recovered the missing commit boundary from head",
                &content
            )
            .unwrap()
            .is_none()
        );
    }

    /// `#queueschedule`: an `agent:loop` program stays `📥` between passes.
    /// That is not this turn's closeout debt. A loop row that is actually
    /// running still is.
    #[test]
    fn standing_loop_pending_is_not_closeout_debt() {
        let content = "\
<!-- agent:loop id=issues interval=1h -->
- 📥 poll issues [#q-6790ddebe533]
<!-- /agent:loop -->
<!-- agent:run go -->
- ⏳ do [#head]
<!-- /agent:run -->
";
        assert!(
            normal_closeout_failure(content).unwrap().is_none(),
            "a standing loop inbox row must not refuse closeout"
        );

        let running = "\
<!-- agent:loop id=issues interval=1h -->
- 🚧 poll issues
<!-- /agent:loop -->
";
        let failure = turn_end_closeout_failure(running)
            .unwrap()
            .expect("a running loop pass is still this turn's debt");
        assert!(
            normal_closeout_failure(running).unwrap().is_none(),
            "a committed response may leave a worker in flight"
        );
        assert!(failure.contains("🚧 poll issues"), "{failure}");
        assert!(failure.contains("normal closeout refused"), "{failure}");

        let legacy = "\
<!-- agent:queue loop -->
- 📥 legacy loop item
<!-- /agent:queue -->
";
        assert!(
            normal_closeout_failure(legacy).unwrap().is_none(),
            "legacy `agent:queue loop` is the same standing program"
        );
    }

    #[test]
    fn a_document_without_a_queue_has_no_transient_debt() {
        let content = "<!-- agent:exchange -->\n### Re: note\n\nDone.\n<!-- /agent:exchange -->\n";
        assert!(normal_closeout_failure(content).unwrap().is_none());
    }

    /// sdk-dev.md, 2026-11-11: a heading-level running leaf and a nested
    /// lost-running leaf under an `agent:run` pull-request parent both stayed
    /// live when the turn ended. Both are transient debt.
    #[test]
    fn sdk_dev_heading_and_nested_run_rows_are_transient_debt() {
        let content = "\
<!-- agent:run id=\"haiven-sdk-prs\" subagents -->
- ### https://github.com/haiven-dev/haiven-sdk/pull/228
  - %% ci(release): dispatch sdk-dev promotion from main %%
  - ⚠️🚧 [#subagent:pr228seventeen#17] #adversarial-pr [#q-abf0adec8270]
- ### 🚧 [#subagent:rpversionpr#1] Do we need to create a PR to make Release Please reliably update the version [#q-3a7937eee419]
<!-- /agent:run -->
";
        let failure = turn_end_closeout_failure(content)
            .unwrap()
            .expect("both sdk-dev rows refuse a turn end");
        let session = normal_closeout_failure(content)
            .unwrap()
            .expect("lost-running still refuses a committed closeout");
        assert!(
            session
                .contains("⚠️🚧 [#subagent:pr228seventeen#17] #adversarial-pr [#q-abf0adec8270]"),
            "{session}"
        );
        assert!(
            !session.contains("🚧 [#subagent:rpversionpr#1]"),
            "a plain in-flight worker does not refuse session-check: {session}"
        );
        assert!(
            failure.contains(
                "🚧 [#subagent:rpversionpr#1] Do we need to create a PR to make Release Please reliably update the version [#q-3a7937eee419]"
            ),
            "{failure}"
        );
        assert!(
            failure
                .contains("⚠️🚧 [#subagent:pr228seventeen#17] #adversarial-pr [#q-abf0adec8270]"),
            "{failure}"
        );
        assert!(failure.contains("finish the running worker without converting the claim to Lost"));
        assert!(failure.contains(
            "a frozen 🚧 whose continue already failed authorizes a new generation in the same turn"
        ));
        assert!(failure.contains(
            "continue the same generation and worker; if continue fails, retry a new generation in this turn so the row leaves Lost"
        ));
        assert!(
            !failure.contains("ci(release)"),
            "a comment child is not a transient row: {failure}"
        );
    }
}
