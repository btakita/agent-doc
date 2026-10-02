//! GH 90 — a durable capture whose response bytes can never land.
//!
//! A captured response is retried by the supervisor idle watch, the Codex
//! `Stop` hook, `agent-doc repair --resume-capture`, and `session-check`. When
//! the refusal is a property of the captured BYTES — the replay guard's
//! component-dump rule or the structural-corruption gate, both evaluated by
//! [`agent_doc_template::response_materialization::response_landability_refusal`]
//! — no editor, controller, or document state edge can change the outcome, so
//! every retry fails identically. Observed on `tasks/laptop/laptop.md`: one
//! capture retried for 3h38m while the loop blamed an unrelated (and dead)
//! editor endpoint, and the true structural reason was written only to
//! `.agent-doc/repair-blocked/` and `doctor` output.
//!
//! This module answers one question for every retry surface: is the CURRENT
//! capture deterministically unlandable, and if so, what exactly unblocks it.

use std::path::Path;

use anyhow::Result;

/// The operator recovery for a deterministically unlandable capture: re-capture
/// the same response with its marker text escaped, then land it through the
/// normal strict closeout.
pub const REQUOTE_UNLANDABLE_CAPTURE_FLAG: &str = "--requote-unlandable-capture";

/// Stable token stamped into every unlandable-capture report, so log greps and
/// cross-process classifiers match it by construction rather than by wording.
pub const UNLANDABLE_CAPTURE_TOKEN: &str = "captured_response_unlandable";

/// A capture that no retry can land, with the reason the validators gave.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnlandableCapture {
    pub cycle_id: String,
    pub capture_id: String,
    pub response_sha256: String,
    pub reason: String,
}

impl UnlandableCapture {
    /// The exact command that clears this capture.
    pub fn recovery_command(file: &Path) -> String {
        format!(
            "agent-doc repair {} {}",
            file.display(),
            REQUOTE_UNLANDABLE_CAPTURE_FLAG
        )
    }

    /// Operator-facing report: the structural reason first, then the one
    /// command that changes the outcome. Never "retry session-check": a retry
    /// is guaranteed to fail identically.
    pub fn operator_report(&self, file: &Path) -> String {
        format!(
            "captured response {} (cycle {}, response_sha256 {}) for `{}` can never land: {} [{UNLANDABLE_CAPTURE_TOKEN}]. Retrying cannot change this; the capture is retained unchanged. Run `{}` to re-capture the same response with its component-marker text escaped and land it through the normal closeout.",
            self.capture_id,
            self.cycle_id,
            self.response_sha256,
            file.display(),
            self.reason,
            Self::recovery_command(file),
        )
    }
}

/// The response body a resume would replay for `capture` — the same choice
/// `resume_captured_finalize` makes.
pub fn captured_replay_body(capture: &agent_doc_cycle_state_io::ProjectedCapturedResponse) -> &str {
    capture
        .intent_body
        .as_deref()
        .unwrap_or(capture.response_body.as_str())
}

/// Classify one capture's bytes. Pure apart from the capture it is handed.
pub fn unlandable_capture_reason(
    capture: &agent_doc_cycle_state_io::ProjectedCapturedResponse,
) -> Option<UnlandableCapture> {
    let reason = agent_doc_template::response_materialization::response_landability_refusal(
        captured_replay_body(capture),
    )?;
    Some(UnlandableCapture {
        cycle_id: capture.cycle_id.clone(),
        capture_id: capture.capture_id.clone(),
        response_sha256: capture.response_sha256.clone(),
        reason,
    })
}

/// The capture a resume would replay right now, if it would replay one.
///
/// Mirrors `captured_finalize_resume_key`: a retained continuation first, then
/// the open cycle's own capture. A capture already materialized (`write_applied`
/// or later) is excluded — its response bytes already landed, so they are not
/// what blocks it.
pub fn current_replayable_capture(
    file: &Path,
) -> Result<Option<agent_doc_cycle_state_io::ProjectedCapturedResponse>> {
    let state = agent_doc_cycle_state_io::load_with_closeout_projection(file)?;
    let materialized = state.as_ref().is_some_and(|state| {
        matches!(
            state.phase,
            agent_doc_turn::CyclePhase::WriteApplied | agent_doc_turn::CyclePhase::Committed
        )
    });
    if let Some(capture) =
        agent_doc_cycle_state_io::load_projected_retained_captured_response(file)?
            .filter(|capture| !capture.response_body.trim().is_empty())
    {
        let same_cycle_materialized = materialized
            && state
                .as_ref()
                .is_some_and(|state| state.cycle_id == capture.cycle_id);
        return Ok((!same_cycle_materialized).then_some(capture));
    }
    let Some(state) = state else {
        return Ok(None);
    };
    if state.phase != agent_doc_turn::CyclePhase::ResponseCaptured {
        return Ok(None);
    }
    let (Some(capture_id), Some(response_sha256)) = (
        state.capture_id.as_deref(),
        state.response_sha256.as_deref(),
    ) else {
        return Ok(None);
    };
    Ok(
        agent_doc_cycle_state_io::load_projected_captured_response(file, capture_id)?.filter(
            |capture| {
                capture.cycle_id == state.cycle_id
                    && capture.response_sha256 == response_sha256
                    && !capture.response_body.trim().is_empty()
            },
        ),
    )
}

/// Whether the capture a resume would replay right now is deterministically
/// unlandable.
pub fn current_unlandable_capture(file: &Path) -> Result<Option<UnlandableCapture>> {
    Ok(current_replayable_capture(file)?
        .as_ref()
        .and_then(unlandable_capture_reason))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(body: &str) -> agent_doc_cycle_state_io::ProjectedCapturedResponse {
        agent_doc_cycle_state_io::ProjectedCapturedResponse {
            cycle_id: "cycle-1".to_string(),
            capture_id: "cycle-1".to_string(),
            response_sha256: "sha".to_string(),
            response_body: body.to_string(),
            intent_body: None,
            mutation_plan_json: None,
            file_hash: None,
            snapshot_hash: None,
            baseline_content: None,
        }
    }

    #[test]
    fn gh90_marker_prose_in_backticks_is_a_landable_capture() {
        assert_eq!(
            unlandable_capture_reason(&capture(
                "### Re: marker — claude\n\n`i<!-- /agent:queue -->`. Fixed: marker restored.\n"
            )),
            None
        );
    }

    #[test]
    fn gh90_unlandable_capture_report_names_reason_and_exact_recovery() {
        let unlandable = unlandable_capture_reason(&capture(
            "### Re: x — claude\n\n<!-- agent:queue -->\n- leaked\n<!-- /agent:queue -->\n",
        ))
        .expect("a component dump can never land");
        let report = unlandable.operator_report(Path::new("tasks/laptop.md"));
        assert!(report.contains("full document component dump"), "{report}");
        assert!(report.contains(UNLANDABLE_CAPTURE_TOKEN), "{report}");
        assert!(
            report.contains("agent-doc repair tasks/laptop.md --requote-unlandable-capture"),
            "{report}"
        );
        assert!(
            !report.contains("Retry only"),
            "a deterministic refusal must not prescribe a retry: {report}"
        );
    }
}
