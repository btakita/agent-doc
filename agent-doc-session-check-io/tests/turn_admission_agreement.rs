//! `#admissionsteeringagree` (GH #118): preflight admission and `session-check`
//! derive "may this turn continue?" from one predicate, and no recovery either
//! of them names may absorb an unanswered operator prompt.
//!
//! The fixture is the issue's shape: the last cycle is `committed`, the snapshot
//! lags HEAD by frontmatter metadata only (`first_differing_line` in the
//! frontmatter), and the operator has typed a fresh prompt into the visible
//! document. Before the fix, `session-check` said "realtime steering — run
//! `agent-doc <FILE>`" while preflight's drift gate refused admission and named
//! `reset --from-current`, which folds that prompt into the baseline.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use agent_doc_session_check_io::{SessionCheckStatus, TurnAdmissionVerdict};
use agent_doc_turn::turn_admission::{PENDING_OPERATOR_STEERING_RECOVERY, TurnAdmission};

const PROMPT: &str = "❯ bug: when I select the remote terminal, the cursor box disappears";

fn head_doc() -> String {
    concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_last_cycle: cycle-2\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ alt+shift no longer works\n\n",
        "### Re: alt+shift no longer works\n\n",
        "Fixed the binding.\n",
        "<!-- /agent:exchange -->\n",
    )
    .to_string()
}

/// The snapshot lags HEAD by binary-owned frontmatter metadata only.
fn lagging_snapshot(head: &str) -> String {
    head.replace("agent_doc_last_cycle: cycle-2\n", "")
}

fn with_operator_prompt(head: &str) -> String {
    head.replace(
        "Fixed the binding.\n",
        &format!("Fixed the binding.\n\n{PROMPT}\n"),
    )
}

fn git(root: &Path, args: &[&str]) {
    let out = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(out.status.success(), "git {args:?}: {out:?}");
}

/// Committed cycle + metadata-only snapshot/HEAD drift; `visible` is what the
/// operator's editor holds.
fn committed_cycle_with_lagging_snapshot(root: &Path, visible: &str) -> PathBuf {
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    let doc = root.join("doc.md");
    let head = head_doc();
    let snapshot = lagging_snapshot(&head);
    fs::write(&doc, &head).unwrap();
    git(root, &["add", "doc.md"]);
    git(
        root,
        &["commit", "-q", "-m", "committed cycle", "--no-verify"],
    );

    agent_doc_cycle_state_io::start_preflight(&doc, Some(&snapshot), Some(&snapshot)).unwrap();
    agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
        &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
        &doc,
        "commit_success",
        Some(&snapshot),
        Some(&head),
    )
    .unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &snapshot,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    fs::write(&doc, visible).unwrap();
    doc
}

fn admission(doc: &Path) -> TurnAdmissionVerdict {
    let _lock = agent_doc_test_support::env_lock();
    agent_doc_session_check_io::turn_admission(doc, false).unwrap()
}

/// What preflight's drift gate refuses with (`enforce_no_uncommitted_closeout_drift`
/// bails on exactly this `Some`).
fn preflight_drift_refusal(doc: &Path) -> Option<String> {
    let _lock = agent_doc_test_support::env_lock();
    agent_doc_session_check_io::detect_uncommitted_closeout_drift(
        doc,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )
    .unwrap()
}

fn session_check(doc: &Path) -> SessionCheckStatus {
    let _lock = agent_doc_test_support::env_lock();
    agent_doc_session_check_io::inspect(
        doc,
        &agent_doc_closeout_runtime_io::session_check_effects(),
    )
    .unwrap()
}

fn recovery_hint(doc: &Path) -> String {
    let _lock = agent_doc_test_support::env_lock();
    agent_doc_closeout_runtime_io::closeout_recovery_hint(doc)
}

#[test]
fn preflight_admits_when_session_check_says_continue_with_steering() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = committed_cycle_with_lagging_snapshot(dir.path(), &with_operator_prompt(&head_doc()));

    // The one predicate: steering after a closed cycle continues the turn.
    let verdict = admission(&doc);
    assert_eq!(verdict.admission, TurnAdmission::ContinueWithSteering);
    let steering = verdict.steering.expect("steering marker");
    assert!(steering.contains("the cursor box disappears"), "{steering}");

    // Preflight's drift gate derives from it: no refusal.
    assert_eq!(
        preflight_drift_refusal(&doc),
        None,
        "preflight must not refuse a turn whose next prompt is pending steering"
    );

    // session-check derives from it too: continue and answer the prompt.
    match session_check(&doc) {
        SessionCheckStatus::Interrupted(message) => {
            assert!(
                message.contains("unresolved prompt-bearing user changes")
                    && message.contains("the cursor box disappears"),
                "{message}"
            );
        }
        SessionCheckStatus::Ok(message) => {
            panic!("pending steering must be surfaced, not reported clean: {message}")
        }
    }
}

#[test]
fn recovery_hint_never_names_an_absorbing_recovery_over_pending_steering() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = committed_cycle_with_lagging_snapshot(dir.path(), &with_operator_prompt(&head_doc()));

    let hint = recovery_hint(&doc);
    assert!(hint.contains(PENDING_OPERATOR_STEERING_RECOVERY), "{hint}");
    assert_eq!(
        agent_doc_turn::closeout_recovery::short_recovery_command_from_recommendation(&hint)
            .as_deref(),
        Some(format!("agent-doc {}", doc.display()).as_str()),
        "the runnable recovery must be the steering turn itself: {hint}"
    );
    assert!(hint.contains("the cursor box disappears"), "{hint}");
}

#[test]
fn without_steering_both_surfaces_require_a_clean_closeout() {
    let dir = tempfile::TempDir::new().unwrap();
    let doc = committed_cycle_with_lagging_snapshot(dir.path(), &head_doc());

    let verdict = admission(&doc);
    assert_eq!(verdict.admission, TurnAdmission::RequireCleanCloseout);
    assert_eq!(verdict.steering, None);

    // GH #118 ask 3: the refusal says which closed cycle it is and that no
    // operator steering was observed, instead of reading as "nothing in flight".
    let refusal = preflight_drift_refusal(&doc).expect("no steering: preflight refuses the drift");
    assert!(
        refusal.contains("phase=committed")
            && refusal.contains("turn_admission=require_clean_closeout")
            && refusal.contains("no unanswered operator steering observed"),
        "{refusal}"
    );
    assert!(
        !refusal.contains(PENDING_OPERATOR_STEERING_RECOVERY),
        "no steering is pending, so the classifier's own recovery stands: {refusal}"
    );
    // ...and session-check agrees: the same drift is not a clean closeout.
    assert!(
        matches!(session_check(&doc), SessionCheckStatus::Interrupted(_)),
        "both surfaces must refuse the same unsteered drift"
    );
}

/// The exact recovery GH #118 refused to run: snapshot == HEAD, and the visible
/// file differs only outside the content signature — here an operator revision
/// of a queue item, which the recovery classifier counts as metadata and
/// therefore names `reset --from-current` for. That revision is the operator's
/// steering; rebuilding the baseline from the visible file would erase it.
#[test]
fn queue_revision_steering_is_never_classified_into_reset_from_current() {
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path();
    let head = format!(
        "{}\n<!-- agent:queue -->\n- do [#cursorbox] old option\n<!-- /agent:queue -->\n",
        head_doc()
    );
    let visible = head.replace(
        "- do [#cursorbox] old option",
        "- do [#cursorbox] keep the cursor box visible when the remote terminal is selected",
    );
    fs::create_dir_all(root.join(".agent-doc/logs")).unwrap();
    git(root, &["init", "-q"]);
    git(root, &["config", "user.email", "test@example.com"]);
    git(root, &["config", "user.name", "Test"]);
    let doc = root.join("doc.md");
    fs::write(&doc, &head).unwrap();
    git(root, &["add", "doc.md"]);
    git(
        root,
        &["commit", "-q", "-m", "committed cycle", "--no-verify"],
    );
    agent_doc_cycle_state_io::start_preflight(&doc, Some(&head), Some(&head)).unwrap();
    agent_doc_cycle_state_io::pipeline_frontmatter::mark_committed(
        &agent_doc_document_realtime_io::RUNTIME_PIPELINE_FRONTMATTER_EFFECTS,
        &doc,
        "commit_success",
        Some(&head),
        Some(&head),
    )
    .unwrap();
    agent_doc_snapshot_io::checkpoint_document_baseline(&doc, &head, agent_doc_ops_log_io::log_op)
        .unwrap();
    fs::write(&doc, &visible).unwrap();

    let state = {
        let _lock = agent_doc_test_support::env_lock();
        agent_doc_flow_io::closeout::classify_closeout_recovery_state_for_file(
            &doc,
            &agent_doc_closeout_runtime_io::closeout_effects(),
        )
    };
    assert_eq!(
        state,
        agent_doc_turn::closeout_recovery::CloseoutRecoveryState::RecoveryProjectionVisibleDrift,
        "fixture must reproduce the classifier state GH #118 named"
    );

    let verdict = admission(&doc);
    assert_eq!(verdict.admission, TurnAdmission::ContinueWithSteering);
    assert!(
        verdict
            .steering
            .as_deref()
            .is_some_and(|steering| steering.contains("keep the cursor box visible")),
        "{verdict:?}"
    );
    assert_eq!(preflight_drift_refusal(&doc), None);

    let hint = recovery_hint(&doc);
    assert_eq!(
        agent_doc_turn::closeout_recovery::short_recovery_command_from_recommendation(&hint)
            .as_deref(),
        Some(format!("agent-doc {}", doc.display()).as_str()),
        "must name the steering turn, not `reset --from-current`: {hint}"
    );
    assert!(
        !hint.starts_with("Recovery [recovery_projection_visible_drift]"),
        "{hint}"
    );
}
