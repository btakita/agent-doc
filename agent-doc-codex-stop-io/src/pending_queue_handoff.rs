//! Transfer an unfinished queue to the supervisor after response persistence.
//!
//! Canonical queue content, the committed cycle, and supervisor readiness are
//! observed independently. The controller projects the durable handoff; Stop
//! may yield only when the returned receipt still names the requested head.

use std::path::Path;

use agent_doc_controller::status::SupervisorDrainReadiness;
use agent_doc_document::queue_projection::QueueLifecycleMarker;
use agent_doc_queue::queue_continuation::{self, DrainScope};
use agent_doc_queue::transient_closeout::TransientCloseoutAssessment;
use anyhow::Result;

pub(crate) fn hand_off_pending_queue_at_stop(
    file: &Path,
    content: &str,
    assessment: &TransientCloseoutAssessment,
    drain_readiness: impl FnOnce(&Path) -> SupervisorDrainReadiness,
) -> Result<bool> {
    hand_off_pending_queue_at_stop_with(
        file,
        content,
        assessment,
        drain_readiness,
        record_pending_queue_handoff,
    )
}

pub(super) fn hand_off_pending_queue_at_stop_with(
    file: &Path,
    content: &str,
    assessment: &TransientCloseoutAssessment,
    drain_readiness: impl FnOnce(&Path) -> SupervisorDrainReadiness,
    record: impl FnOnce(&Path, &str, &str, usize) -> bool,
) -> Result<bool> {
    if !has_pending_debt(assessment) {
        return Ok(false);
    }
    let Some(cycle) =
        agent_doc_cycle_state_io::load_with_closeout_projection(file)?.filter(|cycle| {
            cycle.phase == agent_doc_turn::CyclePhase::Committed && cycle.response_sha256.is_some()
        })
    else {
        return Ok(false);
    };
    let claimed =
        agent_doc_queue_io::queue_claim::in_flight_claimed_items_for_content(file, content);
    let presets =
        agent_doc_frontmatter_io::session::effective_prompt_presets_for_drain(content, file);
    let Some(head) =
        queue_continuation::live_drainable_continuation_head_excluding_claimed_with_presets(
            content,
            DrainScope::Supervisor,
            &claimed,
            &presets,
        )
    else {
        return Ok(false);
    };
    if !drain_readiness(file).is_ready() {
        return Ok(false);
    }
    let count = queue_continuation::drainable_head_count_excluding_claimed_with_presets(
        content, &claimed, &presets,
    );
    Ok(record(file, &cycle.cycle_id, &head, count))
}

fn record_pending_queue_handoff(file: &Path, cycle_id: &str, head: &str, count: usize) -> bool {
    let receipt =
        match agent_doc_controller_io::project_controller::record_stop_queue_handoff_for_file(
            file, cycle_id, head, count,
        ) {
            Ok(receipt) => receipt,
            Err(error) => {
                agent_doc_ops_log_io::log_op(
                    file,
                    &format!(
                        "stop_pending_queue_handoff_failed cycle_id={} error={error:#}",
                        cycle_id
                    ),
                );
                return false;
            }
        };
    let head_sha256 = agent_doc_hash::content_hash(head);
    if !receipt.is_pending()
        || receipt.cycle_id != cycle_id
        || receipt.head_sha256.as_deref() != Some(head_sha256.as_str())
    {
        return false;
    }
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "stop_pending_queue_handoff cycle_id={} head_sha256={head_sha256} stall_epoch={} action=supervisor_continuation",
            cycle_id, receipt.stall_epoch,
        ),
    );
    true
}

fn has_pending_debt(assessment: &TransientCloseoutAssessment) -> bool {
    assessment
        .debts
        .iter()
        .any(|debt| debt.marker == QueueLifecycleMarker::Pending)
}
