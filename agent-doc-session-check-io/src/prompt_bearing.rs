use anyhow::Result;
use std::path::Path;

/// Timed at the definition so every branch that reaches it is attributed
/// to one total (`#sessioncheckprofile`).
pub fn detect_unstarted_prompt_bearing_diff(file: &Path) -> Result<Option<String>> {
    crate::profile::timed("detect_unstarted_prompt_bearing_diff", || {
        detect_unstarted_prompt_bearing_diff_inner(file)
    })
}

fn detect_unstarted_prompt_bearing_diff_inner(file: &Path) -> Result<Option<String>> {
    let set = realtime_steering_set_since_turn_baseline(file)?;
    let steering = set.primary();
    let Some(label) = steering.label() else {
        return Ok(None);
    };
    // `#realtime-steering-verbatim` + `#realtime-steering-aggregate`: surface EVERY
    // operator prompt added mid-turn in FULL (not a first-line preview, and not just
    // the first of several). The operator can add multiple concurrent steering
    // directives while the turn is active; all of them must reach the agent at once
    // so it can address them together and find patterns across them, rather than
    // draining one head at a time.
    let verbatim = set
        .verbatim_aggregate()
        .or_else(|| steering.verbatim().map(str::to_string))
        .unwrap_or_default();
    Ok(Some(format!("{label}: {verbatim}")))
}

pub fn realtime_steering_set_since_turn_baseline(
    file: &Path,
) -> Result<agent_doc_document_realtime::baseline_comparison::RealtimeSteeringSet> {
    // `#steeringobservableset`: an open turn's controller projection is the
    // durable Lazily authority. Its identity-keyed set emptiness is the answer;
    // do not independently HEAD↔buffer re-diff and create a second steering
    // policy owner during closeout.
    let current = crate::resolve_current_document_content(file, "unstarted_prompt_bearing")?;
    let current_content_hash = agent_doc_hash::content_hash(&current);
    if let Some(document) = agent_doc_cycle_state_io::load_document_projection(file)?
        && let Some(set) = projected_open_turn_steering_set(
            document.closeout.phase,
            &document.closeout.realtime_steering,
            &current_content_hash,
        )
    {
        return Ok(set);
    }

    let head = agent_doc_git_io::revision::show_head(file)?;
    if let Some(head) = head.as_deref() {
        // Mirror `realtime_steering_since_turn_baseline`: if nothing is unresolved
        // against HEAD, there is no steering; otherwise fall through to the more
        // precise snapshot turn baseline so the two paths never disagree.
        let set_from_head =
            agent_doc_document_realtime::baseline_comparison::BaselineComparison::new(
                head, &current,
            )
            .realtime_steering_all();
        if !set_from_head.is_present() {
            return Ok(set_from_head);
        }
    }

    let baseline = match agent_doc_snapshot_io::load_document_baseline(file)? {
        Some(snapshot) => snapshot,
        None => head.unwrap_or_default(),
    };
    Ok(
        agent_doc_document_realtime::baseline_comparison::BaselineComparison::new(
            &baseline, &current,
        )
        .realtime_steering_all(),
    )
}

pub fn realtime_steering_since_turn_baseline(
    file: &Path,
) -> Result<agent_doc_document_realtime::baseline_comparison::RealtimeSteering> {
    Ok(realtime_steering_set_since_turn_baseline(file)?.primary())
}

fn projected_open_turn_steering_set(
    phase: Option<agent_doc_turn::CyclePhase>,
    steering: &agent_doc_turn::cp_projection::TurnSteeringProjection,
    current_content_hash: &str,
) -> Option<agent_doc_document_realtime::baseline_comparison::RealtimeSteeringSet> {
    phase
        .filter(|phase| phase.is_open())
        .filter(|_| steering.observed_content_hash.as_deref() == Some(current_content_hash))
        .map(|_| {
            agent_doc_document_realtime::baseline_comparison::RealtimeSteeringSet::from_turn_projection(
                steering,
            )
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_turn::cp_projection::{
        TurnSteeringElementProjection, TurnSteeringProjection, TurnSteeringState,
    };
    use std::collections::BTreeMap;

    #[test]
    fn open_turn_uses_identity_set_emptiness_without_rediff() {
        let empty_projection =
            TurnSteeringProjection::none().with_observed_content_hash("current-hash");
        let empty = projected_open_turn_steering_set(
            Some(agent_doc_turn::CyclePhase::PreflightStarted),
            &empty_projection,
            "current-hash",
        )
        .unwrap();
        assert!(empty.is_empty());

        let elements = BTreeMap::from([(
            "directive-id".to_string(),
            TurnSteeringElementProjection {
                state: TurnSteeringState::PromptTarget,
                ordinal: 0,
                preview: Some("new prompt".into()),
                verbatim: "new prompt body".into(),
            },
        )]);
        let projected = TurnSteeringProjection::observed_identity_set(
            TurnSteeringState::PromptTarget,
            Some("new prompt".into()),
            Some("new prompt body".into()),
            elements,
        )
        .with_observed_content_hash("current-hash");
        let set = projected_open_turn_steering_set(
            Some(agent_doc_turn::CyclePhase::ResponseCaptured),
            &projected,
            "current-hash",
        )
        .unwrap();
        assert_eq!(set.len(), 1);
        assert_eq!(set.primary().verbatim(), Some("new prompt body"));
    }

    #[test]
    fn closed_turn_does_not_mask_fresh_prompt_fallback() {
        assert!(
            projected_open_turn_steering_set(
                Some(agent_doc_turn::CyclePhase::Committed),
                &TurnSteeringProjection::none(),
                "current-hash",
            )
            .is_none()
        );
    }

    #[test]
    fn unobserved_or_stale_open_turn_does_not_mask_fallback() {
        assert!(
            projected_open_turn_steering_set(
                Some(agent_doc_turn::CyclePhase::PreflightStarted),
                &TurnSteeringProjection::none(),
                "current-hash",
            )
            .is_none()
        );
        assert!(
            projected_open_turn_steering_set(
                Some(agent_doc_turn::CyclePhase::PreflightStarted),
                &TurnSteeringProjection::none().with_observed_content_hash("old-hash"),
                "current-hash",
            )
            .is_none()
        );
    }
}
