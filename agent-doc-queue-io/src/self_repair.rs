//! `#selfrepaircleanup`: [`agent_doc_queue::self_repair`] with attempt
//! liveness read from the claim ledger; the follow-up ledgers keyed on row
//! text follow each row the repair rewrites.

use std::path::Path;

use agent_doc_queue::reconcile_incremental::{
    ReconcileCache, ReconcileInputs, ReconcileReport, Reconciled, reconcile_full,
    reconcile_incremental,
};
use agent_doc_queue::self_repair::{SelfRepair, self_repair_document_with_claims};
use agent_doc_queue::stale_typing_copies::drop_stale_typing_copies_with;

/// The external answers the queue reconcile passes read for one document
/// revision (`#incrementalreconcile`): claim-ledger attempt liveness and row
/// claims, response proof from `agent:exchange` and the response index, and
/// struck provenance from the committed copies (read lazily: most revisions
/// have no row that needs it).
pub struct ReconcileInputSources<'a> {
    file: &'a Path,
    content: &'a str,
    /// `(owner, generation)` of every ledger claim; `None` when the ledger is
    /// unreadable.
    attempts: Option<Vec<(String, u64)>>,
    /// `(identity, collapsed text)` of every ledger claim; `None` when the
    /// ledger is unreadable.
    claims: Option<Vec<(agent_doc_element_queue::QueueItemIdentity, String)>>,
    originals: std::cell::OnceCell<std::collections::BTreeSet<String>>,
    committed: std::cell::OnceCell<Vec<String>>,
}

fn normalize_response_id(id: &str) -> String {
    id.trim()
        .trim_start_matches("[#")
        .trim_start_matches('#')
        .trim_end_matches(']')
        .to_ascii_lowercase()
}

impl<'a> ReconcileInputSources<'a> {
    /// Read the sources for `content`, the revision the passes run on.
    pub fn load(file: &'a Path, content: &'a str) -> Self {
        let ledger = crate::queue_claim::load_ledger(file).ok();
        let attempts = ledger.as_ref().map(|ledger| {
            ledger
                .claims
                .iter()
                .map(|claim| (claim.owner.clone(), claim.generation))
                .collect()
        });
        let claims = ledger.as_ref().map(|ledger| {
            ledger
                .claims
                .iter()
                .map(|claim| (claim.identity.clone(), collapse(&claim.item_text)))
                .collect()
        });
        Self {
            file,
            content,
            attempts,
            claims,
            originals: std::cell::OnceCell::new(),
            committed: std::cell::OnceCell::new(),
        }
    }

    /// A claim label is stale when the ledger no longer holds that exact
    /// `(owner, generation)` attempt; an unreadable ledger keeps every label.
    pub fn is_live(&self, owner: &str, generation: u64) -> bool {
        match &self.attempts {
            Some(attempts) => attempts
                .iter()
                .any(|(held, held_generation)| held == owner && *held_generation == generation),
            None => true,
        }
    }

    /// A misplaced response tree is retired only when its original is proven
    /// elsewhere: in this revision's `agent:exchange`, or the response index
    /// of the exchange archive.
    pub fn response_proven(&self, id: &str) -> bool {
        self.originals
            .get_or_init(|| {
                let mut originals: std::collections::BTreeSet<String> =
                    agent_doc_archive_io::indexed_response_ids(self.file)
                        .map(|ids| ids.iter().map(|id| normalize_response_id(id)).collect())
                        .unwrap_or_default();
                if let Ok(components) = agent_doc_element::element::parse(self.content) {
                    for exchange in components
                        .iter()
                        .filter(|component| component.name == "exchange")
                    {
                        originals.extend(
                            agent_doc_archive_io::response_ids_in_content(
                                exchange.content(self.content),
                            )
                            .iter()
                            .map(|id| normalize_response_id(id)),
                        );
                    }
                }
                originals
            })
            .contains(&normalize_response_id(id))
    }

    /// `#e107moved`: a column-0 row a torn merge put inside another row's
    /// response tree gets its strike back only when a committed copy (the
    /// sync baseline or git HEAD) holds it struck outside any response tree.
    pub fn struck_provenance(&self, work: &str) -> bool {
        self.committed
            .get_or_init(|| {
                [
                    agent_doc_snapshot_io::load_document_baseline(self.file)
                        .ok()
                        .flatten(),
                    agent_doc_git_io::revision::show_head(self.file)
                        .ok()
                        .flatten(),
                ]
                .into_iter()
                .flatten()
                .collect()
            })
            .iter()
            .any(|copy| agent_doc_queue::response_tree_rehome::top_level_row_struck_in(copy, work))
    }

    /// `#treefinish`: a row whose work identity, or whose text, any ledger
    /// claim holds is never struck; an unreadable ledger strikes nothing
    /// (every row is assumed claimed).
    pub fn is_claimed(&self, row: &str) -> bool {
        match &self.claims {
            None => true,
            Some(claims) if claims.is_empty() => false,
            Some(claims) => {
                let identity = agent_doc_queue::queue_claim::claim_identity(row);
                let work = collapse(
                    agent_doc_element_queue::struck_work_text(row)
                        .trim()
                        .trim_start_matches("### "),
                );
                // A nested row's claim keys its lineage (`parent: leaf`), so a
                // claim whose text contains the row's work text, or is
                // contained in it, holds the row too. Short texts never match
                // by text.
                let long = |text: &str| text.chars().count() >= 8;
                claims.iter().any(|(held, text)| {
                    *held == identity
                        || (long(&work)
                            && long(text)
                            && (text.contains(&work) || work.contains(text)))
                })
            }
        }
    }

    /// Run `f` with these sources as the chain's [`ReconcileInputs`].
    pub fn with_inputs<T>(&self, f: impl FnOnce(&ReconcileInputs<'_>) -> T) -> T {
        let is_live = |owner: &str, generation: u64| self.is_live(owner, generation);
        let response_proven = |id: &str| self.response_proven(id);
        let struck_provenance = |work: &str| self.struck_provenance(work);
        let is_claimed = |row: &str| self.is_claimed(row);
        f(&ReconcileInputs {
            is_live: &is_live,
            response_proven: &response_proven,
            struck_provenance: &struck_provenance,
            is_claimed: &is_claimed,
        })
    }
}

/// Carry the follow-up records keyed on row text to the rows a self-repair
/// rewrote: their root provenance and owner classification follow them to
/// the new text (the old key stays until its row is gone, so a write that
/// never lands loses nothing).
pub fn carry_self_repair_rewrites(file: &Path, repairs: &[SelfRepair]) {
    let rewrites: Vec<(String, String)> = repairs
        .iter()
        .filter_map(|repair| repair.rewritten.clone())
        .collect();
    crate::followup_provenance_ledger::carry_rewritten_rows(file, &rewrites);
    crate::followup_owner_ledger::carry_rewritten_rows(file, &rewrites);
}

/// Run every self-repair over `content`. A claim label is stale when the
/// ledger no longer holds that exact `(owner, generation)` attempt; an
/// unreadable ledger keeps every label (no claim is assumed retired). A
/// response tree copied under a response-structure row is retired only when
/// its id's original is in `agent:exchange` or the response index.
pub fn self_repair_document_for(file: &Path, content: &str) -> Option<(String, Vec<SelfRepair>)> {
    let sources = ReconcileInputSources::load(file, content);
    let repaired = self_repair_document_with_claims(
        content,
        &|owner: &str, generation: u64| sources.is_live(owner, generation),
        &|id: &str| sources.response_proven(id),
        &|work: &str| sources.struck_provenance(work),
        &|row: &str| sources.is_claimed(row),
    );
    let (document, mut repairs) = match repaired {
        Some((document, repairs)) => (document, repairs),
        None => (content.to_string(), Vec::new()),
    };
    let stale = drop_stale_typing_copies_with(
        &document,
        &mut |candidates| typing_history_for(file, candidates),
        &|row| sources.is_claimed(row),
    );
    let document = stale.document;
    repairs.extend(stale.repairs);
    carry_self_repair_rewrites(file, &repairs);
    (document != content).then_some((document, repairs))
}

/// Bound on each content-bearing fact stream [`typing_history_for`] reads.
const TYPING_HISTORY_EVENTS: usize = 16;

/// Bound on the committed versions [`typing_history_with_commits_for`] reads
/// (`#dupwrite2`).
const TYPING_HISTORY_COMMITS: usize = 128;

/// `#dupwrite`: the recorded versions of `file` that prove a row was an older
/// cut of another (oldest first): the committed copy, then the newest merge
/// baselines and retained-write cuts and targets from the state ledger. An
/// unreadable source contributes nothing, so a repair is only ever withheld.
pub fn typing_history_for(
    file: &Path,
    candidates: &[String],
) -> agent_doc_queue::stale_typing_copies::TypingHistory {
    typing_history_from(file, 1, candidates)
}

/// `#dupwrite2`: [`typing_history_for`] plus the newest
/// [`TYPING_HISTORY_COMMITS`] committed versions of `file`, for the explicit
/// `queue sync` repair. The state ledger keeps only its newest content
/// facts: measured 2026-11-24, an hour after the operator typed, every one of
/// them held the already-damaged document, while the committed copy from
/// mid-typing still proved a row was typed once.
pub fn typing_history_with_commits_for(
    file: &Path,
    candidates: &[String],
) -> agent_doc_queue::stale_typing_copies::TypingHistory {
    typing_history_from(file, TYPING_HISTORY_COMMITS, candidates)
}

/// Committed versions (at most `commits`) and recorded state-ledger
/// versions of `file`, ordered by time (oldest first), so "a later version"
/// in the proof is later in fact. Only versions that can bear on
/// `candidates` are parsed
/// ([`agent_doc_queue::stale_typing_copies::TypingHistory::from_relevant_documents`]).
fn typing_history_from(
    file: &Path,
    commits: usize,
    candidates: &[String],
) -> agent_doc_queue::stale_typing_copies::TypingHistory {
    let mut versions: Vec<(u64, String)> =
        agent_doc_git_io::revision::recent_file_revisions(file, commits)
            .unwrap_or_default()
            .into_iter()
            .map(|revision| (revision.committed_secs, revision.content))
            .collect();
    let recorded = || -> anyhow::Result<Vec<(u64, u64, String)>> {
        let Some(root) = agent_doc_fs::find_project_root(file) else {
            return Ok(Vec::new());
        };
        if !agent_doc_sqlite::state_store::state_db_path(&root).exists() {
            return Ok(Vec::new());
        }
        let hash = agent_doc_hash::path_hash(file)?;
        let conn = agent_doc_sqlite::state_store::open_state_db(&root)?;
        let mut recorded = Vec::new();
        for (fact_type, fields) in [
            ("document_baseline_checkpointed", &["content"][..]),
            (
                "document_write_deferred",
                &["expected_content", "target_content"][..],
            ),
        ] {
            for event in
                agent_doc_sqlite::state_store::load_recent_state_events_by_fact_type_from_db(
                    &conn,
                    &hash,
                    fact_type,
                    TYPING_HISTORY_EVENTS,
                )?
            {
                let Ok(payload) = serde_json::from_str::<serde_json::Value>(&event.payload_json)
                else {
                    continue;
                };
                for field in fields {
                    if let Some(content) = payload["fact"][*field].as_str() {
                        recorded.push((event.sequence, event.timestamp, content.to_string()));
                    }
                }
            }
        }
        recorded.sort_by_key(|(sequence, _, _)| *sequence);
        Ok(recorded)
    };
    versions.extend(
        recorded()
            .unwrap_or_default()
            .into_iter()
            .map(|(_, timestamp, content)| (timestamp, content)),
    );
    // Stable: committed copies before ledger facts of the same second, and
    // ledger facts in sequence order.
    versions.sort_by_key(|(timestamp, _)| *timestamp);
    let contents: Vec<String> = versions.into_iter().map(|(_, content)| content).collect();
    agent_doc_queue::stale_typing_copies::TypingHistory::from_relevant_documents(
        &contents, candidates,
    )
}

/// The response passes of `agent-doc queue sync`
/// ([`agent_doc_queue::reconcile_incremental::queue_sync_response_chain`])
/// with this document's ledgers and provenance, carrying self-repair
/// rewrites into the follow-up ledgers as [`self_repair_document_for`] does.
pub fn queue_sync_response_chain_for(
    file: &Path,
    content: &str,
) -> agent_doc_queue::reconcile_incremental::QueueSyncResponses {
    let sources = ReconcileInputSources::load(file, content);
    let mut chain = sources.with_inputs(|inputs| {
        agent_doc_queue::reconcile_incremental::queue_sync_response_chain(content, inputs)
    });
    // `#dupwrite`: proven stale copies of the operator's typed rows, with
    // the committed history as proof too (`#dupwrite2`).
    let stale = drop_stale_typing_copies_with(
        &chain.document,
        &mut |candidates| typing_history_with_commits_for(file, candidates),
        &|row| sources.is_claimed(row),
    );
    if !stale.unproven.is_empty() {
        agent_doc_ops_log_io::log_op(
            file,
            &format!(
                "stale_typing_copy_unproven file={} source=queue_sync count={} rows={:?} (#dupwrite2): no recorded version shows these rows typed once or typed on from, so they stay",
                file.display(),
                stale.unproven.len(),
                stale.unproven
            ),
        );
    }
    chain.document = stale.document;
    chain.repairs.extend(stale.repairs);
    carry_self_repair_rewrites(file, &chain.repairs);
    chain
}

/// `#treefinish`: [`agent_doc_queue::response_tree_sync::sync_response_trees_striking`]
/// with row claims read from the claim ledger. A row whose work identity, or
/// whose text, any ledger claim holds is never struck; an unreadable ledger
/// strikes nothing (every row is assumed claimed).
pub fn sync_response_trees_for(
    file: &Path,
    content: &str,
) -> Option<agent_doc_queue::response_tree_sync::ResponseTreeSync> {
    let sources = ReconcileInputSources::load(file, content);
    agent_doc_queue::response_tree_sync::sync_response_trees_striking(content, &|row: &str| {
        sources.is_claimed(row)
    })
}

/// `#incrementalreconcile`: the queue/closeout chain
/// ([`agent_doc_queue::reconcile_incremental`]) over `content` with this
/// document's external answers, reusing `cache` (one per document) across
/// revisions. Byte-identical to [`reconcile_chain_full_for`].
pub fn reconcile_chain_for(
    file: &Path,
    content: &str,
    cache: &mut ReconcileCache,
) -> ReconcileReport {
    let sources = ReconcileInputSources::load(file, content);
    sources.with_inputs(|inputs| reconcile_incremental(None, content, inputs, cache))
}

/// [`reconcile_chain_for`] from the revision's changeset: `changeset` turns
/// the revision `cache` reconciled last into `content`
/// ([`agent_doc_queue::reconcile_incremental::reconcile_incremental_changeset`]),
/// so the chain never compares through `agent:exchange` to find what changed.
pub fn reconcile_chain_changeset_for(
    file: &Path,
    changeset: &[agent_doc_queue::reconcile_incremental::TextSplice],
    content: &str,
    cache: &mut ReconcileCache,
) -> ReconcileReport {
    let sources = ReconcileInputSources::load(file, content);
    sources.with_inputs(|inputs| {
        agent_doc_queue::reconcile_incremental::reconcile_incremental_changeset(
            changeset, content, inputs, cache,
        )
    })
}

/// The reference [`reconcile_chain_for`] equals: every pass on the whole
/// document with no cache.
pub fn reconcile_chain_full_for(file: &Path, content: &str) -> Reconciled {
    let sources = ReconcileInputSources::load(file, content);
    sources.with_inputs(|inputs| reconcile_full(content, inputs))
}

fn collapse(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn git(repo: &Path, args: &[&str]) {
        let status = Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?} failed");
    }

    fn doc(run: &str) -> String {
        format!(
            concat!(
                "---\nagent_doc_format: template\n---\n\n",
                "## Run <!-- agent:run id=\"main\" subagents go -->\n",
                "{}",
                "<!-- /agent:run -->\n\n",
                "## Exchange <!-- agent:exchange -->\n<!-- /agent:exchange -->\n",
            ),
            run
        )
    }

    const PIPE: &str = "I do like the pipe operator for commands as well.";
    const SNAP: &str = "[Declare lang](https://example.invalid/declare/) looks interesting.";
    const FINISHED: &str = "[Declare lang](https://example.invalid/declare/) looks interesting. Is this a way to declare guis from agent-doc to present information?";

    /// `#dupwrite2`: the state ledger no longer reaches the typing (no
    /// project ledger here at all), but the document's committed history
    /// does: `queue sync`'s chain removes the pipe copies that a commit from
    /// mid-typing proves were one row, and keeps (and logs) the Declare-lang
    /// snapshot that nothing recorded ever showed replaced.
    #[test]
    fn dupwrite2_queue_sync_proves_copies_from_the_committed_history() {
        let dir = tempfile::tempdir().unwrap();
        let repo = dir.path();
        git(repo, &["init", "-b", "main"]);
        git(repo, &["config", "user.email", "dev@example.com"]);
        git(repo, &["config", "user.name", "Dev"]);
        let file = repo.join("session.md");
        fs::write(&file, doc("- I do like the pipe\n")).unwrap();
        git(repo, &["add", "session.md"]);
        git(repo, &["commit", "-m", "typing"]);
        let damaged = doc(&format!(
            concat!(
                "- ### 📥 {pipe} ⟦effect:1⟧ [#q-000000000001]\n",
                "---\n",
                "📥 {snap} [#q-000000000002]\n",
                "---\n",
                "- ### 📥 {pipe} ⟦effect:2⟧ [#q-000000000003]\n",
                "- ### 📥 {finished} [#q-000000000004]\n",
            ),
            pipe = PIPE,
            snap = SNAP,
            finished = FINISHED,
        ));
        fs::write(&file, &damaged).unwrap();
        git(repo, &["commit", "-am", "damaged"]);

        let chain = queue_sync_response_chain_for(&file, &damaged);
        assert_eq!(
            chain.document,
            doc(&format!(
                concat!(
                    "---\n",
                    "📥 {snap} [#q-000000000002]\n",
                    "---\n",
                    "- ### 📥 {pipe} ⟦effect:2⟧ [#q-000000000003]\n",
                    "- ### 📥 {finished} [#q-000000000004]\n",
                ),
                pipe = PIPE,
                snap = SNAP,
                finished = FINISHED,
            )),
            "{:?}",
            chain.repairs
        );
        assert_eq!(
            chain
                .repairs
                .iter()
                .filter(|repair| repair.kind
                    == agent_doc_queue::self_repair::SelfRepairKind::StaleTypingCopy)
                .count(),
            1
        );
        // Preflight's own pass reads only the committed HEAD and the ledger:
        // HEAD is the damaged copy, so it proves nothing.
        assert!(
            self_repair_document_for(&file, &damaged)
                .is_none_or(|(document, _)| document.contains("⟦effect:1⟧"))
        );
    }
}
