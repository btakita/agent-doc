//! Binary-owned CLI writes that the editor authority *retained*.
//!
//! `#binaryownedcli`: a standalone mutation command (`agent-doc backlog <FILE>
//! add`, `backlog remove`, `queue sync`, `queue consume --id`, ...) writes
//! through the same CRDT authority as a response. When an editor replica is
//! live but has not yet acknowledged the delivery, the post-write barrier
//! ([`EditorConvergenceEffects::guard_visible_delivery_convergence`]) refuses
//! with the narrow `retained=delivery_projection_pending` deferral. The write
//! itself is already accepted and durable: observed 2026-10-11 on
//! `agent-doc-bugs.md`, `queue_sync` deferred at `delivery_version=30` and the
//! next delivery edge (`delivery_version=31`, `delivery_converged=true`) arrived
//! within seconds — yet the command exited non-zero and printed the dogfood
//! `projection_failure` fix prompt, so agents treated a converging write as a
//! defect and retried (double-applying adds) or filed bugs.
//!
//! `agent-doc respond` already reports the same retention as a deferral. This
//! module gives every binary-owned CLI writer the same semantics from ONE place,
//! the write-convergence boundary they all pass through:
//!
//! 1. the refusal must carry the narrow pending token — every other refusal
//!    (no registered replica, sync pending, a rejecting endpoint) propagates
//!    unchanged and still fails;
//! 2. the command blocks on the controller's delivery-convergence edge (an
//!    event subscription, not a polling sleep) for a bounded wait;
//! 3. converged and the written components are visible → the write returns
//!    `Ok` and the command continues (so read-back verification such as
//!    `backlog add`'s `#addreadback` sees the item);
//! 4. still retained when the bound expires → [`RetainedCliWriteDeferred`], a
//!    typed deferral that `main` maps to [`RETAINED_CLI_WRITE_DEFERRED_EXIT_CODE`]
//!    with an informational message and no dogfood fix prompt.
//!
//! No step writes disk or forces anything; the only effect is waiting.

use anyhow::Result;
use std::cell::Cell;
use std::path::Path;
use std::time::Duration;

use crate::EditorConvergenceEffects;

/// Exit status of a binary-owned CLI write whose change is durable in the
/// editor authority but whose delivery projection had not converged when the
/// bounded wait expired. `EX_TEMPFAIL` (sysexits.h): the command did its work,
/// nothing must be retried, and the change becomes visible on its own.
pub const RETAINED_CLI_WRITE_DEFERRED_EXIT_CODE: u8 = 75;

/// Default bound on the convergence wait. The observed retained→converged
/// latency was 1–15s; the bound only decides between exit 0 and the deferred
/// status, never between success and failure.
pub const RETAINED_CLI_WRITE_DEFAULT_WAIT: Duration = Duration::from_secs(30);

/// Environment override for [`RETAINED_CLI_WRITE_DEFAULT_WAIT`], in milliseconds.
pub const RETAINED_CLI_WRITE_WAIT_ENV: &str = "AGENT_DOC_RETAINED_CLI_WRITE_WAIT_MS";

/// Stable token stamped on the deferral message so harness hooks and scripts
/// that only see text can tell a deferral from a failure.
pub use agent_doc_turn::write_ownership::RETAINED_CLI_WRITE_DEFERRED_TOKEN;

thread_local! {
    static BINARY_OWNED_CLI_WRITE_WAIT: Cell<Option<Duration>> = const { Cell::new(None) };
}

/// The wait the current thread's binary-owned CLI write policy allows, if one
/// is installed.
pub fn binary_owned_cli_write_wait() -> Option<Duration> {
    BINARY_OWNED_CLI_WRITE_WAIT.with(Cell::get)
}

/// Restores the previous policy when dropped.
#[must_use = "the policy is only active while the guard is alive"]
pub struct BinaryOwnedCliWriteGuard {
    previous: Option<Duration>,
}

impl Drop for BinaryOwnedCliWriteGuard {
    fn drop(&mut self) {
        BINARY_OWNED_CLI_WRITE_WAIT.with(|slot| slot.set(self.previous));
    }
}

/// Install the binary-owned CLI write policy on this thread until the guard
/// drops. Only the standalone mutation commands install it; the response
/// write/closeout path keeps its own retained-capture ownership semantics.
pub fn enter_binary_owned_cli_write(wait: Duration) -> BinaryOwnedCliWriteGuard {
    let previous = BINARY_OWNED_CLI_WRITE_WAIT.with(|slot| slot.replace(Some(wait)));
    BinaryOwnedCliWriteGuard { previous }
}

/// Run `f` under the binary-owned CLI write policy.
pub fn with_binary_owned_cli_write<T>(wait: Duration, f: impl FnOnce() -> T) -> T {
    let _guard = enter_binary_owned_cli_write(wait);
    f()
}

/// The configured wait: [`RETAINED_CLI_WRITE_WAIT_ENV`] when it parses, else
/// the default.
pub fn configured_retained_cli_write_wait() -> Duration {
    std::env::var(RETAINED_CLI_WRITE_WAIT_ENV)
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .map(Duration::from_millis)
        .unwrap_or(RETAINED_CLI_WRITE_DEFAULT_WAIT)
}

/// What a retained-delivery await observed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RetainedDeliveryAwait {
    /// Delivery converged. `observed_text` is the authority text at that edge
    /// when the document is attached (`None` once it is detached).
    Converged { observed_text: Option<String> },
    /// The bound expired (or no edge source exists) while the write was still
    /// retained.
    StillRetained,
}

/// A durable, retained CLI write whose delivery did not converge within the
/// bounded wait. NOT a failure: nothing to retry, nothing lost.
#[derive(Debug)]
pub struct RetainedCliWriteDeferred {
    pub file: String,
    pub source: String,
    pub waited_ms: u128,
    /// Why the wait ended without a converged landing.
    pub cause: &'static str,
    /// The original retained refusal, kept verbatim so token classifiers
    /// (`recovery=await_editor_replica_no_disk_write`,
    /// `retained=delivery_projection_pending`) still recognize it.
    pub refusal: String,
}

impl std::fmt::Display for RetainedCliWriteDeferred {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "[{RETAINED_CLI_WRITE_DEFERRED_TOKEN}] the {} write for {} was accepted by the editor \
             authority and is durable, but its delivery projection had not converged after {}ms \
             ({}). This is a deferral, not a failure: the change becomes visible on its own. Do \
             NOT re-run this command (it would apply the change twice), force disk, or `admin \
             recycle`; `agent-doc read {}` shows it once the editor converges. Retained refusal: {}",
            self.source, self.file, self.waited_ms, self.cause, self.file, self.refusal,
        )
    }
}

impl std::error::Error for RetainedCliWriteDeferred {}

/// Whether `err` (or any context layer of it) is a retained CLI write deferral.
pub fn retained_cli_write_deferral(err: &anyhow::Error) -> Option<&RetainedCliWriteDeferred> {
    err.downcast_ref::<RetainedCliWriteDeferred>()
}

/// Settle a post-write barrier refusal under the binary-owned CLI policy.
///
/// Returns `Ok(())` when the retained write converged and landed, the typed
/// deferral when the bound expired while still retained, and the original
/// refusal unchanged for anything else (policy not installed, or a refusal that
/// is not the converging-projection deferral).
pub(crate) fn settle_retained_cli_write(
    effects: &dyn EditorConvergenceEffects,
    file: &Path,
    pre_write: &str,
    target: &str,
    source: &str,
    refusal: anyhow::Error,
) -> Result<()> {
    let Some(wait) = binary_owned_cli_write_wait() else {
        return Err(refusal);
    };
    let rendered = format!("{refusal:#}");
    if !agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(&rendered) {
        return Err(refusal);
    }
    let started = std::time::Instant::now();
    let observed = effects.await_retained_delivery_projection(file, source, wait);
    let waited_ms = started.elapsed().as_millis();
    let (outcome, cause) = match observed {
        Ok(RetainedDeliveryAwait::Converged {
            observed_text: Some(text),
        }) if !retained_write_landed(pre_write, target, &text) => (
            "deferred",
            "delivery converged but the authority no longer carries this write's components \
             verbatim (a concurrent edit touched them)",
        ),
        Ok(RetainedDeliveryAwait::Converged { .. }) => ("converged", "converged"),
        Ok(RetainedDeliveryAwait::StillRetained) => {
            ("deferred", "the bounded convergence wait expired")
        }
        Err(_) => ("deferred", "the convergence edge could not be observed"),
    };
    agent_doc_ops_log_io::log_op(
        file,
        &format!(
            "binary_owned_cli_write_retained_settled file={} source={} outcome={} waited_ms={} wait_bound_ms={} disk_write=none",
            file.display(),
            source,
            outcome,
            waited_ms,
            wait.as_millis(),
        ),
    );
    if outcome == "converged" {
        eprintln!(
            "[agent-doc] {source}: the write for {} was retained while the editor delivery \
             projection converged; it converged after {waited_ms}ms and is visible.",
            file.display()
        );
        return Ok(());
    }
    Err(anyhow::Error::new(RetainedCliWriteDeferred {
        file: file.display().to_string(),
        source: source.to_string(),
        waited_ms,
        cause,
        refusal: rendered,
    }))
}

/// Whether `observed` carries every part of the document this write changed
/// from `pre_write` to `target`.
///
/// Compares whole components (markers included) by name and occurrence, plus
/// the frontmatter block, so an unrelated concurrent operator edit elsewhere
/// does not void the proof. Unparseable inputs fall back to exact equality.
pub fn retained_write_landed(pre_write: &str, target: &str, observed: &str) -> bool {
    if observed == target {
        return true;
    }
    let (Some(pre), Some(tgt), Some(obs)) = (
        component_slices(pre_write),
        component_slices(target),
        component_slices(observed),
    ) else {
        return false;
    };
    if frontmatter_block(pre_write) != frontmatter_block(target)
        && frontmatter_block(observed) != frontmatter_block(target)
    {
        return false;
    }
    let lookup = |slices: &[(String, usize, String)], name: &str, nth: usize| {
        slices
            .iter()
            .find(|(n, i, _)| n == name && *i == nth)
            .map(|(_, _, body)| body.clone())
    };
    let mut changed_any = false;
    for (name, nth, _) in pre.iter().chain(tgt.iter()) {
        let before = lookup(&pre, name, *nth);
        let after = lookup(&tgt, name, *nth);
        if before == after {
            continue;
        }
        changed_any = true;
        if lookup(&obs, name, *nth) != after {
            return false;
        }
    }
    // A change outside every component and the frontmatter is only provable
    // by exact equality, which already failed above.
    changed_any || frontmatter_block(pre_write) != frontmatter_block(target)
}

fn component_slices(content: &str) -> Option<Vec<(String, usize, String)>> {
    let components = agent_doc_element::element::parse(content).ok()?;
    let mut seen: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
    Some(
        components
            .into_iter()
            .map(|component| {
                let nth = seen.entry(component.name.clone()).or_default();
                let index = *nth;
                *nth += 1;
                (
                    component.name,
                    index,
                    content[component.open_start..component.close_end].to_string(),
                )
            })
            .collect(),
    )
}

fn frontmatter_block(content: &str) -> Option<&str> {
    let rest = content.strip_prefix("---\n")?;
    let end = rest.find("\n---\n")?;
    Some(&content[..4 + end + 5])
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const PRE: &str = "---\nagent_doc_session: s\n---\n\n<!-- agent:backlog -->\n- [ ] [#a] one\n<!-- /agent:backlog -->\n\n<!-- agent:queue -->\n- [ ] do [#a]\n<!-- /agent:queue -->\n";

    fn target() -> String {
        PRE.replace("- [ ] [#a] one\n", "- [ ] [#a] one\n- [ ] [#b] two\n")
    }

    fn pending_refusal() -> anyhow::Error {
        anyhow::anyhow!(
            "visible document write for x.md is retained by the lazy delivery projection because \
             the editor state projection has not converged; no secondary snapshot/commit or forced \
             disk write was attempted. remedy [{}] [{}]",
            agent_doc_turn::write_ownership::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
            agent_doc_turn::write_ownership::RETAINED_DELIVERY_PROJECTION_PENDING_TOKEN,
        )
    }

    /// Effects whose await reports a scripted outcome and counts calls.
    struct ScriptedAwait {
        outcome: RetainedDeliveryAwait,
        awaits: AtomicUsize,
    }

    impl EditorConvergenceEffects for ScriptedAwait {
        fn atomic_write(&self, _file: &Path, _content: &str) -> Result<()> {
            panic!("a retained CLI write must never fall through to a disk write")
        }

        fn await_retained_delivery_projection(
            &self,
            _file: &Path,
            _source: &str,
            _wait: Duration,
        ) -> Result<RetainedDeliveryAwait> {
            self.awaits.fetch_add(1, Ordering::SeqCst);
            Ok(self.outcome.clone())
        }

        fn guard_visible_write_expected_current(
            &self,
            _file: &Path,
            _source: &str,
            _expected_current: &str,
        ) -> Result<()> {
            panic!("not reached")
        }

        fn atomic_write_if_current(
            &self,
            _file: &Path,
            _content: &str,
            _expected_current: &str,
            _source: &str,
        ) -> Result<()> {
            panic!("a retained CLI write must never fall through to a disk write")
        }
    }

    fn scripted(outcome: RetainedDeliveryAwait) -> ScriptedAwait {
        ScriptedAwait {
            outcome,
            awaits: AtomicUsize::new(0),
        }
    }

    fn doc() -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::TempDir::new().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
        let file = dir.path().join("x.md");
        std::fs::write(&file, PRE).unwrap();
        (dir, file)
    }

    #[test]
    fn without_the_policy_the_refusal_propagates_unchanged() {
        let (_dir, file) = doc();
        let effects = scripted(RetainedDeliveryAwait::Converged {
            observed_text: None,
        });
        let err = settle_retained_cli_write(
            &effects,
            &file,
            PRE,
            &target(),
            "pending_write",
            pending_refusal(),
        )
        .unwrap_err();
        assert!(retained_cli_write_deferral(&err).is_none());
        assert_eq!(effects.awaits.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn a_converged_landed_retained_write_succeeds() {
        let (_dir, file) = doc();
        let effects = scripted(RetainedDeliveryAwait::Converged {
            observed_text: Some(target()),
        });
        with_binary_owned_cli_write(Duration::from_secs(5), || {
            settle_retained_cli_write(
                &effects,
                &file,
                PRE,
                &target(),
                "pending_write",
                pending_refusal(),
            )
        })
        .unwrap();
        assert_eq!(effects.awaits.load(Ordering::SeqCst), 1);
        let log = std::fs::read_to_string(file.parent().unwrap().join(".agent-doc/logs/ops.log"))
            .unwrap();
        assert!(log.contains("binary_owned_cli_write_retained_settled"));
        assert!(log.contains("outcome=converged"));
    }

    #[test]
    fn an_expired_wait_is_a_typed_deferral_that_keeps_the_refusal_tokens() {
        let (_dir, file) = doc();
        let effects = scripted(RetainedDeliveryAwait::StillRetained);
        let err = with_binary_owned_cli_write(Duration::ZERO, || {
            settle_retained_cli_write(
                &effects,
                &file,
                PRE,
                &target(),
                "queue_sync",
                pending_refusal(),
            )
        })
        .unwrap_err()
        .context("queue sync: failed to write document");
        let deferral = retained_cli_write_deferral(&err).expect("typed deferral through context");
        assert_eq!(deferral.source, "queue_sync");
        let rendered = format!("{err:#}");
        assert!(rendered.contains(RETAINED_CLI_WRITE_DEFERRED_TOKEN));
        assert!(
            agent_doc_turn::write_ownership::is_retained_delivery_projection_pending(&rendered),
            "inner callers that classify by token must still recognize the deferral"
        );
        assert!(rendered.contains("Do NOT re-run this command"));
    }

    #[test]
    fn a_refusal_without_the_pending_token_still_fails() {
        let (_dir, file) = doc();
        let effects = scripted(RetainedDeliveryAwait::Converged {
            observed_text: Some(target()),
        });
        // An unreachable editor authority / rejecting endpoint carries the class
        // token but withholds the narrow pending marker.
        let refused = anyhow::anyhow!(
            "visible document write for x.md is retained; the attached editor replica is not \
             registered [{}]",
            agent_doc_turn::write_ownership::AWAIT_EDITOR_REPLICA_NO_DISK_WRITE_TOKEN,
        );
        let err = with_binary_owned_cli_write(Duration::from_secs(5), || {
            settle_retained_cli_write(&effects, &file, PRE, &target(), "pending_write", refused)
        })
        .unwrap_err();
        assert!(retained_cli_write_deferral(&err).is_none());
        assert_eq!(effects.awaits.load(Ordering::SeqCst), 0);
        let generic = with_binary_owned_cli_write(Duration::from_secs(5), || {
            settle_retained_cli_write(
                &effects,
                &file,
                PRE,
                &target(),
                "pending_write",
                anyhow::anyhow!("pending_write: refused direct disk write"),
            )
        })
        .unwrap_err();
        assert!(retained_cli_write_deferral(&generic).is_none());
    }

    #[test]
    fn a_converged_but_clobbered_write_is_deferred_not_claimed() {
        let (_dir, file) = doc();
        let clobbered = PRE.replace("- [ ] [#a] one\n", "- [ ] [#a] one (edited)\n");
        let effects = scripted(RetainedDeliveryAwait::Converged {
            observed_text: Some(clobbered),
        });
        let err = with_binary_owned_cli_write(Duration::from_secs(5), || {
            settle_retained_cli_write(
                &effects,
                &file,
                PRE,
                &target(),
                "pending_write",
                pending_refusal(),
            )
        })
        .unwrap_err();
        assert!(retained_cli_write_deferral(&err).is_some());
    }

    #[test]
    fn the_guard_restores_the_previous_policy() {
        assert_eq!(binary_owned_cli_write_wait(), None);
        {
            let _outer = enter_binary_owned_cli_write(Duration::from_secs(1));
            {
                let _inner = enter_binary_owned_cli_write(Duration::from_secs(2));
                assert_eq!(binary_owned_cli_write_wait(), Some(Duration::from_secs(2)));
            }
            assert_eq!(binary_owned_cli_write_wait(), Some(Duration::from_secs(1)));
        }
        assert_eq!(binary_owned_cli_write_wait(), None);
    }

    #[test]
    fn landing_ignores_unrelated_concurrent_edits() {
        let observed = target().replace("- [ ] do [#a]\n", "- [ ] do [#a]\n- [ ] operator line\n");
        assert!(retained_write_landed(PRE, &target(), &observed));
        assert!(!retained_write_landed(PRE, &target(), PRE));
        let fm_target = PRE.replace("agent_doc_session: s", "agent_doc_session: t");
        assert!(retained_write_landed(PRE, &fm_target, &fm_target));
        assert!(!retained_write_landed(PRE, &fm_target, PRE));
    }
}
