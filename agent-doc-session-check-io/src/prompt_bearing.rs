use anyhow::Result;
use std::path::Path;

pub fn detect_unstarted_prompt_bearing_diff(file: &Path) -> Result<Option<String>> {
    let steering = realtime_steering_since_turn_baseline(file)?;
    let Some(label) = steering.label() else {
        return Ok(None);
    };
    // `#realtime-steering-verbatim`: surface the operator's added prompt in FULL,
    // not a first-line preview, so the closeout guidance hands the agent the whole
    // steering intent to address in the current turn.
    let verbatim = steering.verbatim().unwrap_or_default();
    Ok(Some(format!("{label}: {verbatim}")))
}

pub fn realtime_steering_since_turn_baseline(
    file: &Path,
) -> Result<agent_doc_document_realtime::baseline_comparison::RealtimeSteering> {
    let current = crate::resolve_current_document_content(file, "unstarted_prompt_bearing")?;
    let head = agent_doc_git_io::revision::show_head(file)?;
    if let Some(head) = head.as_deref() {
        let steering_from_head =
            agent_doc_document_realtime::baseline_comparison::BaselineComparison::new(
                head, &current,
            )
            .realtime_steering();
        if steering_from_head.label().is_none() {
            return Ok(steering_from_head);
        }
    }

    // A fresh session can carry an unanswered exchange tail prompt before any
    // cycle snapshot exists. The queue path activates independently of the
    // turn baseline, so without a snapshot we fall back to the committed `HEAD`
    // blob and then to an empty baseline for untracked docs.
    let baseline = match agent_doc_snapshot_io::load(file)? {
        Some(snapshot) => snapshot,
        None => head.unwrap_or_default(),
    };
    Ok(
        agent_doc_document_realtime::baseline_comparison::BaselineComparison::new(
            &baseline, &current,
        )
        .realtime_steering(),
    )
}
