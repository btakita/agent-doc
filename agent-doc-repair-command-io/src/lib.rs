use agent_doc_turn::repair::RepairOutcome;
use anyhow::Result;
use std::path::Path;

pub fn run_write_command_with_empty_response_recovery(
    options: agent_doc_write_command_io::CommandOptions,
    commit_mode: agent_doc_write_command_io::CommitMode,
) -> Result<()> {
    agent_doc_write_runtime_io::run_command_with_empty_response_recovery(
        options,
        commit_mode,
        recover_empty_response_for_strict_closeout,
    )
}

pub fn recover_empty_response_for_strict_closeout(
    file: &Path,
    strict_closeout: bool,
    has_pending_mutation: bool,
    force_disk: bool,
) -> Result<bool> {
    agent_doc_repair_io::recover_empty_response_for_strict_closeout(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
        strict_closeout,
        has_pending_mutation,
        Some(force_disk),
    )
}

#[cfg(test)]
pub fn run(file: &Path) -> Result<RepairOutcome> {
    agent_doc_repair_io::run(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}

pub fn repair(file: &Path) -> Result<RepairOutcome> {
    agent_doc_repair_io::repair(
        agent_doc_repair_runtime_io::repair_coordinator_effects(
            &agent_doc_write_runtime_io::REPAIR_REPLAY_WRITE_EFFECTS,
        ),
        file,
    )
}
