//! # Module: undo
//!
//! ## Spec
//! - `run(file)` removes the last agent response from the document and leaves
//!   every operator revision in place (`#undokeepsoperator`).
//! - Bails with an error if the file does not exist.
//! - The response is the newest captured response for the document (the active
//!   closeout capture, retired or not, else the latest committed capture). It is
//!   located in the current document by the same normalized match that proves
//!   materialization, and only those lines are removed.
//! - When the response is not present unchanged (never written, already undone,
//!   or edited by the operator), nothing is written and the reason is printed.
//!
//! ## Agentic Contracts
//! - `run()` is the sole public entry point.
//! - Undo never restores a whole-document snapshot: that dropped operator text
//!   typed after the checkpoint (a prompt, or a paste made while the response
//!   was being written). Lines outside the response are returned byte-for-byte.
//! - The write is compare-and-swap against the content undo read, through the
//!   editor authority, so a concurrent operator edit fails the undo rather than
//!   being overwritten.
//! - Undo is idempotent: once the response is gone a second call is a no-op.
//! - `dry_run` prints the lines that would be removed and writes nothing.
//!
//! ## Evals
//! - `agent_doc_turn::response_replay::undo_keeps_operator_text` covers the
//!   removal, the refusal on an operator edit, and newest-occurrence selection.
use anyhow::{Context, Result};
use std::path::Path;

pub fn run(file: &Path, dry_run: bool) -> Result<()> {
    if !file.exists() {
        anyhow::bail!("file not found: {}", file.display());
    }
    let Some(last) = last_agent_response(file)? else {
        eprintln!(
            "[undo] Nothing to undo — no captured agent response for {}",
            file.display()
        );
        return Ok(());
    };
    let response = last.response_body.clone();
    let current = agent_doc_document_realtime_io::try_resolve_current_document_content(
        file,
        "undo_command_document",
    )?;
    let Some(undone) =
        agent_doc_turn::response_replay::remove_materialized_response(&current, &response)
    else {
        eprintln!(
            "[undo] Nothing removed from {}: the last agent response is not present unchanged (already undone, or edited by the operator). Operator text is authoritative, so undo does not guess.",
            file.display()
        );
        if !dry_run {
            retire_undone_capture(file, &last)?;
        }
        return Ok(());
    };
    if dry_run {
        let removed: Vec<&str> = current.split_inclusive('\n').collect();
        let kept: Vec<&str> = undone.split_inclusive('\n').collect();
        let prefix = removed.iter().zip(&kept).take_while(|(a, b)| a == b).count();
        let suffix = removed[prefix..]
            .iter()
            .rev()
            .zip(kept[prefix..].iter().rev())
            .take_while(|(a, b)| a == b)
            .count();
        eprintln!(
            "[undo] dry run for {}: would remove lines {}..={} (nothing written)",
            file.display(),
            prefix + 1,
            removed.len() - suffix,
        );
        for line in &removed[prefix..removed.len() - suffix] {
            print!("- {line}");
        }
        return Ok(());
    }
    match agent_doc_document_realtime_io::atomic_write_if_current_through_authority(
        file,
        &undone,
        &current,
        "undo_remove_last_response",
    ) {
        Ok(()) => {}
        // The editor accepted the undo and owns the native save; wait for that
        // exact version on disk rather than report an accepted write as failed.
        Err(error)
            if error
                .downcast_ref::<agent_doc_document_realtime_io::AwaitEditorReplicaNoDiskWrite>()
                .is_some() =>
        {
            await_undo_on_disk(file, &undone).with_context(|| format!("{error:#}"))?;
        }
        Err(error) => return Err(error),
    }
    // `#undokeepsoperator`: with the response gone the operator's prompt reads
    // as that capture's unanswered tail; an unretired capture would be replayed
    // straight back by the next preflight.
    retire_undone_capture(file, &last)?;
    agent_doc_snapshot_io::checkpoint_document_baseline(
        file,
        &undone,
        agent_doc_ops_log_io::log_op,
    )?;
    agent_doc_snapshot_io::clear_undo_content(file)?;
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "undo_removed_last_response file={} response_sha256={} prior_hash={} undone_hash={} operator_text=preserved",
            file.display(),
            agent_doc_hash::content_hash(&response),
            agent_doc_hash::content_hash(&current),
            agent_doc_hash::content_hash(&undone),
        ),
    );
    eprintln!(
        "[undo] Removed the last agent response from {}; operator text is unchanged",
        file.display()
    );
    Ok(())
}

struct LastResponse {
    cycle_id: String,
    capture_id: String,
    response_body: String,
}

/// The newest captured response: the closeout projection's capture (kept as
/// evidence even after retirement), else the latest committed capture.
fn last_agent_response(file: &Path) -> Result<Option<LastResponse>> {
    if let Some(capture) = agent_doc_cycle_state_io::load_closeout_projection(file)?
        .and_then(|projection| projection.captured_response)
        .filter(|capture| !capture.response_body.trim().is_empty())
    {
        return Ok(Some(LastResponse {
            cycle_id: capture.cycle_id,
            capture_id: capture.capture_id,
            response_body: capture.response_body,
        }));
    }
    Ok(agent_doc_capture_io::latest_committed(file)?
        .filter(|capture| !capture.response_body.trim().is_empty())
        .map(|capture| LastResponse {
            cycle_id: capture.cycle_id,
            capture_id: capture.capture_id,
            response_body: capture.response_body,
        }))
}

fn retire_undone_capture(file: &Path, last: &LastResponse) -> Result<()> {
    if agent_doc_cycle_state_io::retire_projected_captured_response(
        file,
        &last.cycle_id,
        &last.capture_id,
        "operator_undo",
    )? {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "undo_retired_capture file={} cycle_id={} capture_id={}",
                file.display(),
                last.cycle_id,
                last.capture_id,
            ),
        );
    }
    Ok(())
}

/// Bounded wait for the editor's native save of the undone text. Driven by the
/// controller's delivery-convergence edge; the deadline only bounds this CLI.
fn await_undo_on_disk(file: &Path, undone: &str) -> Result<()> {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(15);
    loop {
        if agent_doc_document_realtime_io::resolve_disk_current_document_content(
            file,
            "undo_await_native_save",
        )? == undone
        {
            return Ok(());
        }
        anyhow::ensure!(
            std::time::Instant::now() < deadline,
            "the editor holds the undone document but has not saved it to disk yet; it stays retained and saves on its own"
        );
        if let Err(error) = agent_doc_controller_io::project_controller::await_delivery_convergence_for_file(
            file,
            std::time::Duration::from_millis(500),
        ) {
            eprintln!("[undo] delivery convergence wait failed: {error:#}");
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    /// `#undokeepsoperator`: the 2026-09-28 agent-doc-bugs.md recovery. The
    /// operator's prompt and a paste typed while the response was being written
    /// survive; only the response goes, and its capture is retired so preflight
    /// cannot replay it back under the now-unanswered prompt.
    #[test]
    fn undo_removes_only_the_response_and_retires_its_capture() {
        let dir = TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
        let doc = dir.path().join("session.md");
        let base = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n<!-- agent:exchange patch=append -->\nFix api.md issue\n<!-- /agent:exchange -->\n";
        std::fs::write(&doc, base).unwrap();
        agent_doc_snapshot_io::checkpoint_document_baseline(&doc, base, agent_doc_ops_log_io::log_op)
            .unwrap();
        agent_doc_cycle_state_io::start_preflight(&doc, Some(base), Some(base)).unwrap();
        let response = "### Re: infra.md owner — opus-5-5\n\nReplayed body.\n";
        let capture = agent_doc_capture_io::capture_response(&doc, response).unwrap();
        let with_response = base.replace(
            "Fix api.md issue\n",
            &format!("Fix api.md issue\n```\npasted while it wrote\n```\n\n{response}"),
        ) + "operator note after\n";
        std::fs::write(&doc, &with_response).unwrap();

        run(&doc, false).unwrap();

        let undone = std::fs::read_to_string(&doc).unwrap();
        assert_eq!(
            undone,
            base.replace(
                "Fix api.md issue\n",
                "Fix api.md issue\n```\npasted while it wrote\n```\n",
            ) + "operator note after\n"
        );
        let projection = agent_doc_cycle_state_io::load_closeout_projection(&doc)
            .unwrap()
            .unwrap();
        assert_eq!(
            projection.captured_response_retired_reason.as_deref(),
            Some("operator_undo"),
            "capture {} must be retired so it is never replayed",
            capture.capture_id
        );

        // A second undo finds nothing and writes nothing.
        run(&doc, false).unwrap();
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), undone);
    }
}
