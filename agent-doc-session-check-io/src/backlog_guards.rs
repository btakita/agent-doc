use std::path::Path;

use agent_doc_element_backlog::guard_policy::{
    dropped_from_history_report, malformed_tracked_item_guard, shadow_backlog_guard,
};
use agent_doc_run_context_io::{AgentDocContextExt, CycleContext};
use agent_doc_workflow::session_check::GuardResult;
use anyhow::Result;

/// Where a reaped `do #id` directive's `### Re: ... #id` response heading
pub fn check_shadow_backlog_guard(_file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    Ok(shadow_backlog_guard(&rc.doc_content())?.into())
}

pub fn check_malformed_tracked_item_guard(_file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached content + parsed components.
    Ok(malformed_tracked_item_guard(&rc.doc_content(), &rc.components()).into())
}

pub fn check_backlog_replay_guard(file: &Path, rc: &CycleContext) -> Result<GuardResult> {
    // Phase 6 (#lr-content-6): cached document content.
    let current_content = rc.doc_content();

    let baseline_content = agent_doc_snapshot_io::load_document_baseline(file)?;

    let baseline = match baseline_content {
        Some(content) => content,
        None => match rc.head_content() {
            Some(content) => content.to_string(),
            None => return Ok(GuardResult::None),
        },
    };

    let resolved_ids = agent_doc_cycle_state_io::resolved_pending_ids(file)?;

    let mut external_current_ids =
        agent_doc_element_backlog_io::done_archive::external_done_archive_ids(
            file,
            &current_content,
        )?;
    let initial_report = dropped_from_history_report(
        &current_content,
        &baseline,
        &resolved_ids,
        &external_current_ids,
    )?;
    if !initial_report.dropped.is_empty() {
        let candidates = initial_report
            .dropped
            .into_iter()
            .map(|item| item.id)
            .collect();
        let transfer_evidence = agent_doc_element_backlog_io::cross_document::transferred_open_ids(
            file,
            &current_content,
            &candidates,
        )?;
        external_current_ids.extend(transfer_evidence.ids());
    }
    let report = dropped_from_history_report(
        &current_content,
        &baseline,
        &resolved_ids,
        &external_current_ids,
    )?;
    let head_content = rc.head_content();
    let evidence = agent_doc_element_backlog_io::deletion_authority::classify_operator_deletion(
        file,
        &baseline,
        &current_content,
        head_content.as_deref().map(String::as_str),
        &report,
    )?;
    if !report.dropped.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "backlog_deletion_authority file={} ids={} evidence={}",
                file.display(),
                report
                    .dropped
                    .iter()
                    .map(|item| item.id.as_str())
                    .collect::<Vec<_>>()
                    .join(","),
                evidence.as_str(),
            ),
        );
    }
    Ok(
        agent_doc_element_backlog_io::deletion_authority::operator_deletion_outcome(
            &report, evidence,
        )
        .into(),
    )
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use agent_doc_run_context_io::AgentDocContextExt;

    use super::*;

    #[test]
    fn replay_guard_accepts_idle_operator_cut_but_rejects_open_cycle_loss() {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let file = dir.path().join("session.md");
        let baseline = concat!(
            "<!-- agent:backlog -->\n",
            "- [ ] [#keep1] Keep me\n",
            "- [ ] [#remove1] Operator may remove me\n",
            "<!-- /agent:backlog -->\n",
        );
        let operator_cut = baseline.replace("- [ ] [#remove1] Operator may remove me\n", "");

        for args in [
            &["init"][..],
            &["config", "user.email", "test@example.com"],
            &["config", "user.name", "Test"],
        ] {
            assert!(
                Command::new("git")
                    .current_dir(dir.path())
                    .args(args)
                    .status()
                    .unwrap()
                    .success()
            );
        }
        std::fs::write(&file, baseline).unwrap();
        assert!(
            Command::new("git")
                .current_dir(dir.path())
                .args(["add", "session.md"])
                .status()
                .unwrap()
                .success()
        );
        assert!(
            Command::new("git")
                .current_dir(dir.path())
                .args(["commit", "-m", "baseline", "--no-verify"])
                .status()
                .unwrap()
                .success()
        );
        agent_doc_snapshot_io::checkpoint_document_baseline(
            &file,
            baseline,
            agent_doc_ops_log_io::log_op,
        )
        .unwrap();
        std::fs::write(&file, &operator_cut).unwrap();

        let idle_context = agent_doc_run_context_io::cycle_context(file.clone());
        idle_context.set_doc_content(operator_cut.clone());
        assert_eq!(
            check_backlog_replay_guard(&file, &idle_context).unwrap(),
            GuardResult::None
        );

        agent_doc_cycle_state_io::start_preflight(&file, Some(baseline), Some(&operator_cut))
            .unwrap();
        let open_context = agent_doc_run_context_io::cycle_context(file.clone());
        open_context.set_doc_content(operator_cut);
        assert!(matches!(
            check_backlog_replay_guard(&file, &open_context).unwrap(),
            GuardResult::Error(message) if message.contains("#remove1")
        ));
    }
}
