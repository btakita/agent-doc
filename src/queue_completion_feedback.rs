//! Binary-owned completion feedback for background-subagent queue claims.
//!
//! Workers never write the shared session document. The coordinator reports a
//! worker result through `queue report-complete`. The binary appends one inert
//! status artifact to `agent:exchange` through the editor/CRDT authority.
//! Reporting keeps the claim active while integration and verification run;
//! ordinary claim release never implies that a worker completed successfully.

use std::path::{Path, PathBuf};
use std::time::Duration;

use agent_doc_element::component_scope::{changed_component_scope, with_component_write_scope};
use agent_doc_queue::queue_claim::QueueClaim;
use anyhow::{Context, Result, bail};

const SOURCE: &str = "queue_subagent_completion_feedback";
const COMPLETION_LOCK_WAIT: Duration = Duration::from_secs(5);

#[derive(Debug)]
pub(crate) struct CompletionFeedbackOutcome {
    pub claim: QueueClaim,
    pub appended: bool,
}

pub(crate) fn report_complete(
    file: &Path,
    item: &str,
) -> Result<Option<CompletionFeedbackOutcome>> {
    report_complete_with_lock_wait(file, item, COMPLETION_LOCK_WAIT)
}

fn report_complete_with_lock_wait(
    file: &Path,
    item: &str,
    lock_wait: Duration,
) -> Result<Option<CompletionFeedbackOutcome>> {
    // `#subagentcompletionfeedback`: unlike SESSION_ACTOR_WRITE_QUEUE, this
    // lock serializes independent CLI processes. Re-resolve and rebuild inside
    // the owner so stale full-document successors cannot lose sibling notices.
    let lock_path = completion_lock_path(file)?;
    let lock = agent_doc_sync_io::acquire_sync_lock(&lock_path, lock_wait, |_| {});
    let agent_doc_sync_io::SyncLockAcquire::Acquired(_lock) = lock else {
        bail!(
            "timed out after {}ms waiting to publish subagent completion feedback for {}; no document mutation was attempted",
            lock_wait.as_millis(),
            file.display(),
        );
    };
    let current =
        agent_doc_document_realtime_io::try_resolve_current_document_content(file, SOURCE)?;
    let Some(claim) = observed_claim(file, item, &current)? else {
        return Ok(None);
    };
    if !claim.owner.starts_with("subagent:") {
        return Ok(None);
    }
    let appended = append_completion_feedback(file, &current, &claim)?;
    Ok(Some(CompletionFeedbackOutcome { claim, appended }))
}

fn observed_claim(file: &Path, item: &str, current: &str) -> Result<Option<QueueClaim>> {
    let mut ledger = agent_doc_queue_io::queue_claim::load_ledger_following(file, current)?;
    // Use the same pure identity/continuation matching as release without
    // mutating the durable ledger.
    Ok(ledger.release(item))
}

fn append_completion_feedback(file: &Path, current: &str, claim: &QueueClaim) -> Result<bool> {
    let identity = serde_json::to_string(&claim.identity).context("serialize claim identity")?;
    let key_material = format!("{identity}\n{}\n{}", claim.owner, claim.claimed_at_secs);
    let key = agent_doc_hash::content_hash(&key_material);
    let marker = format!("<!-- agent-doc-subagent-completion key={key} -->");
    let item = one_line(&claim.item_text).replace(['“', '”'], "'");
    let owner = claim.owner.replace('`', "'");
    let note = format!(
        "### Re: Subagent completed — agent-doc: `{owner}` returned work for queue item “{item}”; coordinator integration and closeout are pending.\n{marker}"
    );
    let pending_target = agent_doc_document_realtime_io::pending_document_write(file)
        .map(|pending| pending.target_content);
    if current.contains(&marker)
        || pending_target
            .as_deref()
            .is_some_and(|target| target.contains(&marker))
    {
        return Ok(false);
    }
    let updated = agent_doc_element_exchange::insert_prompt_line_before_boundary(current, &note)?;
    let scope = changed_component_scope(current, &updated)
        .context("subagent completion feedback must change only agent:exchange")?;
    if !scope.contains("exchange", 0) || scope.components().len() != 1 {
        bail!("subagent completion feedback escaped the first agent:exchange component");
    }
    // An attached editor owns disk projection. Hand the target to the durable
    // retained-intent graph and return as soon as the controller accepts it;
    // its reactive projector applies the target without keeping this one-shot
    // CLI process alive through typing/native-save backpressure. Detached files
    // still use the ordinary synchronous CAS write.
    let retained_intent = if agent_doc_crdt_relay_io::crdt_authority_for_file(file)
        .editor_attached()
    {
        Some(with_component_write_scope(scope, || {
            agent_doc_document_realtime_io::retain_deferred_document_write_target(
                file,
                current,
                &updated,
                SOURCE,
                agent_doc_document_realtime_io::DocumentWriteDeferredReason::EditorProjectionPending,
            )
        })?)
    } else {
        with_component_write_scope(scope, || {
            agent_doc_document_realtime_io::atomic_write_rebased_through_authority(
                file, current, &updated, SOURCE,
            )
        })?;
        None
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "subagent_completion_feedback_accepted owner={} claimed_at={} key={} item_bytes={} delivery={}{}",
            claim.owner,
            claim.claimed_at_secs,
            key,
            claim.item_text.len(),
            if retained_intent.is_some() {
                "retained_controller_projection"
            } else {
                "detached_materialized"
            },
            retained_intent
                .as_deref()
                .map(|intent| format!(" intent_id={intent}"))
                .unwrap_or_default(),
        ),
    );
    Ok(true)
}

fn completion_lock_path(file: &Path) -> Result<PathBuf> {
    let root = agent_doc_project_root_io::project_root_containing(file)
        .with_context(|| format!("no project root found for {}", file.display()))?;
    let canonical = file.canonicalize().unwrap_or_else(|_| file.to_path_buf());
    let document_hash = agent_doc_hash::document_id_for_path(&canonical);
    Ok(root
        .join(".agent-doc/locks")
        .join(format!("subagent-completion-{document_hash}.lock")))
}

fn one_line(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use agent_doc_element_queue::QueueItemIdentity;
    use std::fs;

    fn test_claim() -> QueueClaim {
        QueueClaim {
            identity: QueueItemIdentity::FreeText("finish-sdk".into()),
            item_text: "finish sdk.md\nwith deterministic tests".into(),
            owner: "subagent:sdk-feedback".into(),
            claimed_at_secs: 10,
            expires_at_secs: 20,
            owner_session: None,
        }
    }

    #[test]
    fn completion_notice_is_deduped_and_not_prompt_bearing() {
        let before = concat!(
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
            "<!-- agent:queue subagents -->\n",
            "- finish sdk.md with deterministic tests\n",
            "<!-- /agent:queue -->\n",
        );
        let claim = test_claim();
        let identity = serde_json::to_string(&claim.identity).unwrap();
        let key = agent_doc_hash::content_hash(&format!(
            "{identity}\n{}\n{}",
            claim.owner, claim.claimed_at_secs
        ));
        let marker = format!("<!-- agent-doc-subagent-completion key={key} -->");
        let note = format!(
            "### Re: Subagent completed — agent-doc: `{}` returned work for queue item “{}”; coordinator integration and closeout are pending.\n{marker}",
            claim.owner,
            one_line(&claim.item_text)
        );
        let after =
            agent_doc_element_exchange::insert_prompt_line_before_boundary(before, &note).unwrap();

        assert_eq!(after.matches("### Re: Subagent completed").count(), 1);
        assert!(after.find(&note).unwrap() < after.find("<!-- agent:boundary:keep -->").unwrap());
        assert!(after.contains("- finish sdk.md with deterministic tests"));
        assert!(after.contains(&marker));
        let diff = agent_doc_diff::unified_diff_from_contents(before, &after).unwrap();
        let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff);
        assert!(
            changes.iter().all(|change| matches!(
                change.kind,
                agent_doc_diff::PromptBearingChangeKind::RecoveryArtifact
                    | agent_doc_diff::PromptBearingChangeKind::BoundaryArtifact
            )),
            "{changes:?}"
        );
        assert!(agent_doc_diff::prompt_prefix_normalization_targets(&diff).is_empty());
        let exchange_subagent_dispatch = agent_doc_preflight_io::exchange_subagent_dispatch_entries(
            &changes
                .iter()
                .filter(|change| {
                    change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                })
                .map(|change| change.text.clone())
                .collect::<Vec<_>>(),
        );
        assert!(
            exchange_subagent_dispatch.is_empty(),
            "an N-1 controller must not route the completion artifact as an exchange subagent prompt"
        );
        assert_eq!(
            agent_doc_document_realtime::write_policy::classify_safe_out_of_band_agent_doc_mutation(
                before, &after,
            ),
            Some("exchange"),
            "an open coordinator cycle must accept the binary completion artifact"
        );
    }

    #[test]
    fn completion_marker_does_not_hide_a_mixed_operator_prompt() {
        let before = concat!(
            "<!-- agent:exchange patch=append subagents -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let after = concat!(
            "<!-- agent:exchange patch=append subagents -->\n",
            "Please review this before integrating.\n",
            "<!-- agent-doc-subagent-completion key=not-a-binary-block -->\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n",
        );
        let diff = agent_doc_diff::unified_diff_from_contents(before, after).unwrap();
        let changes = agent_doc_diff::classify_prompt_bearing_changes(&diff);
        assert!(changes.iter().any(|change| {
            change.kind == agent_doc_diff::PromptBearingChangeKind::PromptTarget
                && change.text.contains("Please review this")
        }));
    }

    #[test]
    fn report_is_immediate_but_keeps_claim_until_release() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        fs::write(
            &doc,
            concat!(
                "---\nagent_doc_session: sdk-feedback\nagent_doc_format: template\n---\n\n",
                "<!-- agent:exchange patch=append -->\n",
                "### Re: prior — gpt-5\n\nDone.\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue subagents -->\n",
                "- finish sdk.md with deterministic tests\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(
            &doc,
            "finish sdk.md with deterministic tests",
            "subagent:sdk-feedback",
            600,
        )
        .unwrap();

        let first = report_complete(&doc, "finish sdk.md with deterministic tests")
            .unwrap()
            .unwrap();
        assert!(first.appended);
        assert_eq!(
            agent_doc_queue_io::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .len(),
            1,
            "completion feedback must not release the integration guard"
        );
        let refreshed = agent_doc_queue_io::queue_claim::refresh(
            &doc,
            "finish sdk.md with deterministic tests",
            "subagent:sdk-feedback",
            1_200,
        )
        .unwrap();
        assert_ne!(refreshed.expires_at_secs, first.claim.expires_at_secs);
        let second = report_complete(&doc, "finish sdk.md with deterministic tests")
            .unwrap()
            .unwrap();
        assert!(
            !second.appended,
            "the same completion must dedupe across TTL refresh"
        );

        let released = agent_doc_queue_io::queue_claim::release(
            &doc,
            "finish sdk.md with deterministic tests",
        )
        .unwrap()
        .unwrap();
        assert_eq!(released.owner, "subagent:sdk-feedback");
        assert!(
            agent_doc_queue_io::queue_claim::load_ledger(&doc)
                .unwrap()
                .claims
                .is_empty()
        );
        let content = fs::read_to_string(&doc).unwrap();
        assert_eq!(content.matches("### Re: Subagent completed").count(), 1);
        assert!(
            content.find("### Re: Subagent completed").unwrap()
                < content.find("<!-- agent:boundary:keep -->").unwrap()
        );
    }

    #[test]
    fn attached_editor_reports_are_durable_bounded_and_concurrency_safe() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        let items = ["finish sdk", "finish dev", "finish pane", "finish api"];
        let queue = items
            .iter()
            .map(|item| format!("- {item}\n"))
            .collect::<String>();
        fs::write(
            &doc,
            format!(
                concat!(
                    "---\nagent_doc_session: sdk-feedback-live\nagent_doc_format: template\n---\n\n",
                    "<!-- agent:exchange patch=append subagents -->\n",
                    "### Re: prior — gpt-5\n\nDone.\n",
                    "Operator edit that must survive.\n",
                    "<!-- agent:boundary:keep -->\n",
                    "<!-- /agent:exchange -->\n\n",
                    "<!-- agent:queue subagents -->\n",
                    "{}",
                    "<!-- /agent:queue -->\n",
                ),
                queue,
            ),
        )
        .unwrap();
        for item in items {
            agent_doc_queue_io::queue_claim::claim(
                &doc,
                item,
                &format!("subagent:{}", item.replace(' ', "-")),
                600,
            )
            .unwrap();
        }

        let canonical = doc.canonicalize().unwrap();
        let document_hash = agent_doc_hash::document_id_for_path(&canonical);
        agent_doc_reliable_sync_io::global_liveness_plane()
            .lock()
            .restore_liveness(&[agent_doc_reliable_sync_io::liveness::LivenessOp::Open {
                document_hash,
                pid: std::process::id().into(),
                tag: "completion-feedback-editor".to_string(),
            }]);
        agent_doc_crdt_relay_io::register_replica_for_file(&doc, "completion-feedback-editor")
            .unwrap();
        assert!(
            agent_doc_crdt_relay_io::crdt_authority_for_file(&doc).editor_attached(),
            "fixture must exercise the retained active-editor branch"
        );

        let started = std::time::Instant::now();
        let threads = items
            .into_iter()
            .map(|item| {
                let doc = doc.clone();
                std::thread::spawn(move || report_complete(&doc, item).unwrap().unwrap())
            })
            .collect::<Vec<_>>();
        let outcomes = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect::<Vec<_>>();
        assert!(outcomes.iter().all(|outcome| outcome.appended));
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "four reports must return on durable acceptance, not wait for editor projection"
        );

        let pending = agent_doc_document_realtime_io::pending_document_write(&doc)
            .expect("active-editor feedback must have one durable composed target");
        assert!(
            pending
                .target_content
                .contains("Operator edit that must survive.")
        );
        assert_eq!(
            pending
                .target_content
                .matches("### Re: Subagent completed")
                .count(),
            items.len(),
            "the retained successor must contain every concurrent completion exactly once"
        );
        assert_eq!(
            pending
                .target_content
                .matches("<!-- agent-doc-subagent-completion key=")
                .count(),
            items.len()
        );
        assert_eq!(
            fs::read_to_string(&doc)
                .unwrap()
                .matches("### Re: Subagent completed")
                .count(),
            0,
            "the one-shot command must not force disk while the editor owns projection"
        );
        agent_doc_crdt_relay_io::deregister_replica_for_file(&doc, "completion-feedback-editor")
            .unwrap();
    }

    #[test]
    fn contended_cross_process_owner_is_strictly_bounded() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("sdk.md");
        fs::write(
            &doc,
            concat!(
                "<!-- agent:exchange patch=append -->\n",
                "<!-- agent:boundary:keep -->\n",
                "<!-- /agent:exchange -->\n",
                "<!-- agent:queue subagents -->\n",
                "- finish sdk\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        agent_doc_queue_io::queue_claim::claim(&doc, "finish sdk", "subagent:sdk-feedback", 600)
            .unwrap();
        let lock_path = completion_lock_path(&doc).unwrap();
        let agent_doc_sync_io::SyncLockAcquire::Acquired(_held) =
            agent_doc_sync_io::acquire_sync_lock(&lock_path, Duration::ZERO, |_| {})
        else {
            panic!("fixture must acquire completion lock");
        };
        let started = std::time::Instant::now();
        let error = report_complete_with_lock_wait(&doc, "finish sdk", Duration::from_millis(50))
            .unwrap_err();
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.to_string().contains("timed out after 50ms"));
        assert!(
            !fs::read_to_string(&doc)
                .unwrap()
                .contains("Subagent completed")
        );
    }
}
