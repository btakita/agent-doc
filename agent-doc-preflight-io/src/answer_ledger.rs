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
//!
//! Preflight alone ([`run_answer_ledger_maintenance_with_recall`]) also asks
//! the optional tsift recall source ([`crate::answer_recall`]) for semantic
//! neighbours of the open, unanswered questions, inside this document and in
//! other session documents (`#decisiontsiftrecall`). Those stay "possibly
//! answered" notes; without tsift the pass is exactly the in-process one.

use std::path::Path;

use agent_doc_answer_ledger::recall::{AnswerRecall, OtherDocument, RecallOptions, RecalledAnswer};
use agent_doc_answer_ledger::{
    AnsweredFact, DuplicateClosure, Ledger, PossibleMatch, PriorIndex, PriorKey, PriorRevision,
    Reask, SemanticNeighbours,
};
use anyhow::Result;

use crate::answer_recall::{RecallEnv, TsiftRecall};

/// What one ledger pass produced.
#[derive(Debug, Clone, Default)]
pub struct AnswerLedgerReport {
    pub facts: Vec<AnsweredFact>,
    pub possible: Vec<PossibleMatch>,
    pub closed: Vec<DuplicateClosure>,
    /// Cross-document "possibly answered elsewhere" notes (tsift recall).
    pub recalled: Vec<RecalledAnswer>,
}

impl AnswerLedgerReport {
    /// Cycle-contract guidance; `None` when nothing is answered, possible or
    /// recalled.
    pub fn guidance(&self) -> Option<String> {
        let parts: Vec<String> = [
            agent_doc_answer_ledger::contract_guidance(&self.facts, &self.possible),
            agent_doc_answer_ledger::recall::recall_guidance(&self.recalled),
        ]
        .into_iter()
        .flatten()
        .collect();
        (!parts.is_empty()).then(|| parts.join("\n"))
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
    build_ledger_with(file, content, None)
}

fn build_ledger_with(
    file: &Path,
    content: &str,
    neighbours: Option<&dyn SemanticNeighbours>,
) -> (agent_doc_answer_ledger::extract::Observations, Ledger) {
    let archives = archive_texts(file, content);
    let refs: Vec<&str> = archives.iter().map(String::as_str).collect();
    let observations = agent_doc_answer_ledger::extract::observe(content, &refs);
    let prior = load_prior(file);
    let ledger = Ledger::build(
        &observations,
        &prior,
        agent_doc_log_time::current_epoch_secs(),
        neighbours,
    );
    (observations, ledger)
}

/// Largest other document recall reads.
const MAX_RECALL_DOCUMENT_BYTES: u64 = 4 * 1024 * 1024;

fn canonical(path: &Path) -> std::path::PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// `#decisiontsiftrecall`: one recall pass over `ledger` (built from
/// `content`). Returns the ledger rebuilt with in-document neighbours (when
/// any) and the cross-document notes. Annotation only.
fn recall_pass(
    file: &Path,
    content: &str,
    ledger: Ledger,
    source: &dyn AnswerRecall,
    root: &Path,
) -> (Ledger, Vec<RecalledAnswer>) {
    let here = canonical(file);
    let mut excluded: std::collections::BTreeSet<std::path::PathBuf> =
        agent_doc_element_backlog_io::done_archive::read_done_archive_sources(file, content)
            .map(|sources| {
                sources
                    .into_iter()
                    .map(|source| canonical(&source.path))
                    .collect()
            })
            .unwrap_or_default();
    excluded.insert(here.clone());
    let rows = agent_doc_answer_ledger::extract::observe(content, &[]);
    let now = agent_doc_log_time::current_epoch_secs();
    let mut load = |path: &str| -> Option<OtherDocument> {
        let path = Path::new(path);
        if excluded.contains(path)
            || std::fs::metadata(path).ok()?.len() > MAX_RECALL_DOCUMENT_BYTES
        {
            return None;
        }
        let text = std::fs::read_to_string(path).ok()?;
        let rows = agent_doc_answer_ledger::extract::observe(&text, &[]);
        if rows.answers.is_empty() {
            return None;
        }
        Some(OtherDocument {
            display: path
                .strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .into_owned(),
            ledger: Ledger::build(&rows, &PriorIndex::default(), now, None),
            rows,
        })
    };
    let pass = agent_doc_answer_ledger::recall::run_recall(
        &ledger,
        &rows,
        &here.to_string_lossy(),
        source,
        RecallOptions::default(),
        &mut load,
    );
    if pass.queries > 0 {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "answer_ledger_recall file={} queries={} neighbours={} recalled={}",
                file.display(),
                pass.queries,
                pass.neighbours.len(),
                pass.recalled.len()
            ),
        );
    }
    let ledger = if pass.neighbours.is_empty() {
        ledger
    } else {
        build_ledger_with(file, content, Some(&pass.neighbours)).1
    };
    (ledger, pass.recalled)
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
    run_answer_ledger_maintenance_with_options(file, false, None, None)
}

/// Preflight pass: [`run_answer_ledger_maintenance`] plus the optional tsift
/// recall notes (`#decisiontsiftrecall`). Only preflight carries the notes
/// into a cycle contract, so only preflight pays for the search.
pub fn run_answer_ledger_maintenance_with_recall(file: &Path) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_recall_env(file, &RecallEnv::from_process())
}

/// [`run_answer_ledger_maintenance_with_recall`] with an explicit environment.
pub fn run_answer_ledger_maintenance_with_recall_env(
    file: &Path,
    env: &RecallEnv,
) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_options(file, false, None, Some(env))
}

/// Force-disk counterpart for an explicitly force-disk strict closeout.
pub fn run_answer_ledger_maintenance_force_disk(
    file: &Path,
    write_effects: &dyn super::PreflightMaintenanceWriteEffects,
) -> Result<AnswerLedgerReport> {
    run_answer_ledger_maintenance_with_options(file, true, Some(write_effects), None)
}

fn run_answer_ledger_maintenance_with_options(
    file: &Path,
    force_disk: bool,
    write_effects: Option<&dyn super::PreflightMaintenanceWriteEffects>,
    recall: Option<&RecallEnv>,
) -> Result<AnswerLedgerReport> {
    let source = "preflight_answer_ledger";
    let Some(content) = read_authoritative(file, force_disk)? else {
        return Ok(AnswerLedgerReport::default());
    };
    let now = agent_doc_log_time::current_epoch_secs();
    let (observations, ledger) = build_ledger(file, &content);
    let closed = ledger.duplicate_closures(&observations);
    let mut report_ledger = ledger;
    let mut final_content = content.clone();
    if !closed.is_empty()
        && let Some(target) = agent_doc_answer_ledger::apply_duplicate_closures(&content, &closed)?
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
        let (_, rebuilt) = build_ledger(file, &persisted);
        report_ledger = rebuilt;
        final_content = persisted;
    }
    store_index(file, &report_ledger, now);
    let mut recalled = Vec::new();
    if let Some(source) = recall.and_then(|env| TsiftRecall::discover(file, env)) {
        let root = source.root().to_path_buf();
        let (ledger, notes) = recall_pass(file, &final_content, report_ledger, &source, &root);
        report_ledger = ledger;
        recalled = notes;
    }
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
        recalled,
    })
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

    /// `#decisiontsiftrecall` fixture: a project with a tsift index marker,
    /// this document (an answered request copy plus an open, unanswered
    /// redirect question) and another session document that answered the
    /// redirect question.
    fn recall_project() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        std::fs::create_dir_all(dir.path().join(".tsift")).unwrap();
        std::fs::write(dir.path().join(".tsift/index.db"), b"").unwrap();
        std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
        let doc = dir.path().join("tasks/session.md");
        let content = DOC.replace(
            "<!-- /agent:todo -->\n",
            "- [ ] [#redir] Choose the demo sign-in redirect port for local runs\n<!-- /agent:todo -->\n",
        );
        std::fs::write(&doc, content).unwrap();
        let other = dir.path().join("tasks/other.md");
        std::fs::write(
            &other,
            concat!(
                "<!-- agent:todo -->\n",
                "- [x] [#portpick] Which port should the demo sign-in redirect use for local runs\n",
                "<!-- /agent:todo -->\n",
                "<!-- agent:queue -->\n",
                "- ~~[#portpick]: Use port 8443.~~\n",
                "<!-- /agent:queue -->\n",
            ),
        )
        .unwrap();
        (dir, doc, other)
    }

    /// A fake `tsift` that prints one search envelope hitting `other` line 2,
    /// after `delay` seconds.
    fn fake_tsift(dir: &Path, other: &Path, delay: &str) -> std::path::PathBuf {
        use std::os::unix::fs::PermissionsExt;
        let bin_dir = dir.join("fakebin");
        std::fs::create_dir_all(&bin_dir).unwrap();
        let script = bin_dir.join("tsift");
        let report = serde_json::json!({
            "tool": "search",
            "report": {"ranked": [
                {"path": other.to_string_lossy(), "line": 2, "score": 7.5, "source": "vector_chunk"}
            ]}
        });
        std::fs::write(
            &script,
            format!("#!/bin/sh\nsleep {delay}\ncat <<'JSON'\n{report}\nJSON\n"),
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        bin_dir
    }

    fn recall_env(path: &Path, timeout_ms: u64) -> RecallEnv {
        RecallEnv {
            disabled: false,
            tsift_bin: None,
            path: Some(path.as_os_str().to_owned()),
            timeout_ms: Some(timeout_ms),
        }
    }

    /// No tsift on PATH: recall is not discovered and the pass is exactly the
    /// in-process one (tiers 1-2 act, no semantic or cross-document notes).
    #[test]
    fn recall_without_tsift_on_path_degrades_to_the_in_process_tiers() {
        let (dir, doc, _) = recall_project();
        let empty_path = dir.path().join("empty-path");
        std::fs::create_dir_all(&empty_path).unwrap();
        let env = recall_env(&empty_path, 2000);
        assert!(TsiftRecall::discover(&doc, &env).is_none());

        let report =
            run_answer_ledger_maintenance_with_options(&doc, true, Some(&NoEffects), Some(&env))
                .unwrap();
        // The acting tiers still close the answered request copy.
        assert_eq!(report.closed.len(), 1, "{report:?}");
        assert!(report.recalled.is_empty(), "{report:?}");
        assert!(
            report
                .possible
                .iter()
                .all(|candidate| candidate.tier != agent_doc_answer_ledger::MatchTier::Semantic)
        );
        let guidance = report.guidance().unwrap_or_default();
        assert!(!guidance.contains("#decisiontsiftrecall"), "{guidance}");
    }

    /// A project without a tsift index never looks for the executable.
    #[test]
    fn recall_needs_a_tsift_index() {
        let (dir, doc, other) = recall_project();
        std::fs::remove_file(dir.path().join(".tsift/index.db")).unwrap();
        let bin_dir = fake_tsift(dir.path(), &other, "0");
        assert!(TsiftRecall::discover(&doc, &recall_env(&bin_dir, 2000)).is_none());
        let disabled = RecallEnv {
            disabled: true,
            ..recall_env(&bin_dir, 2000)
        };
        std::fs::write(dir.path().join(".tsift/index.db"), b"").unwrap();
        assert!(TsiftRecall::discover(&doc, &disabled).is_none());
    }

    /// With tsift present, an answer in another document becomes a
    /// "possibly answered elsewhere" note; nothing closes because of it.
    #[test]
    fn recall_with_tsift_notes_answers_from_other_documents() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "0");
        let env = recall_env(&bin_dir, 10_000);
        assert!(TsiftRecall::discover(&doc, &env).is_some());

        let report =
            run_answer_ledger_maintenance_with_options(&doc, true, Some(&NoEffects), Some(&env))
                .unwrap();
        assert_eq!(report.closed.len(), 1, "{report:?}");
        // The fake answers every open question with the same hit.
        let note = report
            .recalled
            .iter()
            .find(|note| note.open_items == ["redir"])
            .unwrap_or_else(|| panic!("{report:?}"));
        assert_eq!(note.document, "tasks/other.md");
        assert_eq!(note.answer, "Use port 8443.");
        assert_eq!(note.tier, agent_doc_answer_ledger::MatchTier::Semantic);
        let written = std::fs::read_to_string(&doc).unwrap();
        assert!(written.contains("- [ ] [#redir]"), "{written}");
        let guidance = report.guidance().unwrap();
        assert!(
            guidance.contains("possibly answered elsewhere"),
            "{guidance}"
        );
        assert!(guidance.contains("Use port 8443."), "{guidance}");
    }

    /// A tsift that outlives the deadline is killed; the pass returns
    /// without notes well before the fake would have answered.
    #[test]
    fn recall_is_bounded_by_its_deadline() {
        let (dir, doc, other) = recall_project();
        let bin_dir = fake_tsift(dir.path(), &other, "30");
        let env = recall_env(&bin_dir, 300);
        let started = std::time::Instant::now();
        let report =
            run_answer_ledger_maintenance_with_options(&doc, true, Some(&NoEffects), Some(&env))
                .unwrap();
        assert!(
            started.elapsed() < std::time::Duration::from_secs(10),
            "{:?}",
            started.elapsed()
        );
        assert!(report.recalled.is_empty(), "{report:?}");
        assert_eq!(report.closed.len(), 1, "{report:?}");
    }

    /// Agent-free path: a plain force-disk maintenance pass (the closeout
    /// path) closes the stale request copy, keeps the work todo open, and the
    /// facts carry the answer; no agent command is involved.
    #[test]
    fn maintenance_closes_the_answered_copy_and_reports_facts() {
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
