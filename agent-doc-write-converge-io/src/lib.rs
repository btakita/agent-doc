//! Write convergence sidecar adapters.
//!
//! This crate owns file-backed write-convergence decisions that sit between
//! pure realtime/write policy and durable sidecars. It keeps those decision
//! graphs out of the orchestration command crate.

use agent_doc_document_realtime::write_policy::{
    exchange_change_is_safe_historical_reduction, stale_snapshot_reset_drift,
};
use agent_doc_element::element::is_backlog_component;
use agent_doc_turn::response_replay::response_materialized_in_content;
use anyhow::Result;
use std::path::Path;

pub fn guard_no_stale_snapshot_reset_drift(
    file: &Path,
    snapshot_doc: Option<&str>,
    current_doc: &str,
    phase: &str,
) -> Result<bool> {
    let Some(snapshot_doc) = snapshot_doc else {
        return Ok(false);
    };
    if let Ok(Some(cleaned)) =
        agent_doc_template::deleted_conversation_tail_cleanup(snapshot_doc, current_doc)
        && cleaned == current_doc
    {
        return Ok(false);
    }
    let Some(drift) = stale_snapshot_reset_drift(snapshot_doc, current_doc) else {
        return Ok(false);
    };
    let snapshot_len = drift.snapshot_len;
    let current_len = drift.current_len;
    if active_capture_response_removed(file, snapshot_doc, current_doc) {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "stale_snapshot_rebase_skipped_active_capture file={} phase={} old_snap_len={} new_snap_len={}",
                file.display(),
                phase,
                snapshot_len,
                current_len
            ),
        );
        return Ok(false);
    }
    if let Some(reason) = classify_stale_snapshot_visible_rebase(file, snapshot_doc, current_doc) {
        agent_doc_snapshot_io::save(file, current_doc, agent_doc_ops_log_io::log_op)?;
        let crdt = agent_doc_merge::crdt::CrdtDoc::from_text(current_doc).encode_state();
        agent_doc_merge_io::save_document_crdt(file, &crdt, current_doc)?;
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "stale_snapshot_visible_rebased file={} phase={} reason={} old_snap_len={} new_snap_len={}",
                file.display(),
                phase,
                reason,
                snapshot_len,
                current_len
            ),
        );
        return Ok(true);
    }

    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "stale_snapshot_reset_drift_blocked file={} phase={} snap_len={} file_len={}",
            file.display(),
            phase,
            snapshot_len,
            current_len
        ),
    );
    anyhow::bail!(
        "refusing {phase} for {}: snapshot is {} bytes but the visible file is {} bytes, which looks like a manual cleanup with stale snapshot/CRDT state. Reset the sidecars from the current file before writing: `agent-doc reset --from-current {}`",
        file.display(),
        snapshot_len,
        current_len,
        file.display()
    );
}

fn classify_stale_snapshot_visible_rebase(
    file: &Path,
    snapshot_doc: &str,
    current_doc: &str,
) -> Option<&'static str> {
    let scope = agent_doc_turn_scope_io::load(file);
    let recent_binary_compaction =
        agent_doc_session_accretion_io::recent_exchange_compaction_timestamp(file)
            .ok()
            .flatten()
            .is_some();
    if active_capture_response_removed(file, snapshot_doc, current_doc) {
        return None;
    }

    let (snapshot_frontmatter, snapshot_body) =
        agent_doc_frontmatter::frontmatter::parse(snapshot_doc).ok()?;
    let (current_frontmatter, current_body) =
        agent_doc_frontmatter::frontmatter::parse(current_doc).ok()?;
    if !agent_doc_frontmatter::frontmatter::frontmatter_agent_only_equivalent(
        &snapshot_frontmatter,
        &current_frontmatter,
    ) {
        return None;
    }

    let snap_components = agent_doc_element::element::parse(snapshot_body).ok()?;
    let current_components = agent_doc_element::element::parse(current_body).ok()?;
    if snap_components.is_empty() || snap_components.len() != current_components.len() {
        return None;
    }

    let mut saw_exchange_trim = false;
    let mut saw_independent_component = false;
    for (snap_comp, current_comp) in snap_components.iter().zip(current_components.iter()) {
        if snap_comp.name != current_comp.name {
            return None;
        }
        if !is_backlog_component(&snap_comp.name)
            && snap_comp.patch_mode() != current_comp.patch_mode()
        {
            return None;
        }

        let snap_content =
            agent_doc_document::commit_normalization::normalize_component_content_for_absorb(
                snap_comp.content(snapshot_body),
            );
        let current_content =
            agent_doc_document::commit_normalization::normalize_component_content_for_absorb(
                current_comp.content(current_body),
            );
        if snap_content == current_content {
            continue;
        }

        if snap_comp.name == "exchange" {
            if exchange_change_is_safe_historical_reduction(
                snap_comp.content(snapshot_body),
                current_comp.content(current_body),
            ) {
                saw_exchange_trim = true;
                continue;
            }
            return None;
        }

        match scope.as_ref() {
            Some(scope)
                if component_change_is_turn_independent(
                    snapshot_body,
                    current_body,
                    &snap_comp.name,
                    scope,
                ) =>
            {
                saw_independent_component = true;
                continue;
            }
            _ => return None,
        }
    }

    match (saw_exchange_trim, saw_independent_component) {
        (true, true) => Some("historical_exchange_trim_unrelated_drift"),
        (true, false) => {
            if scope.is_some() || recent_binary_compaction {
                Some("historical_exchange_trim")
            } else {
                None
            }
        }
        (false, true) => Some("unrelated_component_drift"),
        (false, false) => None,
    }
}

fn active_capture_response_removed(file: &Path, snapshot_doc: &str, current_doc: &str) -> bool {
    let Ok(Some(state)) = agent_doc_cycle_state_io::load(file) else {
        return false;
    };
    if !state.is_open() {
        return false;
    }
    let Ok(Some(capture)) = agent_doc_capture_io::load_active(file) else {
        return false;
    };
    !capture.response_body.trim().is_empty()
        && response_materialized_in_content(&capture.response_body, snapshot_doc)
        && !response_materialized_in_content(&capture.response_body, current_doc)
}

fn component_change_is_turn_independent(
    snap_body: &str,
    current_body: &str,
    component_name: &str,
    scope: &agent_doc_turn::turn_scope::TurnScope,
) -> bool {
    use agent_doc_turn::op_log::OpActor;
    use agent_doc_turn::turn_scope::{Address, classify_op};

    let events: Vec<_> = agent_doc_markdown_ast::events::diff_node_events(snap_body, current_body)
        .into_iter()
        .filter(|event| event.component == component_name)
        .collect();
    if events.is_empty() {
        return false;
    }

    events.iter().all(|event| {
        let address = Address::from_component_node_key(&event.component, &event.node_key);
        let node_index = event.after_index.or(event.before_index);
        !classify_op(
            OpActor::User,
            event.kind.as_str(),
            &address,
            node_index,
            scope,
        )
        .affects_turn()
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    #[test]
    fn stale_snapshot_reset_drift_blocks_large_snapshot_only_content() {
        let dir = TempDir::new().unwrap();
        let doc = dir.path().join("test.md");
        let stale_exchange = "duplicated response\n".repeat(20);
        let snapshot = format!(
            "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\n{}<!-- /agent:exchange -->\n",
            stale_exchange
        );
        let current = "---\nagent_doc_session: test\n---\n\n<!-- agent:exchange patch=append -->\nclean\n<!-- /agent:exchange -->\n";

        let result =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "stream write");

        let message = result
            .expect_err("stale larger snapshot must fail closed")
            .to_string();
        assert!(
            message.contains("agent-doc reset --from-current"),
            "recovery guidance should name deterministic sidecar reset: {message}"
        );
    }

    #[test]
    fn stale_snapshot_reset_drift_rebases_compact_summary_after_clear_via_binary_origin_marker() {
        let dir = TempDir::new().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("test.md");
        fs::write(&doc, "seed").unwrap();
        let old_blocks = (0..12)
            .map(|idx| {
                format!(
                    "### Re: archived {idx} - gpt-5\n\n{}\n",
                    "Archived response body.\n".repeat(12)
                )
            })
            .collect::<String>();
        let snapshot = format!(
            "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n{old_blocks}<!-- agent:boundary:old -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n"
        );
        let current = "---\nagent_doc_session: test\nagent: codex\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n### Session Summary\n\n*Compacted. Content archived to `.agent-doc/archives/session.md`*\n\nCompacted content:\n- Archived 12 response topic(s): archived 0; archived 1; archived 2; 9 more\n- Prior summary/context: compacted prior responses\n<!-- agent:boundary:new -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue -->\n<!-- /agent:queue -->\n";
        fs::write(&doc, current).unwrap();
        agent_doc_snapshot_io::save(&doc, &snapshot, agent_doc_ops_log_io::log_op).unwrap();
        agent_doc_session_accretion_io::record_recent_exchange_compaction(&doc).unwrap();

        let rebased =
            guard_no_stale_snapshot_reset_drift(&doc, Some(&snapshot), current, "preflight")
                .expect("binary-origin compaction marker should rebase the stale snapshot");

        assert!(rebased, "guard should report a snapshot refresh");
        assert_eq!(
            agent_doc_snapshot_io::load(&doc).unwrap(),
            Some(current.to_string())
        );
    }
}
