use anyhow::Result;
use std::path::Path;

pub fn detect_unstarted_prompt_bearing_diff(file: &Path) -> Result<Option<String>> {
    let steering = realtime_steering_since_turn_baseline(file)?;
    let Some(label) = steering.label() else {
        return Ok(None);
    };
    // `#realtime-steering-verbatim` + `#realtime-steering-aggregate`: surface EVERY
    // operator prompt added mid-turn in FULL (not a first-line preview, and not just
    // the first of several). The operator can add multiple concurrent steering
    // directives while the turn is active; all of them must reach the agent at once
    // so it can address them together and find patterns across them, rather than
    // draining one head at a time.
    let set = realtime_steering_set_since_turn_baseline(file)?;
    let verbatim = set
        .verbatim_aggregate()
        .or_else(|| steering.verbatim().map(str::to_string))
        .unwrap_or_default();
    Ok(Some(format!("{label}: {verbatim}")))
}

pub fn realtime_steering_set_since_turn_baseline(
    file: &Path,
) -> Result<agent_doc_document_realtime::baseline_comparison::RealtimeSteeringSet> {
    let current = crate::resolve_current_document_content(file, "unstarted_prompt_bearing")?;
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

    let baseline = match agent_doc_snapshot_io::load(file)? {
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
