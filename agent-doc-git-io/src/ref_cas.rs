//! Lazily-owned Git ref CAS (`#implementlazilyowned`).
//!
//! `git update-ref <ref> <new> <expected>` is the atomic comparator of the Git
//! authority backend. [`GitUpdateRef`] is the narrow `lazily::CompareAndApply`
//! adapter around it: `observe` reads the ref's current object id without side
//! effects, and `compare_and_apply` executes exactly one `update-ref`. A refused
//! comparison whose ref moved is reported to Lazily as `Superseded { observed }`;
//! a transient ref or index lock with the authority unchanged is `Retryable`;
//! anything else is `Rejected`. The adapter never retries and never chooses a
//! newer commit. Commit-object construction is preparation, not application:
//! an unreferenced commit object is inert until this adapter moves the ref.
//!
//! `lazily::EffectCasCore` owns each update's identity, expected authority,
//! single-flight claim, retry eligibility (on a logical clock), and supersession:
//!
//! - [`compare_and_update_ref`] runs one desired epoch for a commit the caller
//!   already built over `expected`.
//! - [`drive_ref_update`] serves producers that still want their change after a
//!   supersession: a superseded epoch re-desires a commit rebuilt over the
//!   observed authority as the next epoch, and a retryable lock is re-claimed
//!   from Lazily's own retained pending intent. Both are bounded by
//!   `max_attempts`; nothing outside the Lazily core decides what to retry.
//! - [`drive_commit_transaction`] drives a whole exact-path commit transaction
//!   (staging + commit object + HEAD CAS): a moved HEAD is `Superseded` and the
//!   still-wanted commit is re-desired over the observed HEAD; a held lock is
//!   `Retryable`. It replaces the caller-side `RetryableHeadMoved` /
//!   `RetryableIndexLock` loops of `agent-doc-commit-io`.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use agent_doc_git::{output_has_index_lock_contention, render_git_process_output};

use crate::transaction::CommitTransactionError;
use lazily::{
    CompareAndApply, EffectApply, EffectAttempt, EffectCasApplied, EffectCasClaim, EffectCasCore,
    EffectCasFailure, EffectCasOutcome, EffectCasUpsert, EffectIdentity, EffectIntent, EffectPlan,
};

/// The authority of an unborn ref: `update-ref` creates the ref only when it
/// does not exist yet.
pub const ZERO_OID: &str = "0000000000000000000000000000000000000000";

/// Phase label of the ref comparator in `CommitTransactionError` diagnostics.
pub const UPDATE_REF_PHASE: &str = "update-ref";

/// The single actor generation of these in-process cores: every call owns its
/// own core, so they never reconnect.
const GENERATION: u64 = 0;

/// Which ref of which repository an effect moves.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct GitRefKey {
    pub repo: PathBuf,
    pub reference: String,
}

impl GitRefKey {
    pub fn new(repo: &Path, reference: &str) -> Self {
        Self {
            repo: repo.to_path_buf(),
            reference: reference.to_string(),
        }
    }
}

/// Payload of one ref effect: the object id the ref should point at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRefTarget {
    pub new: String,
}

/// Receipt of an applied ref CAS.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRefReceipt {
    pub previous: String,
    pub new: String,
    pub output: Output,
}

/// Why a ref CAS failed with the authority unchanged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GitRefFailureKind {
    /// The ref's own lock file was held by a concurrent writer.
    RefLock,
    /// The repository index lock was held by a concurrent writer.
    IndexLock,
    /// A non-transient Git failure.
    Fatal,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitRefFailure {
    pub kind: GitRefFailureKind,
    pub detail: String,
}

impl GitRefFailure {
    fn fatal(detail: impl Into<String>) -> Self {
        Self {
            kind: GitRefFailureKind::Fatal,
            detail: detail.into(),
        }
    }
}

impl std::fmt::Display for GitRefFailure {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(&self.detail)
    }
}

type GitRefCore = EffectCasCore<GitRefKey, String, GitRefTarget, GitRefReceipt, GitRefFailure>;

/// Typed result of one Lazily-owned ref epoch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRefCas {
    Applied(GitRefReceipt),
    /// The ref moved: Lazily recorded the epoch as superseded and the ref was
    /// not written. `detail` is Git's refusal text.
    Superseded {
        expected: String,
        observed: String,
        detail: String,
    },
    /// A transient lock refused the update while the ref stayed at `expected`.
    Retryable(GitRefFailure),
    Rejected(GitRefFailure),
}

impl GitRefCas {
    /// Git's refusal text (or the receipt's output) for diagnostics.
    pub fn detail(&self) -> String {
        match self {
            Self::Applied(receipt) => render_git_process_output(&receipt.output),
            Self::Superseded {
                expected,
                observed,
                detail,
            } => format!("{detail} (expected {expected}, observed {observed})"),
            Self::Retryable(failure) | Self::Rejected(failure) => failure.detail.clone(),
        }
    }
}

/// Effect-free observation of a ref's current object id ([`ZERO_OID`] when the
/// ref does not exist yet).
pub fn observe_ref(key: &GitRefKey) -> Result<String, GitRefFailure> {
    let output = Command::new("git")
        .current_dir(&key.repo)
        .args(["rev-parse", "--verify", "--quiet", &key.reference])
        .output()
        .map_err(|error| GitRefFailure::fatal(format!("invoke git rev-parse: {error}")))?;
    match output.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&output.stdout).trim().to_string()),
        Some(1) if output.stdout.is_empty() => Ok(ZERO_OID.to_string()),
        _ => Err(GitRefFailure::fatal(format!(
            "git rev-parse {} failed: {}",
            key.reference,
            render_git_process_output(&output)
        ))),
    }
}

/// `lazily::CompareAndApply` adapter over `git update-ref`.
#[derive(Debug, Default)]
pub struct GitUpdateRef {
    refusal: RefCell<Option<String>>,
}

impl GitUpdateRef {
    fn take_refusal(&self) -> String {
        self.refusal.borrow_mut().take().unwrap_or_default()
    }
}

impl CompareAndApply<GitRefKey, String, GitRefTarget, GitRefReceipt> for GitUpdateRef {
    type Error = GitRefFailure;

    fn observe(&self, key: &GitRefKey) -> Result<String, GitRefFailure> {
        observe_ref(key)
    }

    fn compare_and_apply(
        &self,
        attempt: &EffectAttempt<GitRefKey, String, GitRefTarget, GitRefReceipt>,
    ) -> EffectApply<GitRefReceipt, String, GitRefFailure> {
        let output = match Command::new("git")
            .current_dir(&attempt.key.repo)
            .args([
                "update-ref",
                &attempt.key.reference,
                &attempt.payload.new,
                &attempt.expected,
            ])
            .output()
        {
            Ok(output) => output,
            Err(error) => {
                return EffectApply::Rejected(GitRefFailure::fatal(format!(
                    "invoke git {UPDATE_REF_PHASE}: {error}"
                )));
            }
        };
        if output.status.success() {
            return EffectApply::Applied {
                receipt: GitRefReceipt {
                    previous: attempt.expected.clone(),
                    new: attempt.payload.new.clone(),
                    output,
                },
                authority: attempt.payload.new.clone(),
            };
        }
        let detail = render_git_process_output(&output);
        *self.refusal.borrow_mut() = Some(detail.clone());
        match observe_ref(&attempt.key) {
            Ok(observed) if observed != attempt.expected => EffectApply::Superseded { observed },
            Ok(_) if detail.contains("cannot lock ref") || detail.contains("expected") => {
                EffectApply::Retryable(GitRefFailure {
                    kind: GitRefFailureKind::RefLock,
                    detail,
                })
            }
            Ok(_) if output_has_index_lock_contention(&output) => {
                EffectApply::Retryable(GitRefFailure {
                    kind: GitRefFailureKind::IndexLock,
                    detail,
                })
            }
            Ok(_) => EffectApply::Rejected(GitRefFailure::fatal(detail)),
            Err(observe_error) => EffectApply::Rejected(GitRefFailure::fatal(format!(
                "{detail}; observing {} after the refusal failed: {observe_error}",
                attempt.key.reference
            ))),
        }
    }
}

fn desire(
    core: &mut GitRefCore,
    key: &GitRefKey,
    epoch: u64,
    expected: String,
    new: String,
) -> anyhow::Result<()> {
    let intent = EffectIntent {
        identity: EffectIdentity::new(format!(
            "git-update-ref:{}:{}:{new}#{epoch}",
            key.repo.display(),
            key.reference
        ))
        .map_err(|error| anyhow::anyhow!("invalid git ref effect identity: {error:?}"))?,
        epoch,
        key: key.clone(),
        expected,
        plan: EffectPlan::single(GitRefTarget { new }),
    };
    match core.upsert_desired(intent).1 {
        EffectCasUpsert::Accepted => Ok(()),
        other => anyhow::bail!("git ref effect epoch {epoch} was not accepted: {other:?}"),
    }
}

/// Claim the next Lazily attempt for `key` at logical time `now`, run the
/// adapter once, and record its outcome (a retry becomes eligible at `now + 1`).
fn apply_claimed(
    core: &mut GitRefCore,
    adapter: &GitUpdateRef,
    key: &GitRefKey,
    now: u64,
    before_apply: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<GitRefCas> {
    let attempt = match core.claim(key, GENERATION, now).1 {
        EffectCasClaim::Claimed(attempt) => attempt,
        other => anyhow::bail!("git ref effect was not claimed: {other:?}"),
    };
    if let Err(error) = before_apply() {
        // The authority guard refused: the ref is untouched, and the claimed
        // attempt ends as a Lazily rejection rather than an orphaned claim.
        let rendered = format!("{error:#}");
        core.record_rejected(key, &attempt.identity, GitRefFailure::fatal(rendered));
        return Err(error);
    }
    let expected = attempt.expected.clone();
    let outcome = adapter.compare_and_apply(&attempt);
    let applied = match &outcome {
        EffectApply::Applied { receipt, .. } => Some(receipt.clone()),
        _ => None,
    };
    let observed = match &outcome {
        EffectApply::Superseded { observed } => Some(observed.clone()),
        _ => None,
    };
    let failure = match &outcome {
        EffectApply::Retryable(failure) | EffectApply::Rejected(failure) => Some(failure.clone()),
        _ => None,
    };
    let (_, recorded) = core.record_outcome(key, &attempt.identity, outcome, now + 1);
    Ok(match (recorded, applied, observed, failure) {
        (EffectCasOutcome::Applied(EffectCasApplied::Completed { .. }), Some(receipt), _, _) => {
            GitRefCas::Applied(receipt)
        }
        (EffectCasOutcome::Failure(EffectCasFailure::Superseded), _, Some(observed), _) => {
            GitRefCas::Superseded {
                expected,
                observed,
                detail: adapter.take_refusal(),
            }
        }
        (EffectCasOutcome::Failure(EffectCasFailure::Retrying { .. }), _, _, Some(failure)) => {
            GitRefCas::Retryable(failure)
        }
        (EffectCasOutcome::Failure(EffectCasFailure::Rejected), _, _, Some(failure)) => {
            GitRefCas::Rejected(failure)
        }
        (recorded, ..) => {
            anyhow::bail!("git ref effect outcome diverged from Lazily: {recorded:?}")
        }
    })
}

/// Move `reference` in `repo` from `expected` to `new` as one Lazily-owned
/// effect epoch. `before_apply` is the caller's authority guard, run after the
/// Lazily claim and immediately before the atomic comparator.
pub fn compare_and_update_ref(
    repo: &Path,
    reference: &str,
    new: &str,
    expected: &str,
    before_apply: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<GitRefCas> {
    let key = GitRefKey::new(repo, reference);
    let adapter = GitUpdateRef::default();
    let mut core = GitRefCore::new(GENERATION);
    desire(&mut core, &key, 1, expected.to_string(), new.to_string())?;
    apply_claimed(&mut core, &adapter, &key, 0, before_apply)
}

/// Terminal result of [`drive_ref_update`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitRefDrive {
    Applied(GitRefReceipt),
    /// The producer had nothing to apply over `authority`.
    Unchanged {
        authority: String,
    },
    /// Every attempt was superseded or retryable; the last refusal is kept.
    Exhausted {
        attempts: u32,
        last: GitRefCas,
    },
}

/// Drive a producer's ref change through Lazily until it applies.
///
/// `prepare(expected)` builds the commit to publish over `expected` (or
/// `None` when nothing changes); it is preparation only and never moves a ref.
/// A superseded epoch re-desires `prepare(observed)` as the next epoch; a
/// retryable lock re-claims Lazily's retained intent. `before_apply` guards
/// every comparator call. A rejection or guard failure is returned as an error.
pub fn drive_ref_update(
    repo: &Path,
    reference: &str,
    max_attempts: u32,
    prepare: &mut dyn FnMut(&str) -> anyhow::Result<Option<String>>,
    before_apply: &dyn Fn() -> anyhow::Result<()>,
) -> anyhow::Result<GitRefDrive> {
    let key = GitRefKey::new(repo, reference);
    let adapter = GitUpdateRef::default();
    let mut core = GitRefCore::new(GENERATION);
    let mut expected = adapter
        .observe(&key)
        .map_err(|error| anyhow::anyhow!("{error}"))?;
    let mut epoch = 1;
    let Some(new) = prepare(&expected)? else {
        return Ok(GitRefDrive::Unchanged {
            authority: expected,
        });
    };
    desire(&mut core, &key, epoch, expected.clone(), new)?;
    let mut now = 0;
    let max_attempts = max_attempts.max(1);
    for attempt in 1..=max_attempts {
        match apply_claimed(&mut core, &adapter, &key, now, before_apply)? {
            GitRefCas::Applied(receipt) => return Ok(GitRefDrive::Applied(receipt)),
            GitRefCas::Rejected(failure) => {
                anyhow::bail!("git {UPDATE_REF_PHASE} failed: {failure}")
            }
            last if attempt == max_attempts => {
                return Ok(GitRefDrive::Exhausted {
                    attempts: max_attempts,
                    last,
                });
            }
            GitRefCas::Superseded { observed, .. } => {
                epoch += 1;
                expected = observed;
                let Some(new) = prepare(&expected)? else {
                    return Ok(GitRefDrive::Unchanged {
                        authority: expected,
                    });
                };
                desire(&mut core, &key, epoch, expected.clone(), new)?;
            }
            GitRefCas::Retryable(_) => now += 1,
        }
    }
    unreachable!("the final attempt always returns")
}

/// Total attempts of one exact-path commit transaction driven by
/// [`drive_commit_transaction`]: the first attempt plus three Lazily-owned
/// retries (the bound the caller-side loops used).
pub const COMMIT_TRANSACTION_MAX_ATTEMPTS: u32 = 4;

/// Payload of one commit-transaction epoch: which epoch of the producer's
/// still-wanted commit it is (the commit object is rebuilt over the
/// epoch's expected HEAD by the transaction itself).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitTransactionTarget {
    pub epoch: u64,
}

type CommitTransactionCore =
    EffectCasCore<GitRefKey, String, CommitTransactionTarget, Output, String>;

/// `lazily::CompareAndApply` adapter over one exact-path commit transaction.
///
/// `transact` is one `stage_and_commit_exact_paths_once*` call: it builds the
/// commit over the current HEAD and moves HEAD through [`compare_and_update_ref`],
/// so the adapter performs no comparison of its own. Its outcome maps onto
/// Lazily: a moved HEAD is `Superseded { observed }`, a held index/ref lock is
/// `Retryable`, an ignored path or fatal failure is `Rejected`. The typed
/// `CommitTransactionError` is kept beside the core for the caller.
struct CommitTransaction<'a> {
    key: GitRefKey,
    transact: RefCell<&'a mut dyn FnMut() -> Result<Output, CommitTransactionError>>,
    error: RefCell<Option<CommitTransactionError>>,
}

impl CompareAndApply<GitRefKey, String, CommitTransactionTarget, Output> for CommitTransaction<'_> {
    type Error = String;

    fn observe(&self, key: &GitRefKey) -> Result<String, String> {
        observe_ref(key).map_err(|error| error.detail)
    }

    fn compare_and_apply(
        &self,
        _attempt: &EffectAttempt<GitRefKey, String, CommitTransactionTarget, Output>,
    ) -> EffectApply<Output, String, String> {
        let outcome = (self.transact.borrow_mut())();
        match outcome {
            Ok(output) => match observe_ref(&self.key) {
                Ok(authority) => EffectApply::Applied {
                    receipt: output,
                    authority,
                },
                Err(error) => {
                    let detail = error.detail.clone();
                    *self.error.borrow_mut() = Some(CommitTransactionError::Fatal(
                        anyhow::anyhow!("observe HEAD after a landed commit: {detail}"),
                    ));
                    EffectApply::Rejected(detail)
                }
            },
            Err(error) => {
                let rendered = commit_transaction_error_detail(&error);
                let apply = match &error {
                    CommitTransactionError::RetryableHeadMoved { .. } => {
                        match observe_ref(&self.key) {
                            Ok(observed) => EffectApply::Superseded { observed },
                            Err(observe_error) => EffectApply::Rejected(observe_error.detail),
                        }
                    }
                    CommitTransactionError::RetryableIndexLock { .. } => {
                        EffectApply::Retryable(rendered)
                    }
                    CommitTransactionError::IgnoredPath { .. }
                    | CommitTransactionError::Fatal(_) => EffectApply::Rejected(rendered),
                };
                *self.error.borrow_mut() = Some(error);
                apply
            }
        }
    }
}

fn commit_transaction_error_detail(error: &CommitTransactionError) -> String {
    match error {
        CommitTransactionError::RetryableIndexLock { phase, detail } => {
            format!("{phase}: {detail}")
        }
        CommitTransactionError::RetryableHeadMoved { detail } => detail.clone(),
        CommitTransactionError::IgnoredPath { path } => format!("ignored path {path}"),
        CommitTransactionError::Fatal(error) => format!("{error:#}"),
    }
}

/// Drive one exact-path commit transaction through Lazily until it lands.
///
/// Lazily owns the transaction's identity, expected HEAD, single-flight claim,
/// retry eligibility, and supersession: a moved HEAD is recorded as
/// `Superseded` and the still-wanted commit is re-desired over the observed
/// HEAD as the next epoch; a held lock is `Retryable` and re-claimed from
/// Lazily's retained intent. `on_retry(retry, error)` runs before each retry
/// (diagnostics and the physical backoff delay); it never decides whether to
/// retry. After `max_attempts` the last retryable error is returned; an
/// ignored path or fatal error is returned at once.
pub fn drive_commit_transaction(
    git_root: &Path,
    max_attempts: u32,
    transact: &mut dyn FnMut() -> Result<Output, CommitTransactionError>,
    on_retry: &mut dyn FnMut(u32, &CommitTransactionError),
) -> Result<Output, CommitTransactionError> {
    let key = GitRefKey::new(git_root, "HEAD");
    let adapter = CommitTransaction {
        key: key.clone(),
        transact: RefCell::new(transact),
        error: RefCell::new(None),
    };
    let fatal = |error: anyhow::Error| CommitTransactionError::Fatal(error);
    let mut core = CommitTransactionCore::new(GENERATION);
    let mut expected = adapter
        .observe(&key)
        .map_err(|detail| fatal(anyhow::anyhow!("observe HEAD before commit: {detail}")))?;
    let mut epoch = 1;
    let desire = |core: &mut CommitTransactionCore, epoch: u64, expected: String| {
        let intent = EffectIntent {
            identity: EffectIdentity::new(format!(
                "git-commit-transaction:{}#{epoch}",
                git_root.display()
            ))
            .map_err(|error| fatal(anyhow::anyhow!("invalid commit effect identity: {error:?}")))?,
            epoch,
            key: key.clone(),
            expected,
            plan: EffectPlan::single(CommitTransactionTarget { epoch }),
        };
        match core.upsert_desired(intent).1 {
            EffectCasUpsert::Accepted => Ok(()),
            other => Err(fatal(anyhow::anyhow!(
                "commit transaction epoch {epoch} was not accepted: {other:?}"
            ))),
        }
    };
    desire(&mut core, epoch, expected.clone())?;
    let mut now = 0;
    let max_attempts = max_attempts.max(1);
    for attempt in 1..=max_attempts {
        let claimed = match core.claim(&key, GENERATION, now).1 {
            EffectCasClaim::Claimed(claimed) => claimed,
            other => {
                return Err(fatal(anyhow::anyhow!(
                    "commit transaction was not claimed: {other:?}"
                )));
            }
        };
        let outcome = adapter.compare_and_apply(&claimed);
        let applied = match &outcome {
            EffectApply::Applied { receipt, .. } => Some(receipt.clone()),
            _ => None,
        };
        let observed = match &outcome {
            EffectApply::Superseded { observed } => Some(observed.clone()),
            _ => None,
        };
        let recorded = core
            .record_outcome(&key, &claimed.identity, outcome, now + 1)
            .1;
        let error = adapter.error.borrow_mut().take();
        match (recorded, applied, observed, error) {
            (EffectCasOutcome::Applied(EffectCasApplied::Completed { .. }), Some(output), ..) => {
                return Ok(output);
            }
            (EffectCasOutcome::Failure(EffectCasFailure::Rejected), _, _, Some(error)) => {
                return Err(error);
            }
            (_, _, _, Some(error)) if attempt == max_attempts => return Err(error),
            (
                EffectCasOutcome::Failure(EffectCasFailure::Superseded),
                _,
                Some(observed),
                Some(error),
            ) => {
                on_retry(attempt, &error);
                epoch += 1;
                expected = observed;
                desire(&mut core, epoch, expected.clone())?;
            }
            (EffectCasOutcome::Failure(EffectCasFailure::Retrying { .. }), _, _, Some(error)) => {
                on_retry(attempt, &error);
                now += 1;
            }
            (recorded, ..) => {
                return Err(fatal(anyhow::anyhow!(
                    "commit transaction outcome diverged from Lazily: {recorded:?}"
                )));
            }
        }
    }
    unreachable!("the final attempt always returns")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    fn git(repo: &Path, args: &[&str]) -> String {
        let output = Command::new("git")
            .current_dir(repo)
            .args(args)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            render_git_process_output(&output)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    }

    fn init_repo(path: &Path) {
        std::fs::create_dir_all(path).unwrap();
        git(path, &["init", "-q", "--initial-branch=main"]);
        git(path, &["config", "user.email", "test@example.com"]);
        git(path, &["config", "user.name", "Test"]);
    }

    fn commit_file(repo: &Path, name: &str, body: &str) -> String {
        std::fs::write(repo.join(name), body).unwrap();
        git(repo, &["add", name]);
        git(repo, &["commit", "-q", "-m", name]);
        git(repo, &["rev-parse", "HEAD"])
    }

    /// A commit object over `parent` that no ref points at yet.
    fn detached_commit(repo: &Path, parent: &str, message: &str) -> String {
        let tree = git(repo, &["rev-parse", &format!("{parent}^{{tree}}")]);
        git(repo, &["commit-tree", &tree, "-p", parent, "-m", message])
    }

    fn ref_files(repo: &Path) -> (String, Vec<u8>) {
        (
            git(repo, &["rev-parse", "HEAD"]),
            std::fs::read(repo.join(".git/refs/heads/main")).unwrap(),
        )
    }

    fn no_guard() -> anyhow::Result<()> {
        Ok(())
    }

    fn ok_output() -> Output {
        Command::new("git").arg("--version").output().unwrap()
    }

    /// A moved HEAD is Lazily supersession: the still-wanted commit is
    /// re-desired over the observed HEAD as the next epoch, and the retry
    /// callback paces it without deciding it.
    #[test]
    fn commit_transaction_head_moved_is_superseded_and_redesired_over_observed_head() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "a\n");
        let mut calls = 0;
        let mut heads = Vec::new();
        let mut retries = Vec::new();
        let output = drive_commit_transaction(
            &repo,
            COMMIT_TRANSACTION_MAX_ATTEMPTS,
            &mut || {
                calls += 1;
                heads.push(git(&repo, &["rev-parse", "HEAD"]));
                if calls == 1 {
                    // A concurrent writer moves HEAD under the first attempt.
                    commit_file(&repo, "b.txt", "b\n");
                    return Err(CommitTransactionError::RetryableHeadMoved {
                        detail: "HEAD moved".into(),
                    });
                }
                Ok(ok_output())
            },
            &mut |retry, error| retries.push((retry, format!("{error:?}"))),
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(calls, 2);
        assert_ne!(
            heads[0], heads[1],
            "the second epoch runs over the moved HEAD"
        );
        assert_eq!(retries.len(), 1);
        assert_eq!(retries[0].0, 1);
        assert!(retries[0].1.contains("RetryableHeadMoved"));
    }

    #[test]
    fn commit_transaction_index_lock_is_retryable_and_exhaustion_returns_the_last_error() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "a\n");
        let mut calls = 0;
        let mut retries = Vec::new();
        let error = drive_commit_transaction(
            &repo,
            COMMIT_TRANSACTION_MAX_ATTEMPTS,
            &mut || {
                calls += 1;
                Err(CommitTransactionError::RetryableIndexLock {
                    phase: "update-index",
                    detail: format!("index.lock held ({calls})"),
                })
            },
            &mut |retry, _| retries.push(retry),
        )
        .unwrap_err();
        assert_eq!(calls, COMMIT_TRANSACTION_MAX_ATTEMPTS as usize);
        assert_eq!(retries, vec![1, 2, 3]);
        match error {
            CommitTransactionError::RetryableIndexLock { phase, detail } => {
                assert_eq!(phase, "update-index");
                assert_eq!(detail, "index.lock held (4)");
            }
            other => panic!("expected the last index-lock error, got {other:?}"),
        }
    }

    #[test]
    fn commit_transaction_ignored_path_and_fatal_are_rejected_without_retry() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "a\n");
        for fatal in [false, true] {
            let mut calls = 0;
            let mut retried = false;
            let error = drive_commit_transaction(
                &repo,
                COMMIT_TRANSACTION_MAX_ATTEMPTS,
                &mut || {
                    calls += 1;
                    Err(if fatal {
                        CommitTransactionError::Fatal(anyhow::anyhow!("boom"))
                    } else {
                        CommitTransactionError::IgnoredPath {
                            path: "scratch.md".into(),
                        }
                    })
                },
                &mut |_, _| retried = true,
            )
            .unwrap_err();
            assert_eq!(calls, 1);
            assert!(!retried);
            match (fatal, error) {
                (false, CommitTransactionError::IgnoredPath { path }) => {
                    assert_eq!(path, "scratch.md")
                }
                (true, CommitTransactionError::Fatal(error)) => {
                    assert_eq!(error.to_string(), "boom")
                }
                (_, other) => panic!("unexpected {other:?}"),
            }
        }
    }

    /// The real exact-path transaction under the driver: a concurrent commit
    /// between attempts supersedes the first epoch, and the landed commit's
    /// parent is the concurrent writer's commit (no lost update).
    #[test]
    fn driven_exact_path_commit_lands_over_a_concurrent_head() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "doc.md", "v1\n");
        let doc = repo.join("doc.md");
        std::fs::write(&doc, "v2\n").unwrap();
        let mut first = true;
        let output = drive_commit_transaction(
            &repo,
            COMMIT_TRANSACTION_MAX_ATTEMPTS,
            &mut || {
                if std::mem::take(&mut first) {
                    let concurrent = commit_file(&repo, "other.txt", "x\n");
                    return Err(CommitTransactionError::RetryableHeadMoved {
                        detail: format!("HEAD moved to {concurrent}"),
                    });
                }
                crate::transaction::stage_and_commit_exact_paths_once(
                    &repo,
                    &doc,
                    Some("v2\n"),
                    &[],
                    "driven commit",
                )
            },
            &mut |_, _| {},
        )
        .unwrap();
        assert!(output.status.success());
        assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "driven commit");
        assert_eq!(
            git(&repo, &["log", "-1", "--format=%s", "HEAD^"]),
            "other.txt"
        );
        assert_eq!(git(&repo, &["show", "HEAD:doc.md"]), "v2");
    }

    #[test]
    fn applied_ref_cas_moves_the_ref_and_receipts_the_new_authority() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let base = commit_file(&repo, "a.txt", "a\n");
        let next = detached_commit(&repo, &base, "next");
        let result = compare_and_update_ref(&repo, "HEAD", &next, &base, &no_guard).unwrap();
        let GitRefCas::Applied(receipt) = result else {
            panic!("expected an applied ref CAS, got {result:?}");
        };
        assert_eq!(receipt.previous, base);
        assert_eq!(receipt.new, next);
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), next);
    }

    #[test]
    fn changed_authority_is_superseded_and_leaves_the_ref_byte_for_byte_unchanged() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let base = commit_file(&repo, "a.txt", "a\n");
        let ours = detached_commit(&repo, &base, "ours");
        // A concurrent writer wins the ref after we observed `base`.
        let winner = commit_file(&repo, "b.txt", "b\n");
        let before = ref_files(&repo);
        let result = compare_and_update_ref(&repo, "HEAD", &ours, &base, &no_guard).unwrap();
        match result {
            GitRefCas::Superseded {
                expected, observed, ..
            } => {
                assert_eq!(expected, base);
                assert_eq!(observed, winner);
            }
            other => panic!("expected Superseded, got {other:?}"),
        }
        assert_eq!(ref_files(&repo), before);
    }

    #[test]
    fn unborn_ref_is_created_only_against_the_zero_authority() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        assert_eq!(
            observe_ref(&GitRefKey::new(&repo, "HEAD")).unwrap(),
            ZERO_OID
        );
        let first = commit_file(&repo, "a.txt", "a\n");
        // Expecting the ref to be unborn after it was born is a supersession.
        let next = detached_commit(&repo, &first, "next");
        let result = compare_and_update_ref(&repo, "HEAD", &next, ZERO_OID, &no_guard).unwrap();
        assert!(
            matches!(&result, GitRefCas::Superseded { observed, .. } if observed == &first),
            "{result:?}"
        );
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), first);
    }

    #[test]
    fn held_ref_lock_with_unchanged_authority_is_retryable_not_superseded() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let base = commit_file(&repo, "a.txt", "a\n");
        let next = detached_commit(&repo, &base, "next");
        let lock = repo.join(".git/refs/heads/main.lock");
        std::fs::write(&lock, "").unwrap();
        let result = compare_and_update_ref(&repo, "HEAD", &next, &base, &no_guard).unwrap();
        assert!(
            matches!(
                &result,
                GitRefCas::Retryable(GitRefFailure {
                    kind: GitRefFailureKind::RefLock,
                    ..
                })
            ),
            "{result:?}"
        );
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), base);
        std::fs::remove_file(lock).unwrap();
    }

    #[test]
    fn guard_refusal_runs_after_the_claim_and_never_reaches_the_comparator() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let base = commit_file(&repo, "a.txt", "a\n");
        let next = detached_commit(&repo, &base, "next");
        let error =
            compare_and_update_ref(&repo, "HEAD", &next, &base, &|| anyhow::bail!("owner lost"))
                .unwrap_err();
        assert!(format!("{error:#}").contains("owner lost"));
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), base);
    }

    #[test]
    fn superseded_producer_re_desires_over_the_observed_authority_as_the_next_epoch() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let base = commit_file(&repo, "a.txt", "a\n");
        let prepared = RefCell::new(Vec::new());
        let raced = Cell::new(false);
        let mut prepare = |expected: &str| -> anyhow::Result<Option<String>> {
            prepared.borrow_mut().push(expected.to_string());
            Ok(Some(detached_commit(&repo, expected, "producer")))
        };
        // The first guard call lets a concurrent writer win the ref, so the
        // first epoch's comparator observes a moved authority.
        let guard = || -> anyhow::Result<()> {
            if !raced.replace(true) {
                commit_file(&repo, "b.txt", "b\n");
            }
            Ok(())
        };
        let result = drive_ref_update(&repo, "HEAD", 3, &mut prepare, &guard).unwrap();
        let GitRefDrive::Applied(receipt) = result else {
            panic!("expected the second epoch to apply, got {result:?}");
        };
        let prepared = prepared.into_inner();
        assert_eq!(prepared.len(), 2, "{prepared:?}");
        assert_eq!(prepared[0], base);
        assert_ne!(prepared[1], base, "epoch 2 must be rebuilt over the winner");
        assert_eq!(receipt.previous, prepared[1]);
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), receipt.new);
        assert_eq!(git(&repo, &["rev-parse", "HEAD^"]), prepared[1]);
    }

    #[test]
    fn exhausted_drive_leaves_the_winner_in_place() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        commit_file(&repo, "a.txt", "a\n");
        let writes = Cell::new(0);
        let mut prepare = |expected: &str| -> anyhow::Result<Option<String>> {
            Ok(Some(detached_commit(&repo, expected, "producer")))
        };
        // Every comparator call races a concurrent winner.
        let guard = || -> anyhow::Result<()> {
            writes.set(writes.get() + 1);
            commit_file(&repo, "b.txt", &format!("{}\n", writes.get()));
            Ok(())
        };
        let result = drive_ref_update(&repo, "HEAD", 3, &mut prepare, &guard).unwrap();
        assert!(
            matches!(
                &result,
                GitRefDrive::Exhausted {
                    attempts: 3,
                    last: GitRefCas::Superseded { .. }
                }
            ),
            "{result:?}"
        );
        assert_eq!(writes.get(), 3);
        assert_eq!(git(&repo, &["log", "-1", "--format=%s"]), "b.txt");
    }

    #[test]
    fn unchanged_producer_applies_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let repo = tmp.path().join("repo");
        init_repo(&repo);
        let base = commit_file(&repo, "a.txt", "a\n");
        let result = drive_ref_update(&repo, "HEAD", 3, &mut |_| Ok(None), &no_guard).unwrap();
        assert_eq!(
            GitRefDrive::Unchanged {
                authority: base.clone()
            },
            result
        );
        assert_eq!(git(&repo, &["rev-parse", "HEAD"]), base);
    }
}
