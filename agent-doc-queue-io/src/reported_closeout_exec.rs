//! Finish a reported closeout the binary can run without a judgment.
//!
//! A `☑️` row is a worker that already called `queue report-complete`. The
//! coordinator still owes the response, the integration proof, and the strike.
//! This module runs that transition when
//! [`ReportedClaimCloseout::mechanical_terminal_steps`] says every step is a
//! concrete verify, an integrated-lineage record, or the closing consume, and
//! the worker's response is already in the document or can be rebuilt from
//! the receipt `report-complete` saved. It does not merge, cherry-pick, fetch,
//! or push, and it does not strike a row whose closeout still needs a
//! decision or whose response was never captured.
//!
//! A verify that fails, or a consume the strike path refuses, leaves the
//! attempt `Reported`.

use std::collections::HashSet;
use std::path::Path;
use std::process::{Command, Stdio};

use agent_doc_document::queue_projection::QueueOwnerProjection;
use agent_doc_queue::queue_claim::QueueClaim;
use agent_doc_queue::reported_closeout::{ReportedCloseoutAction, ReportedCloseoutStep};
use anyhow::{Context, Result, bail};

use crate::queue_cmd::QueueCommandEffects;
use crate::queue_consume::AckTextSelector;
use crate::queue_lineage::{IntegrationMethod, QueueLineageEventKind, RecordRequest};

/// One reported attempt after a mechanical-closeout pass.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MechanicalCloseoutOutcome {
    pub item: String,
    pub owner: String,
    pub generation: u64,
    pub result: MechanicalCloseoutResult,
}

/// What the pass did with one reported attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MechanicalCloseoutResult {
    /// The response is in the document and the row is no longer `☑️`.
    Closed,
    /// The visible marker does not match the durable claim, so consume would
    /// strike the wrong row. Repair the projection, then run the pass again.
    ProjectionOutOfSync,
    /// The closeout is not mechanical, the response is missing, or a step
    /// was refused.
    LeftReported { reason: String },
}

impl MechanicalCloseoutOutcome {
    pub fn closed(&self) -> bool {
        matches!(self.result, MechanicalCloseoutResult::Closed)
    }

    pub fn projection_out_of_sync(&self) -> bool {
        matches!(self.result, MechanicalCloseoutResult::ProjectionOutOfSync)
    }
}

/// One pass over the live reported attempts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MechanicalCloseoutPass {
    pub outcomes: Vec<MechanicalCloseoutOutcome>,
    /// True when this pass wrote the document (a restored response cell).
    pub document_changed: bool,
}

/// Close every reported attempt whose closeout is mechanical, whose visible
/// marker matches the claim, and whose worker response is in the document.
///
/// A saved response whose completion cell is missing is written back first,
/// from the receipt `report-complete` captured. Out-of-sync rows are reported
/// as [`MechanicalCloseoutResult::ProjectionOutOfSync`] and are not consumed.
/// One item's refusal does not stop the others.
pub fn close_mechanical_reported_attempts(
    effects: &impl QueueCommandEffects,
    file: &Path,
) -> Result<MechanicalCloseoutPass> {
    let mut content = effects.current_document_content(file, "mechanical_reported_closeout")?;
    let mut document_changed = false;
    if let Some(restored) = restore_missing_response_cells(effects, file, &content)? {
        content = restored;
        document_changed = true;
    }
    let pairs = crate::queue_claim::reported_claim_closeouts_for_content(file, &content)?;
    let observed = crate::queue_claim::visible_lifecycle_markers_for_content(&content)?;
    let receipts = crate::claim_release_receipt::load(file)?.receipts;
    let mut outcomes = Vec::with_capacity(pairs.len());
    for (claim, closeout) in pairs {
        let projection = crate::queue_claim::visible_claim_projection(&observed, &claim);
        if !projection.synchronized {
            outcomes.push(outcome(
                &claim,
                MechanicalCloseoutResult::ProjectionOutOfSync,
            ));
            continue;
        }
        let Some(steps) = closeout.mechanical_terminal_steps() else {
            outcomes.push(outcome(
                &claim,
                MechanicalCloseoutResult::LeftReported {
                    reason: format!(
                        "{} closeout still needs a coordinator; it was not struck",
                        closeout.shape.as_str()
                    ),
                },
            ));
            continue;
        };
        if !crate::sync_payload::reported_result_recorded_in_document(&content, &claim, &receipts) {
            outcomes.push(outcome(
                &claim,
                MechanicalCloseoutResult::LeftReported {
                    reason: "the worker response is not in the document, so the row was not struck"
                        .to_string(),
                },
            ));
            continue;
        }
        match run_steps(effects, file, &claim, steps) {
            Ok(()) => outcomes.push(outcome(&claim, MechanicalCloseoutResult::Closed)),
            Err(err) => outcomes.push(outcome(
                &claim,
                MechanicalCloseoutResult::LeftReported {
                    reason: one_line(&format!("{err:#}")),
                },
            )),
        }
    }
    Ok(MechanicalCloseoutPass {
        outcomes,
        document_changed,
    })
}

fn restore_missing_response_cells(
    effects: &impl QueueCommandEffects,
    file: &Path,
    content: &str,
) -> Result<Option<String>> {
    let report = crate::completion_cell_restore::inspect(file, content, &HashSet::new())?;
    if report.restored.is_empty() {
        return Ok(None);
    }
    let updated = crate::completion_cell_restore::compose(content, &report)?;
    if updated == content {
        return Ok(None);
    }
    effects.converge_document_or_disk(
        file,
        &updated,
        content,
        "mechanical_reported_closeout_write_response",
    )?;
    Ok(Some(effects.current_document_content(
        file,
        "mechanical_reported_closeout",
    )?))
}

fn outcome(claim: &QueueClaim, result: MechanicalCloseoutResult) -> MechanicalCloseoutOutcome {
    MechanicalCloseoutOutcome {
        item: claim.item_text.clone(),
        owner: claim.owner.clone(),
        generation: claim.generation,
        result,
    }
}

fn one_line(text: &str) -> String {
    text.replace('\n', " ")
}

fn run_steps(
    effects: &impl QueueCommandEffects,
    file: &Path,
    claim: &QueueClaim,
    steps: &[ReportedCloseoutStep],
) -> Result<()> {
    for step in steps {
        match step.action {
            ReportedCloseoutAction::VerifyIntegration => run_verify(&step.argv)?,
            ReportedCloseoutAction::RecordIntegration => run_record(file, claim, &step.argv)?,
            ReportedCloseoutAction::Consume => run_consume(effects, file, &step.argv)?,
            other => bail!("mechanical closeout refused a {} step", other.as_str()),
        }
    }
    Ok(())
}

fn flag<'a>(argv: &'a [String], name: &str) -> Option<&'a str> {
    argv.windows(2)
        .find(|pair| pair[0] == name)
        .map(|pair| pair[1].as_str())
}

/// `git -C <sink> merge-base --is-ancestor <source> <sink-ref>`.
fn run_verify(argv: &[String]) -> Result<()> {
    if argv.len() != 7
        || argv[0] != "git"
        || argv[1] != "-C"
        || argv[3] != "merge-base"
        || argv[4] != "--is-ancestor"
    {
        bail!("verify step is not `git -C <repo> merge-base --is-ancestor <source> <sink-ref>`");
    }
    let status = Command::new("git")
        .args(&argv[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .with_context(|| format!("failed to run {}", argv.join(" ")))?;
    if status.success() {
        return Ok(());
    }
    bail!(
        "designated sink ref {} does not contain source commit {}",
        argv[6],
        argv[5]
    )
}

fn run_record(file: &Path, claim: &QueueClaim, argv: &[String]) -> Result<()> {
    if flag(argv, "--event") != Some("integrated") {
        bail!("record step is not an integrated lineage receipt");
    }
    let generation = flag(argv, "--generation")
        .context("record step missing --generation")?
        .parse::<u64>()
        .context("record step generation is not an integer")?;
    if generation != claim.generation {
        bail!(
            "record step generation {generation} is not attempt {}",
            claim.generation
        );
    }
    let method =
        IntegrationMethod::parse(flag(argv, "--method").context("record step missing --method")?)?;
    let path = |name: &str| -> Result<std::path::PathBuf> {
        Ok(
            Path::new(flag(argv, name).with_context(|| format!("record step missing {name}"))?)
                .to_path_buf(),
        )
    };
    let required = |name: &str| -> Result<String> {
        Ok(flag(argv, name)
            .with_context(|| format!("record step missing {name}"))?
            .to_string())
    };
    crate::queue_lineage::record(
        file,
        &claim.item_text,
        claim.generation,
        RecordRequest {
            kind: QueueLineageEventKind::Integrated,
            worker_id: claim.worker.as_ref().map(|worker| worker.worker_id.clone()),
            source_repo_path: Some(path("--source-repo")?),
            sink_repo_path: Some(path("--sink-repo")?),
            source_sha: Some(required("--source-sha")?),
            integration_sha: Some(required("--integration-sha")?),
            integration_method: Some(method),
            sink_ref: Some(required("--sink-ref")?),
            resolves_sha: flag(argv, "--resolves").map(str::to_string),
            detail: Some("integration recorded by the mechanical reported closeout".to_string()),
            ..RecordRequest::default()
        },
    )?;
    Ok(())
}

fn parse_ack_owner(raw: &str) -> Result<QueueOwnerProjection> {
    let raw = raw.trim();
    let (owner, generation) = raw
        .rsplit_once('#')
        .with_context(|| format!("invalid --ack-owner {raw:?}; expected OWNER#GENERATION"))?;
    if owner.is_empty() {
        bail!("invalid --ack-owner {raw:?}; OWNER must not be empty");
    }
    let generation = generation
        .parse::<u64>()
        .with_context(|| format!("invalid --ack-owner {raw:?}; GENERATION must be an integer"))?;
    if generation == 0 {
        bail!("invalid --ack-owner {raw:?}; GENERATION must be greater than zero");
    }
    Ok(QueueOwnerProjection {
        owner: owner.to_string(),
        generation,
    })
}

fn run_consume(effects: &impl QueueCommandEffects, file: &Path, argv: &[String]) -> Result<()> {
    let guard = crate::queue_edit_owner::QueueEditGuard::acquire(file)?;
    if let Some(id) = flag(argv, "--ack-id") {
        let attempt = flag(argv, "--ack-owner").map(parse_ack_owner).transpose()?;
        let struck = effects.acknowledge_open_id_backed_queue_head_for_attempt(
            file,
            id,
            attempt.as_ref(),
            &guard,
        )?;
        if !struck {
            bail!("consume did not strike the reported row");
        }
        return Ok(());
    }
    if let Some(text) = flag(argv, "--ack-text") {
        let selector = flag(argv, "--ack-owner")
            .map(parse_ack_owner)
            .transpose()?
            .map(AckTextSelector::Owner);
        let struck = effects
            .acknowledge_paused_free_text_queue_head_with_selector(
                file,
                text,
                selector.as_ref(),
                &guard,
            )?
            .is_some();
        if !struck {
            bail!("consume did not strike the reported row");
        }
        return Ok(());
    }
    bail!("consume step has neither --ack-id nor --ack-text")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::queue_claim::{claim, report_attempt, start_attempt};
    use crate::queue_consume::QueueConsumeWriteEffects;
    use crate::queue_lineage::record_prose_only;
    use agent_doc_queue::queue_claim::QueueClaimState;
    use anyhow::Context;
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;

    struct DiskConsume;

    impl QueueConsumeWriteEffects for DiskConsume {
        fn current_document_content(&self, file: &Path, _source: &str) -> Result<String> {
            fs::read_to_string(file).with_context(|| format!("failed to read {}", file.display()))
        }

        fn atomic_write(&self, file: &Path, content: &str) -> Result<()> {
            agent_doc_fs::write_atomic(file, content.as_bytes())
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            _source_content: &str,
            _reason: &str,
        ) -> Result<()> {
            self.atomic_write(file, target_content)
        }
    }

    struct DiskEffects;

    impl QueueCommandEffects for DiskEffects {
        fn current_document_content(&self, file: &Path, source: &str) -> Result<String> {
            DiskConsume.current_document_content(file, source)
        }

        fn converge_document_or_disk(
            &self,
            file: &Path,
            target_content: &str,
            source_content: &str,
            reason: &str,
        ) -> Result<()> {
            DiskConsume.converge_document_or_disk(file, target_content, source_content, reason)
        }

        fn consume_free_text_queue_prompts_force_disk(
            &self,
            _file: &Path,
            _count: usize,
            _apply_guard: &crate::queue_edit_owner::QueueEditGuard,
        ) -> Result<Option<crate::queue_cmd::QueueCommandConsumeOutcome>> {
            bail!("unused")
        }

        fn consume_free_text_queue_prompts_with_outcome(
            &self,
            _file: &Path,
            _count: usize,
            _apply_guard: &crate::queue_edit_owner::QueueEditGuard,
        ) -> Result<Option<crate::queue_cmd::QueueCommandConsumeOutcome>> {
            bail!("unused")
        }

        fn strike_orphan_id_backed_queue_head(
            &self,
            _file: &Path,
            _id: &str,
            _apply_guard: &crate::queue_edit_owner::QueueEditGuard,
        ) -> Result<bool> {
            bail!("unused")
        }

        fn acknowledge_open_id_backed_queue_head(
            &self,
            file: &Path,
            id: &str,
            apply_guard: &crate::queue_edit_owner::QueueEditGuard,
        ) -> Result<bool> {
            self.acknowledge_open_id_backed_queue_head_for_attempt(file, id, None, apply_guard)
        }

        fn acknowledge_open_id_backed_queue_head_for_attempt(
            &self,
            file: &Path,
            id: &str,
            attempt: Option<&QueueOwnerProjection>,
            apply_guard: &crate::queue_edit_owner::QueueEditGuard,
        ) -> Result<bool> {
            crate::queue_consume::acknowledge_open_id_backed_queue_head_under_apply_guard(
                file,
                id,
                attempt,
                &DiskConsume,
                apply_guard,
            )
        }

        fn acknowledge_paused_free_text_queue_head(
            &self,
            file: &Path,
            expected_head: &str,
            apply_guard: &crate::queue_edit_owner::QueueEditGuard,
        ) -> Result<Option<crate::queue_cmd::QueueCommandConsumeOutcome>> {
            self.acknowledge_paused_free_text_queue_head_with_selector(
                file,
                expected_head,
                None,
                apply_guard,
            )
        }

        fn acknowledge_paused_free_text_queue_head_with_selector(
            &self,
            file: &Path,
            expected_head: &str,
            selector: Option<&AckTextSelector>,
            apply_guard: &crate::queue_edit_owner::QueueEditGuard,
        ) -> Result<Option<crate::queue_cmd::QueueCommandConsumeOutcome>> {
            crate::queue_consume::acknowledge_paused_free_text_queue_head_under_apply_guard(
                file,
                expected_head,
                selector,
                &DiskConsume,
                apply_guard,
            )
            .map(|outcome| {
                outcome.map(|outcome| crate::queue_cmd::QueueCommandConsumeOutcome {
                    consumed_text: outcome.consumed_text,
                    consumed_count: outcome.consumed_count,
                    remaining: outcome.remaining,
                    drained: outcome.drained,
                })
            })
        }

        fn prune_noise_queue_heads(
            &self,
            _file: &Path,
            _apply_guard: &crate::queue_edit_owner::QueueEditGuard,
        ) -> Result<usize> {
            bail!("unused")
        }
    }

    fn write_doc(root: &Path, prompt: &str) -> PathBuf {
        fs::create_dir_all(root.join(".agent-doc")).unwrap();
        let doc = root.join("task.md");
        fs::write(
            &doc,
            format!(
                "---\nsession: sid\nagent_doc_format: template\n---\n\n\
## Exchange\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n\n\
## Queue\n\n<!-- agent:queue go -->\n- {prompt}\n<!-- /agent:queue -->\n"
            ),
        )
        .unwrap();
        doc
    }

    fn project_reported(doc: &Path) {
        crate::queue_cmd::project_claim_runtime_state_with_effects(&DiskEffects, doc, None)
            .unwrap();
    }

    fn capture(doc: &Path, prompt: &str, generation: u64, response: &str) {
        crate::claim_release_receipt::capture_response_for_attempt(
            doc, prompt, generation, response,
        )
        .unwrap();
    }

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn init_repo(repo: &Path) -> String {
        fs::create_dir_all(repo).unwrap();
        git(repo, &["init", "-b", "main"]);
        git(repo, &["config", "user.email", "dev@example.com"]);
        git(repo, &["config", "user.name", "Dev"]);
        fs::write(repo.join("README"), "a\n").unwrap();
        git(repo, &["add", "README"]);
        git(repo, &["commit", "-m", "init"]);
        let output = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().trim().to_string()
    }

    #[test]
    fn prose_reported_row_writes_the_response_and_strikes_in_the_same_pass() {
        let dir = tempfile::tempdir().unwrap();
        let prompt = "review the pull request";
        let doc = write_doc(dir.path(), prompt);
        let (_, claimed) = claim(&doc, prompt, "subagent:review", 600).unwrap();
        let started = start_attempt(
            &doc,
            prompt,
            "subagent:review",
            "worker:review",
            600,
            claimed.generation,
        )
        .unwrap();
        record_prose_only(&doc, &started, "prose result").unwrap();
        capture(
            &doc,
            prompt,
            started.generation,
            "The discussion is already resolved.",
        );
        report_attempt(&doc, prompt, Some(started.generation)).unwrap();
        project_reported(&doc);

        let pass = close_mechanical_reported_attempts(&DiskEffects, &doc).unwrap();
        assert_eq!(pass.outcomes.len(), 1, "{pass:#?}");
        assert!(pass.outcomes[0].closed(), "{pass:#?}");
        assert!(pass.document_changed, "the saved response must be written");
        let updated = fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("The discussion is already resolved."),
            "{updated}"
        );
        assert!(updated.contains("### Re: Subagent completed"), "{updated}");
        assert!(updated.contains("~~"), "{updated}");
        assert!(!updated.contains("☑️"), "{updated}");
        let ledger = crate::queue_claim::load_ledger(&doc).unwrap();
        assert!(
            ledger
                .claims
                .iter()
                .all(|claim| claim.state != QueueClaimState::Reported),
            "{ledger:?}"
        );
    }

    #[test]
    fn prose_row_without_a_captured_response_stays_reported() {
        let dir = tempfile::tempdir().unwrap();
        let prompt = "review the pull request";
        let doc = write_doc(dir.path(), prompt);
        let (_, claimed) = claim(&doc, prompt, "subagent:review", 600).unwrap();
        let started = start_attempt(
            &doc,
            prompt,
            "subagent:review",
            "worker:review",
            600,
            claimed.generation,
        )
        .unwrap();
        record_prose_only(&doc, &started, "prose result").unwrap();
        report_attempt(&doc, prompt, Some(started.generation)).unwrap();
        project_reported(&doc);

        let pass = close_mechanical_reported_attempts(&DiskEffects, &doc).unwrap();
        assert!(
            matches!(
                pass.outcomes[0].result,
                MechanicalCloseoutResult::LeftReported { .. }
            ),
            "{pass:#?}"
        );
        let updated = fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("☑️"), "{updated}");
        assert!(!updated.contains("~~"), "{updated}");
    }

    #[test]
    fn contained_sink_is_recorded_and_consumed() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let sha = init_repo(&repo);
        let prompt = "land the note";
        let doc = write_doc(dir.path(), prompt);
        let (_, claimed) = claim(&doc, prompt, "subagent:land", 600).unwrap();
        let started = start_attempt(
            &doc,
            prompt,
            "subagent:land",
            "worker:land",
            600,
            claimed.generation,
        )
        .unwrap();
        crate::queue_lineage::record_claim(&doc, &started).unwrap();
        crate::queue_lineage::record(
            &doc,
            prompt,
            started.generation,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                worker_id: Some("worker:land".to_string()),
                source_repo_path: Some(repo.clone()),
                sink_repo_path: Some(repo.clone()),
                source_sha: Some(sha.clone()),
                sink_ref: Some("refs/heads/main".to_string()),
                detail: Some("committed on main".to_string()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        capture(&doc, prompt, started.generation, "Landed on main.");
        report_attempt(&doc, prompt, Some(started.generation)).unwrap();
        project_reported(&doc);

        let pass = close_mechanical_reported_attempts(&DiskEffects, &doc).unwrap();
        assert!(pass.outcomes[0].closed(), "{pass:#?}");
        let updated = fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("Landed on main."), "{updated}");
        assert!(!updated.contains("☑️"), "{updated}");
        let ledger = crate::queue_lineage::load(&doc).unwrap();
        assert!(
            ledger.attempts.iter().any(|attempt| {
                attempt.events.iter().any(|event| {
                    event.kind == QueueLineageEventKind::Integrated
                        && event.source_sha.as_deref() == Some(sha.as_str())
                })
            }),
            "{ledger:?}"
        );
    }

    #[test]
    fn sink_that_does_not_contain_the_source_stays_reported() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path().join("repo");
        let base = init_repo(&repo);
        git(&repo, &["checkout", "-b", "feature"]);
        fs::write(repo.join("README"), "b\n").unwrap();
        git(&repo, &["add", "README"]);
        git(&repo, &["commit", "-m", "feature"]);
        let feature = {
            let output = Command::new("git")
                .arg("-C")
                .arg(&repo)
                .args(["rev-parse", "HEAD"])
                .output()
                .unwrap();
            String::from_utf8(output.stdout).unwrap().trim().to_string()
        };
        assert_ne!(base, feature);
        let prompt = "land the feature";
        let doc = write_doc(dir.path(), prompt);
        let (_, claimed) = claim(&doc, prompt, "subagent:feature", 600).unwrap();
        let started = start_attempt(
            &doc,
            prompt,
            "subagent:feature",
            "worker:feature",
            600,
            claimed.generation,
        )
        .unwrap();
        crate::queue_lineage::record_claim(&doc, &started).unwrap();
        crate::queue_lineage::record(
            &doc,
            prompt,
            started.generation,
            RecordRequest {
                kind: QueueLineageEventKind::Committed,
                worker_id: Some("worker:feature".to_string()),
                source_repo_path: Some(repo.clone()),
                sink_repo_path: Some(repo),
                source_sha: Some(feature),
                sink_ref: Some("refs/heads/main".to_string()),
                ..RecordRequest::default()
            },
        )
        .unwrap();
        capture(
            &doc,
            prompt,
            started.generation,
            "Feature is on a side branch.",
        );
        report_attempt(&doc, prompt, Some(started.generation)).unwrap();
        project_reported(&doc);

        let pass = close_mechanical_reported_attempts(&DiskEffects, &doc).unwrap();
        assert!(
            matches!(
                pass.outcomes[0].result,
                MechanicalCloseoutResult::LeftReported { .. }
            ),
            "{pass:#?}"
        );
        let updated = fs::read_to_string(&doc).unwrap();
        assert!(updated.contains("☑️"), "{updated}");
        assert!(updated.contains("land the feature"), "{updated}");
        assert!(!updated.contains("~~"), "{updated}");
    }

    /// One ` |>` stage through the real worker lifecycle: claim, start,
    /// capture the hand-back, report, and the mechanical reported closeout.
    fn run_stage(doc: &Path, item: &str, owner: &str, hand_back: &str) {
        let (_, claimed) =
            claim(doc, item, owner, 600).unwrap_or_else(|err| panic!("claim {item:?}: {err:#}"));
        let started =
            start_attempt(doc, item, owner, "worker:stage", 600, claimed.generation).unwrap();
        record_prose_only(doc, &started, "prose result").unwrap();
        capture(doc, item, started.generation, hand_back);
        report_attempt(doc, item, Some(started.generation)).unwrap();
        project_reported(doc);
        let pass = close_mechanical_reported_attempts(&DiskEffects, doc).unwrap();
        assert_eq!(pass.outcomes.len(), 1, "{item}: {pass:#?}");
        assert!(pass.outcomes[0].closed(), "{item}: {pass:#?}");
        // The next preflight's claim projection repaints the released holds.
        project_reported(doc);
    }

    fn queue_body(doc: &Path) -> String {
        let content = fs::read_to_string(doc).unwrap();
        let start = content.find("<!-- agent:queue go -->\n").unwrap() + 24;
        let end = content.find("<!-- /agent:queue -->").unwrap();
        content[start..end].to_string()
    }

    fn runnable_rows(doc: &Path) -> Vec<String> {
        let entries = agent_doc_queue::document_queue::parse(&queue_body(doc)).unwrap();
        let waits = agent_doc_queue::document_queue::ordered_step_waits(&entries);
        agent_doc_queue::document_queue::executable_prompt_indexes(&entries)
            .into_iter()
            .filter(|index| waits[*index].is_none())
            .filter_map(|index| match &entries[index] {
                agent_doc_queue::document_queue::QueueEntry::Prompt(prompt) => {
                    Some(agent_doc_element_queue::pipe::row_work_text(&prompt.text))
                }
                _ => None,
            })
            .collect()
    }

    /// `#pipeprompt` ` |>` end to end: stage 1, two sequential stages, then a
    /// nested ` |>` whose bulleted stages fan out in parallel. Each stage is
    /// claimed by its own worker only once the previous one closed, its brief
    /// carries the previous hand-back, every closeout is the automatic
    /// reported closeout, and each ` |>` row is struck with its last stage.
    #[test]
    fn hand_off_pipe_runs_sequential_stages_then_a_parallel_fan_out() {
        let dir = tempfile::tempdir().unwrap();
        let doc = write_doc(
            dir.path(),
            "review the PR |>\n  1. address the findings\n  2. summarize the changes |>\n     - post to the tracker\n     - post to the channel",
        );
        assert_eq!(runnable_rows(&doc), vec!["review the PR"]);
        // A held stage refuses its claim before stage 1 closed.
        let refused = claim(&doc, "address the findings", "subagent:early", 600)
            .expect_err("stage 2 waits on stage 1");
        assert!(
            format!("{refused:#}").contains("hand-back of stage 1"),
            "{refused:#}"
        );

        run_stage(
            &doc,
            "review the PR |>",
            "subagent:review",
            "Two findings: the parser drops `N)` rows; the lint misses ` |>`.",
        );
        let body = queue_body(&doc);
        assert!(body.contains("✅ review the PR |>"), "{body}");
        assert!(!body.contains("~~review the PR"), "{body}");
        assert!(!body.contains("☑️"), "{body}");
        assert_eq!(runnable_rows(&doc), vec!["address the findings"]);
        let ledger = crate::queue_claim::load_ledger(&doc).unwrap();
        assert!(
            ledger
                .claims
                .iter()
                .all(|claim| claim.state != QueueClaimState::Reported),
            "the head's claim settles at the close transition: {ledger:?}"
        );

        // Stage 2's brief carries stage 1's hand-back.
        let content = fs::read_to_string(&doc).unwrap();
        let brief = agent_doc_queue::subagent_brief::subagent_authorization(
            "task.md",
            &content,
            "address the findings",
        );
        assert!(
            brief
                .subagent_prompt_preamble
                .contains("the parser drops `N)` rows"),
            "{}",
            brief.subagent_prompt_preamble
        );

        run_stage(
            &doc,
            "address the findings",
            "subagent:address",
            "Fixed both findings in commit abc123.",
        );
        assert_eq!(runnable_rows(&doc), vec!["summarize the changes"]);
        let content = fs::read_to_string(&doc).unwrap();
        let input = agent_doc_queue::hand_off::hand_off_input_for_item(
            &content,
            "summarize the changes |>",
        )
        .expect("stage 3 input");
        assert_eq!(input.from_stage, "address the findings");
        assert!(
            input.hand_backs[0].text.contains("abc123"),
            "{:?}",
            input.hand_backs
        );

        run_stage(
            &doc,
            "summarize the changes |>",
            "subagent:summary",
            "Summary: parser and lint fixed.",
        );
        let body = queue_body(&doc);
        assert!(body.contains("✅ summarize the changes |>"), "{body}");
        assert!(body.contains("✅ review the PR |>"), "{body}");
        // The fan-out: both posts run at once, each with the summary.
        assert_eq!(
            runnable_rows(&doc),
            vec!["post to the tracker", "post to the channel"]
        );
        let content = fs::read_to_string(&doc).unwrap();
        for post in ["post to the tracker", "post to the channel"] {
            let input = agent_doc_queue::hand_off::hand_off_input_for_item(&content, post)
                .unwrap_or_else(|| panic!("{post} input"));
            assert_eq!(input.from_stage, "summarize the changes");
            assert!(input.hand_backs[0].text.contains("parser and lint fixed"));
        }

        run_stage(&doc, "post to the tracker", "subagent:tracker", "Posted.");
        let body = queue_body(&doc);
        assert!(body.contains("✅ summarize the changes |>"), "{body}");
        run_stage(&doc, "post to the channel", "subagent:channel", "Posted.");
        let body = queue_body(&doc);
        assert!(body.contains("~~summarize the changes |>~~"), "{body}");
        assert!(body.contains("~~review the PR |>~~"), "{body}");
        assert!(!body.contains('✅'), "{body}");
        assert!(runnable_rows(&doc).is_empty(), "{body}");
    }
}
