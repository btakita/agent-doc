//! A dispatched worker that runs out of tokens or credits
//! (`#harnessquotaexhausted`), driven through the real `agent-doc` CLI.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

/// `#integrationtesttargets`: this target drives the real `agent-doc` CLI.
const CONTROLLER_LAUNCH_REASON: &str = "queue claim/report CLI commands reach the project controller, which they launch as production does";

const LIMIT_NOTICE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/agent-doc-harness/src/fixtures/usage_limit/worker_claude_partial_then_limit.txt"
));
const REAL_REPORT: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/agent-doc-harness/src/fixtures/usage_limit/worker_real_report_quoting_limit.txt"
));

const ITEM: &str = "finish sdk";

fn run(dir: &Path, args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_agent-doc"))
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env_remove("TMUX")
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn ok(dir: &Path, args: &[&str]) -> String {
    let output = run(dir, args);
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(output.status.success(), "{args:?} failed: {stderr}");
    stderr
}

fn started_session(dir: &Path) -> String {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    fs::write(
        &doc,
        concat!(
            "---\nagent_doc_session: worker-quota\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append subagents -->\n",
            "### Re: prior — gpt-5\n\nDone.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "<!-- agent:queue subagents -->\n",
            "- finish sdk\n",
            "- finish dev\n",
            "<!-- /agent:queue -->\n",
        ),
    )
    .unwrap();
    let doc = doc.to_str().unwrap().to_string();
    ok(
        dir,
        &[
            "queue",
            "claim",
            &doc,
            "--item",
            ITEM,
            "--owner",
            "subagent:sdk",
        ],
    );
    ok(
        dir,
        &[
            "queue",
            "start",
            &doc,
            "--item",
            ITEM,
            "--owner",
            "subagent:sdk",
            "--generation",
            "1",
            "--worker-id",
            "worker-sdk",
        ],
    );
    doc
}

fn claims(dir: &Path, doc: &str) -> serde_json::Value {
    let output = run(dir, &["queue", "claims", doc]);
    assert!(output.status.success());
    serde_json::from_slice(&output.stdout).unwrap()
}

fn claim_for<'a>(claims: &'a serde_json::Value, item: &str) -> &'a serde_json::Value {
    claims
        .as_array()
        .or_else(|| claims.get("claims").and_then(|claims| claims.as_array()))
        .expect("claims array")
        .iter()
        .find(|report| {
            let claim = report.get("claim").unwrap_or(report);
            claim["item_text"]
                .as_str()
                .is_some_and(|text| text.contains(item))
        })
        .expect("claim for item")
}

fn report_limit(dir: &Path, doc: &str) -> String {
    ok(
        dir,
        &[
            "queue",
            "report-complete",
            "--prose-only",
            doc,
            "--item",
            ITEM,
            "--generation",
            "1",
            "--branch",
            "fix/sdk-partial",
            "--worktree",
            "/work/sdk",
            "--response",
            LIMIT_NOTICE,
        ],
    )
}

#[test]
fn a_limit_notice_hand_back_blocks_the_attempt_instead_of_reporting_it() {
    let dir = tempfile::tempdir().unwrap();
    let doc = started_session(dir.path());
    let stderr = report_limit(dir.path(), &doc);
    for needle in [
        "worker_quota_exhausted",
        "session limit",
        "resets 7:30pm",
        "branch `fix/sdk-partial` in worktree `/work/sdk`",
        "resume the same worker `worker-sdk`",
        "--requeue",
    ] {
        assert!(stderr.contains(needle), "missing {needle:?}: {stderr}");
    }

    let rendered = fs::read_to_string(&doc).unwrap();
    assert!(rendered.contains("⛔"), "row must read blocked: {rendered}");
    assert!(
        !rendered.contains("🚧"),
        "row must not read running: {rendered}"
    );
    assert!(
        !rendered.contains("You've hit your session limit"),
        "a limit notice is not worker output for the exchange: {rendered}"
    );

    let claims = claims(dir.path(), &doc);
    let report = claim_for(&claims, ITEM);
    let claim = report.get("claim").unwrap_or(report);
    assert_eq!(claim["state"], "quota_blocked");
    assert_eq!(claim["worker"]["worker_id"], "worker-sdk");
    assert_eq!(claim["quota_block"]["branch"], "fix/sdk-partial");
    assert_eq!(claim["quota_block"]["worktree"], "/work/sdk");
}

#[test]
fn a_blocked_attempt_is_not_silently_redispatched() {
    let dir = tempfile::tempdir().unwrap();
    let doc = started_session(dir.path());
    report_limit(dir.path(), &doc);

    let reclaim = run(
        dir.path(),
        &[
            "queue",
            "claim",
            &doc,
            "--item",
            ITEM,
            "--owner",
            "subagent:replacement",
        ],
    );
    assert!(!reclaim.status.success());
    let stderr = String::from_utf8_lossy(&reclaim.stderr);
    assert!(stderr.contains("worker_quota_exhausted"), "{stderr}");
    assert!(stderr.contains("--requeue"), "{stderr}");
}

#[test]
fn resuming_the_same_worker_returns_the_row_to_running() {
    let dir = tempfile::tempdir().unwrap();
    let doc = started_session(dir.path());
    report_limit(dir.path(), &doc);

    ok(
        dir.path(),
        &[
            "queue",
            "start",
            &doc,
            "--item",
            ITEM,
            "--owner",
            "subagent:sdk",
            "--generation",
            "1",
            "--worker-id",
            "worker-sdk",
        ],
    );
    let claims = claims(dir.path(), &doc);
    let report = claim_for(&claims, ITEM);
    let claim = report.get("claim").unwrap_or(report);
    assert_eq!(claim["state"], "running");
    assert!(claim.get("quota_block").is_none());
    assert!(fs::read_to_string(&doc).unwrap().contains("🚧"));
}

#[test]
fn an_explicit_requeue_carries_the_partial_branch_into_the_next_brief() {
    let dir = tempfile::tempdir().unwrap();
    let doc = started_session(dir.path());
    report_limit(dir.path(), &doc);

    ok(
        dir.path(),
        &[
            "queue",
            "release",
            &doc,
            "--item",
            ITEM,
            "--generation",
            "1",
            "--requeue",
        ],
    );
    ok(
        dir.path(),
        &[
            "queue",
            "claim",
            &doc,
            "--item",
            ITEM,
            "--owner",
            "subagent:replacement",
        ],
    );
    let brief = run(dir.path(), &["queue", "brief", &doc, "--item", ITEM]);
    assert!(brief.status.success());
    let brief = String::from_utf8_lossy(&brief.stdout);
    assert!(brief.contains("stopped by a usage limit"), "{brief}");
    assert!(
        brief.contains("branch `fix/sdk-partial` in worktree `/work/sdk`"),
        "{brief}"
    );
}

#[test]
fn a_real_report_that_quotes_a_limit_is_still_a_report() {
    let dir = tempfile::tempdir().unwrap();
    let doc = started_session(dir.path());
    ok(
        dir.path(),
        &[
            "queue",
            "report-complete",
            "--prose-only",
            &doc,
            "--item",
            ITEM,
            "--generation",
            "1",
            "--response",
            REAL_REPORT,
        ],
    );
    let claims = claims(dir.path(), &doc);
    let report = claim_for(&claims, ITEM);
    let claim = report.get("claim").unwrap_or(report);
    assert_eq!(claim["state"], "reported");
}

#[test]
fn the_explicit_command_records_an_exit_with_unknown_wording() {
    let dir = tempfile::tempdir().unwrap();
    let doc = started_session(dir.path());
    let stderr = ok(
        dir.path(),
        &[
            "queue",
            "report-quota-blocked",
            &doc,
            "--item",
            ITEM,
            "--generation",
            "1",
            "--harness",
            "opencode",
            "--branch",
            "fix/sdk-partial",
            "--evidence",
            "provider returned 402",
        ],
    );
    assert!(stderr.contains("worker_quota_exhausted"), "{stderr}");
    let claims = claims(dir.path(), &doc);
    let report = claim_for(&claims, ITEM);
    let claim = report.get("claim").unwrap_or(report);
    assert_eq!(claim["state"], "quota_blocked");
    assert_eq!(claim["quota_block"]["harness"], "opencode");
    assert_eq!(claim["quota_block"]["evidence"], "provider returned 402");
}
