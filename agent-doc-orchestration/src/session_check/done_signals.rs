use super::*;

pub(crate) fn open_tracked_work_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    Ok(agent_doc_element_backlog::backlog::open_tracked_work_ids_in_content(&content))
}

pub fn inline_done_signal_ids(
    file: &Path,
    prompt_texts: &[String],
    auto_done: bool,
) -> Result<Vec<String>> {
    if prompt_texts.is_empty() {
        return Ok(Vec::new());
    }

    let open_ids = open_tracked_work_ids(file)?
        .into_iter()
        .collect::<std::collections::HashSet<_>>();
    if open_ids.is_empty() {
        return Ok(Vec::new());
    }

    let single_review_id = if auto_done {
        single_open_review_item_id(file)?
    } else {
        None
    };
    let mut ids = Vec::new();

    for prompt in prompt_texts {
        for id in agent_doc_turn::closeout_signal::explicit_done_signal_ids(prompt) {
            if open_ids.contains(&id) && !ids.iter().any(|existing| existing == &id) {
                ids.push(id);
            }
        }

        if auto_done
            && agent_doc_turn::closeout_signal::plain_done_signal(prompt)
            && let Some(id) = single_review_id.as_deref()
            && open_ids.contains(id)
            && !ids.iter().any(|existing| existing == id)
        {
            ids.push(id.to_string());
        }
    }

    Ok(ids)
}

/// Open (`[ ]`/gated, not done) ids that live specifically in the live
/// `agent:backlog` component. The `expect_done_or_gate` guard keys off backlog
/// membership: `--done`, `--pending-gate`, reap, and icebox moves all remove an
/// id from `agent:backlog`, so an id still present here was never given a
/// lifecycle outcome.
pub(crate) fn open_backlog_ids(file: &Path) -> Result<Vec<String>> {
    let content = std::fs::read_to_string(file)?;
    Ok(agent_doc_element_backlog::backlog::open_backlog_ids_in_content(&content))
}
