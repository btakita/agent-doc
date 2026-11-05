//! Operator answer-ledger enforcement at preflight and closeout
//! (`#answerbyrequestid`).
//!
//! Every preflight and every strict closeout runs [`run_answer_ledger_maintenance`]:
//! it reads the document through the same authority as todo-id maintenance,
//! builds the ledger from the document, its done archives and the state.db
//! index, closes duplicate todo copies of answered questions, refreshes the
//! index, and returns the "already answered" facts for the cycle contract.
//! [`reasks_for_closeout`] is the pre-write gate a closeout runs on the
//! request packets and `--todo-add` texts it is about to record. No agent
//! command is involved in any of it.

use std::path::Path;

use agent_doc_answer_ledger::{
    AnsweredFact, DuplicateClosure, Ledger, PossibleMatch, PriorIndex, PriorKey, PriorRevision,
    Reask,
};
use anyhow::Result;

/// What one ledger pass produced.
#[derive(Debug, Clone, Default)]
pub struct AnswerLedgerReport {
    pub facts: Vec<AnsweredFact>,
    pub possible: Vec<PossibleMatch>,
    pub closed: Vec<DuplicateClosure>,
    /// `#decisionstaleaudit`: open todo rows a delivered operator answer
    /// already settled (identity tier), closed by this pass.
    pub stale_closed: Vec<DuplicateClosure>,
}

impl AnswerLedgerReport {
    /// Cycle-contract guidance; `None` when nothing is answered or possible.
    pub fn guidance(&self) -> Option<String> {
        agent_doc_answer_ledger::contract_guidance(&self.facts, &self.possible)
    }

    /// `#decisionstaleaudit`: todo signals computed before this pass, minus
    /// the ones about todo rows this pass closed (their audit obligation is
    /// already met, so the contract never asks to close a closed row).
    pub fn without_closed_todos(&self, signals: &[crate::TodoSignal]) -> Vec<crate::TodoSignal> {
        let closed: std::collections::BTreeSet<String> = self
            .closed
            .iter()
            .chain(&self.stale_closed)
            .map(|closure| closure.todo_id.to_ascii_lowercase())
            .collect();
        signals
            .iter()
            .filter(|signal| {
                let about_todo_row = match signal.kind.as_str() {
                    "tracked_work_missing_todo_backlink" => false,
                    "stale_tracked_item" => signal.component.as_deref() == Some("todo"),
                    _ => true,
                };
                !(about_todo_row && closed.contains(&signal.todo.to_ascii_lowercase()))
            })
            .cloned()
            .collect()
    }
}

fn archive_texts(file: &Path, content: &str) -> Vec<String> {
    match agent_doc_element_backlog_io::done_archive::read_done_archive_sources(file, content) {
        Ok(sources) => sources.into_iter().map(|source| source.content).collect(),
        Err(err) => {
            eprintln!(
                "[preflight] answer ledger: done archive unreadable ({err:#}); using the document only"
            );
            Vec::new()
        }
    }
}

fn index_target(file: &Path) -> Option<(std::path::PathBuf, String)> {
    let root = agent_doc_project_root_io::project_root_containing(file)?;
    Some((root, agent_doc_hash::document_id_for_path(file)))
}

fn load_prior(file: &Path) -> PriorIndex {
    let Some((root, document_id)) = index_target(file) else {
        return PriorIndex::default();
    };
    match agent_doc_sqlite::answer_ledger_index::load_for_project(&root, &document_id) {
        None => PriorIndex::default(),
        Some(Ok((keys, revisions))) => PriorIndex {
            keys: keys
                .into_iter()
                .map(|row| PriorKey {
                    key: row.key,
                    decision_id: row.decision_id,
                    first_seen_at: row.first_seen_at,
                })
                .collect(),
            revisions: revisions
                .into_iter()
                .map(|row| PriorRevision {
                    decision_id: row.decision_id,
                    answer: row.answer,
                    location: row.location,
                    relation: row.relation,
                    first_seen_at: row.first_seen_at,
                    change_flagged: row.change_flagged,
                })
                .collect(),
        },
        Some(Err(err)) => {
            eprintln!(
                "[preflight] answer ledger: index unreadable ({err:#}); rebuilding from the document"
            );
            PriorIndex::default()
        }
    }
}

fn store_index(file: &Path, ledger: &Ledger, now: u64) {
    let Some((root, document_id)) = index_target(file) else {
        return;
    };
    let (keys, revisions) = ledger.index_rows();
    let keys: Vec<_> = keys
        .into_iter()
        .map(|key| agent_doc_sqlite::answer_ledger_index::AnswerKeyRow {
            key: key.key,
            decision_id: key.decision_id,
            first_seen_at: key.first_seen_at,
        })
        .collect();
    let revisions: Vec<_> = revisions
        .into_iter()
        .map(
            |revision| agent_doc_sqlite::answer_ledger_index::AnswerRevisionRow {
                decision_id: revision.decision_id,
                answer: revision.answer,
                location: revision.location,
                relation: revision.relation,
                first_seen_at: revision.first_seen_at,
                change_flagged: revision.change_flagged,
            },
        )
        .collect();
    let edges: Vec<_> = ledger
        .edges
        .iter()
        .map(
            |edge| agent_doc_sqlite::answer_ledger_index::AnswerEdgeRow {
                from: edge.from.clone(),
                kind: edge.kind.as_str().to_string(),
                to: edge.to.clone(),
            },
        )
        .collect();
    if let Err(err) = agent_doc_sqlite::answer_ledger_index::store_for_project(
        &root,
        &document_id,
        &keys,
        &revisions,
        &edges,
        now,
    ) {
        eprintln!("[preflight] answer ledger: index not refreshed ({err:#})");
    }
}

/// Build the ledger for `content` (read-only: no index write).
pub fn build_ledger(
    file: &Path,
    content: &str,
) -> (agent_doc_answer_ledger::extract::Observations, Ledger) {
    let archives = archive_texts(file, content);
    let refs: Vec<&str> = archives.iter().map(String::as_str).collect();
    let observations = agent_doc_answer_ledger::extract::observe(content, &refs);
    let prior = load_prior(file);
    let ledger = Ledger::build(
        &observations,
        &prior,
        agent_doc_log_time::current_epoch_secs(),
        None,
    );
    (observations, ledger)
}

fn read_authoritative(file: &Path, force_disk: bool) -> Result<Option<String>> {
    if force_disk {
        return Ok(std::fs::read_to_string(file).ok());
    }
    Ok(
        match super::current_text_via_preflight_authority_retrying(file, "preflight_answer_ledger")?
        {
            Some(agent_doc_crdt_relay_io::CurrentText::Current { text, .. }) => Some(text),
            Some(agent_doc_crdt_relay_io::CurrentText::Detached) | None => {
                std::fs::read_to_string(file).ok()
            }
            Some(_) => None,
        },
    )
}

/// Preflight / closeout pass: close duplicate copies of answered questions,
/// refresh the index, and return the facts for the cycle contract.
pub fn run_answer_ledger_maintenance(file: &Path) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_options(file, false, None)
}

/// Force-disk counterpart for an explicitly force-disk strict closeout.
pub fn run_answer_ledger_maintenance_force_disk(
    file: &Path,
    write_effects: &dyn super::PreflightMaintenanceWriteEffects,
) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_options(file, true, Some(write_effects))
}

fn run_answer_ledger_maintenance_with_options(
    file: &Path,
    force_disk: bool,
    write_effects: Option<&dyn super::PreflightMaintenanceWriteEffects>,
) -> Result<AnswerLedgerReport> {
    let source = "preflight_answer_ledger";
    let Some(content) = read_authoritative(file, force_disk)? else {
        return Ok(AnswerLedgerReport::default());
    };
    let now = agent_doc_log_time::current_epoch_secs();
    let (observations, ledger) = build_ledger(file, &content);
    let closed = ledger.duplicate_closures(&observations);
    // `#decisionstaleaudit`: identity-tier stale todo rows close in the same
    // write; every other stale finding is only a signal.
    let stale_closed: Vec<DuplicateClosure> = ledger
        .stale_auto_closures(&observations)
        .into_iter()
        .filter(|stale| !closed.iter().any(|dup| dup.todo_id == stale.todo_id))
        .collect();
    let all_closed: Vec<DuplicateClosure> =
        closed.iter().chain(stale_closed.iter()).cloned().collect();
    let mut report_ledger = ledger;
    if !all_closed.is_empty()
        && let Some(target) =
            agent_doc_answer_ledger::apply_duplicate_closures(&content, &all_closed)?
    {
        let persisted = if force_disk {
            let effects = write_effects.ok_or_else(|| {
                anyhow::anyhow!("force-disk answer-ledger maintenance requires write effects")
            })?;
            super::persist_pending_maintenance_doc(file, &content, &target, source, true, effects)?;
            target.clone()
        } else {
            let root = agent_doc_project_root_io::project_root_containing(file);
            super::persist_queue_maintenance_doc(file, &target, &content, root.as_deref(), source)?
        };
        if let Ok(Some(snapshot)) = agent_doc_snapshot_io::load_document_baseline(file)
            && let Some(new_snapshot) = agent_doc_element::todo::carry_todo_body_into_snapshot(
                &snapshot, &content, &persisted,
            )
            && let Err(err) = agent_doc_snapshot_io::checkpoint_document_baseline(
                file,
                &new_snapshot,
                agent_doc_ops_log_io::log_op,
            )
        {
            eprintln!("[preflight] answer ledger: snapshot sync warning: {err}");
        }
        for closure in &closed {
            eprintln!(
                "[preflight] answer ledger: closed duplicate todo [#{}] — already answered `{}`: “{}” ({}) (#answerbyrequestid)",
                closure.todo_id, closure.decision_id, closure.answer, closure.location
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "answer_ledger_duplicate_closed file={} todo={} decision={}",
                    file.display(),
                    closure.todo_id,
                    closure.decision_id
                ),
            );
        }
        for closure in &stale_closed {
            eprintln!(
                "[preflight] answer ledger: closed answered todo [#{}] — already answered `{}`: “{}” ({}) (#decisionstaleaudit)",
                closure.todo_id, closure.decision_id, closure.answer, closure.location
            );
            agent_doc_ops_log_io::log_op(
                file,
                &format!(
                    "answer_ledger_stale_closed file={} todo={} decision={}",
                    file.display(),
                    closure.todo_id,
                    closure.decision_id
                ),
            );
        }
        let (_, rebuilt) = build_ledger(file, &persisted);
        report_ledger = rebuilt;
    }
    store_index(file, &report_ledger, now);
    let facts = report_ledger.answered_facts();
    for fact in &facts {
        eprintln!(
            "[preflight] answer ledger: already answered `{}` ({}): “{}”",
            fact.decision_id,
            fact.open_items.join(", "),
            fact.answer
        );
    }
    Ok(AnswerLedgerReport {
        facts,
        possible: report_ledger.possible.clone(),
        closed,
        stale_closed,
    })
}

/// `#decisionstaleaudit`: every open tracked item a delivered decision makes
/// stale, read-only (no index write, no document write).
pub fn stale_tracked_items(
    file: &Path,
    content: &str,
) -> Vec<agent_doc_answer_ledger::stale::StaleTrackedItem> {
    let (observations, ledger) = build_ledger(file, content);
    ledger.stale_tracked_items(&observations)
}

/// Pre-write closeout gate: the questions this closeout would add that the
/// operator already answered and that do not cite the answer.
pub fn reasks_for_closeout(
    file: &Path,
    content: &str,
    response: &str,
    todo_adds: &[String],
) -> Vec<Reask> {
    let new_questions =
        agent_doc_answer_ledger::extract::observe_new_questions(response, todo_adds);
    if new_questions.is_empty() {
        return Vec::new();
    }
    let (_, ledger) = build_ledger(file, content);
    ledger.reasks(&new_questions)
}

#[cfg(test)]
mod tests {
    use super::*;

    const DOC: &str = concat!(
        "---\nagent_doc_session: answer-ledger\n---\n\n",
        "<!-- agent:queue -->\n",
        "- ~~[#subagent-operator-request-tdport-allowlist]: Use the standard http ports.~~\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:todo -->\n",
        "- [ ] [#tdport] Register the demo sign-in redirect, or pick an already-registered port\n",
        "- [ ] [#subagent-operator-request-tdport-allowlist] Choose how to add the demo callback: stop, studio append, or local demo only\n",
        "<!-- /agent:todo -->\n",
    );

    /// `#decisionstaleaudit`: an idle todo whose own id a delivered operator
    /// answer settled closes in the maintenance write (identity tier), once;
    /// a backlog question with the same kind of answer is never closed.
    #[test]
    fn maintenance_closes_a_settled_todo_once_and_leaves_tracked_work_open() {
        const STALE_DOC: &str = concat!(
            "---\nagent_doc_session: stale-audit\n---\n\n",
            "<!-- agent:run id=\"main\" -->\n",
            "- ### ~~[#tdapprovers]: Make a PR to link the ADRs ⟦published⟧~~\n",
            "- ### ~~[#nextsteps]: Run the go-live checklist first.~~\n",
            "<!-- /agent:run -->\n\n",
            "<!-- agent:todo -->\n",
            "- [ ] [#tdapprovers] Name the approver for docs PR #88 and request their review\n",
            "<!-- /agent:todo -->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] [#nextsteps] What are next steps to make the docs site public?\n",
            "<!-- /agent:backlog -->\n",
        );
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, STALE_DOC).unwrap();
        let report = run_answer_ledger_maintenance_force_disk(&doc, &DiskEffects).unwrap();
        assert!(report.closed.is_empty(), "{report:?}");
        assert_eq!(report.stale_closed.len(), 1, "{report:?}");
        assert_eq!(report.stale_closed[0].todo_id, "tdapprovers");
        let written = std::fs::read_to_string(&doc).unwrap();
        assert!(written.contains("- [x] [#tdapprovers]"), "{written}");
        assert!(written.contains("already answered `"), "{written}");
        assert!(written.contains("- [ ] [#nextsteps]"), "{written}");
        let again = run_answer_ledger_maintenance_force_disk(&doc, &DiskEffects).unwrap();
        assert!(again.stale_closed.is_empty(), "{again:?}");
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), written);
        let stale = stale_tracked_items(&doc, &written);
        assert_eq!(stale.len(), 1, "{stale:?}");
        assert_eq!(stale[0].item, "nextsteps");
        assert!(!stale[0].auto_close);
    }

    struct DiskEffects;
    impl crate::PreflightMaintenanceWriteEffects for DiskEffects {
        fn record_document_write_provenance(&self, _file: &Path, _content: &str) {}

        fn guard_visible_write_expected_current(
            &self,
            _file: &Path,
            _source: &str,
            _expected: &str,
        ) -> Result<()> {
            Ok(())
        }

        fn converge_or_disk_write(
            &self,
            file: &Path,
            _current: &str,
            target: &str,
            _source: &str,
        ) -> Result<()> {
            std::fs::write(file, target)?;
            Ok(())
        }
    }

    /// Agent-free path: a plain force-disk maintenance pass (the closeout
    /// path) closes the stale request copy, keeps the work todo open, and the
    /// facts carry the answer; no agent command is involved.
    #[test]
    fn maintenance_closes_the_answered_copy_and_reports_facts() {
        struct NoEffects;
        impl crate::PreflightMaintenanceWriteEffects for NoEffects {
            fn record_document_write_provenance(&self, _file: &Path, _content: &str) {}

            fn guard_visible_write_expected_current(
                &self,
                _file: &Path,
                _source: &str,
                _expected: &str,
            ) -> Result<()> {
                Ok(())
            }

            fn converge_or_disk_write(
                &self,
                file: &Path,
                _current: &str,
                target: &str,
                _source: &str,
            ) -> Result<()> {
                std::fs::write(file, target)?;
                Ok(())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let doc = dir.path().join("session.md");
        std::fs::write(&doc, DOC).unwrap();
        let report = run_answer_ledger_maintenance_force_disk(&doc, &NoEffects).unwrap();
        assert_eq!(report.closed.len(), 1, "{report:?}");
        let written = std::fs::read_to_string(&doc).unwrap();
        assert!(
            written.contains("- [x] [#subagent-operator-request-tdport-allowlist]"),
            "{written}"
        );
        assert!(written.contains("already answered `"), "{written}");
        assert!(written.contains("- [ ] [#tdport]"), "{written}");
        // Idempotent second pass.
        let again = run_answer_ledger_maintenance_force_disk(&doc, &NoEffects).unwrap();
        assert!(again.closed.is_empty());
        assert_eq!(std::fs::read_to_string(&doc).unwrap(), written);

        // Re-asking the same request id at closeout is refused.
        let response = "[#subagent-operator-request-tdport-allowlist] subagent operator request\n- *kind:* question\n- *blocking:* true\n- *request:*\n  - Choose how to add the demo callback.\n- *choices:* []\n";
        let reasks = reasks_for_closeout(&doc, &written, response, &[]);
        assert_eq!(reasks.len(), 1, "{reasks:?}");
        assert!(
            agent_doc_answer_ledger::reask_refusal(&reasks)
                .contains("Use the standard http ports.")
        );
    }
}
