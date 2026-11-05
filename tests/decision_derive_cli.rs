//! `#firstprinciplesstore`: `agent-doc decision derive` stores one
//! first-principles derivation step in the session document's decision graph
//! (a `derivation` node, `derived_from` edges to its premises, a `concludes`
//! edge to the decision), and only the operator's `confirm [#drv-…]` /
//! `reject [#drv-…]` rows change its review status.
use std::fs;
use std::process::Command;

const CONTROLLER_LAUNCH_REASON: &str =
    "decision derive CLI fixture drives derive/graph/preflight against a temp document";

fn git(root: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(root)
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn run_agent_doc(dir: &std::path::Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_agent-doc"))
        .env(
            agent_doc_controller_io::project_controller::TEST_CONTROLLER_LAUNCH_REASON_ENV,
            CONTROLLER_LAUNCH_REASON,
        )
        .env("AGENT_DOC_ROOT_CEILING_DIRECTORIES", dir.parent().unwrap())
        // Never read the operator's real global principles file.
        .env("XDG_CONFIG_HOME", dir.join(".config"))
        .current_dir(dir)
        .args(args)
        .output()
        .unwrap()
}

fn assert_ok(output: &std::process::Output, what: &str) {
    assert!(
        output.status.success(),
        "{what} failed: stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn content(run_rows: &str) -> String {
    format!(
        concat!(
            "---\nagent_doc_session: derive\nagent_doc_format: template\n---\n\n",
            "## Rules <!-- agent:rules -->\n",
            "- principle [#simple]: Prefer the simplest mechanism that is correct.\n",
            "<!-- /agent:rules -->\n\n",
            "## Exchange\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: tokens — gpt-5 · 2026-11-05T08:00-05:00\n\nReady.\n",
            "<!-- agent:boundary:keep -->\n",
            "<!-- /agent:exchange -->\n\n",
            "## Run <!-- agent:run -->\n",
            "{run_rows}",
            "<!-- /agent:run -->\n\n",
            "## Todo <!-- agent:todo priority -->\n",
            "- [ ] [#tdcap] Decide whether to cap the session token lifetime\n",
            "<!-- /agent:todo -->\n",
        ),
        run_rows = run_rows,
    )
}

fn session(dir: &std::path::Path) -> std::path::PathBuf {
    fs::create_dir_all(dir.join(".agent-doc/locks")).unwrap();
    let doc = dir.join("session.md");
    let content = content("");
    fs::write(&doc, &content).unwrap();
    git(dir, &["init", "-q"]);
    git(dir, &["config", "user.email", "test@example.com"]);
    git(dir, &["config", "user.name", "Test User"]);
    git(dir, &["add", "session.md"]);
    git(dir, &["commit", "-q", "--no-verify", "-m", "initial"]);
    agent_doc_snapshot_io::checkpoint_document_baseline(
        &doc,
        &content,
        agent_doc_ops_log_io::log_op,
    )
    .unwrap();
    doc
}

fn derive(dir: &std::path::Path, path: &str, answer: &str) -> serde_json::Value {
    let output = run_agent_doc(
        dir,
        &[
            "decision",
            "derive",
            path,
            "--premise",
            "principle:simple",
            "--premise",
            "fact:jws=Session tokens are JWS and carry their own expiry.",
            "--inference",
            "A self-expiring JWS already bounds the lifetime, so a second cap adds a mechanism without adding correctness.",
            "--decision",
            "tdcap",
            "--answer",
            answer,
            "--json",
        ],
    );
    assert_ok(&output, "decision derive");
    serde_json::from_slice(&output.stdout).unwrap()
}

fn set_run_rows(doc: &std::path::Path, rows: &str) {
    fs::write(doc, content(rows)).unwrap();
}

#[test]
fn a_derivation_is_stored_in_the_decision_graph_and_reviewed_by_the_operator() {
    let dir = tempfile::tempdir().unwrap();
    let doc = session(dir.path());
    let path = doc.to_str().unwrap();

    let first = derive(dir.path(), path, "No cap.");
    let derivation = &first["derivation"];
    let id = derivation["id"].as_str().unwrap().to_string();
    assert!(id.starts_with("drv-"), "{first:#}");
    assert_eq!(first["created"], true, "{first:#}");
    assert_eq!(
        derivation["status"], "agent_derived_unreviewed",
        "{first:#}"
    );
    assert!(
        derivation["decision"].as_str().unwrap().starts_with("dec-"),
        "{first:#}"
    );
    let premises = derivation["premises"].as_array().unwrap();
    assert_eq!(premises.len(), 2, "{first:#}");
    assert_eq!(premises[0]["node"], "principle:simple", "{first:#}");
    assert_eq!(
        premises[0]["label"], "Prefer the simplest mechanism that is correct.",
        "{first:#}"
    );
    assert_eq!(premises[1]["node"], "fact:jws", "{first:#}");
    assert_eq!(
        first["review_forms"][0],
        format!("confirm [#{id}]"),
        "{first:#}"
    );

    // Authoring the same step again finds it on record.
    let again = derive(dir.path(), path, "No cap.");
    assert_eq!(again["created"], false, "{again:#}");
    assert_eq!(again["derivation"]["id"], id.as_str(), "{again:#}");

    // A premise must be a principle in scope.
    let output = run_agent_doc(
        dir.path(),
        &[
            "decision",
            "derive",
            path,
            "--premise",
            "principle:fast",
            "--inference",
            "x",
            "--decision",
            "tdcap",
            "--answer",
            "y",
        ],
    );
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("no principle [#fast] is in scope") && stderr.contains("simple"),
        "{stderr}"
    );

    // The step is ordinary decision-graph rows.
    let output = run_agent_doc(dir.path(), &["decision", "graph", path, "--json"]);
    assert_ok(&output, "decision graph");
    let graph: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let nodes = graph["nodes"].as_array().unwrap();
    for (node, kind) in [
        (id.as_str(), "derivation"),
        ("principle:simple", "principle"),
        ("fact:jws", "fact"),
    ] {
        assert!(
            nodes.iter().any(|n| n["id"] == node && n["kind"] == kind),
            "{node} {kind}: {graph:#}"
        );
    }
    let edges = graph["edges"].as_array().unwrap();
    let from_step: Vec<_> = edges.iter().filter(|e| e["from"] == id.as_str()).collect();
    assert_eq!(from_step.len(), 3, "{graph:#}");
    assert!(
        from_step.iter().any(|e| e["kind"] == "concludes"
            && e["to"] == derivation["decision"]
            && e["clause"] == "No cap."
            && e["source"] == "proposed"),
        "{graph:#}"
    );
    assert_eq!(
        from_step
            .iter()
            .filter(|e| e["kind"] == "derived_from")
            .count(),
        2,
        "{graph:#}"
    );

    // A conflicting step from the same premises is reported.
    let other = derive(dir.path(), path, "Cap at one hour.");
    let other_id = other["derivation"]["id"].as_str().unwrap().to_string();
    assert_ne!(other_id, id);
    let contradictions = other["contradictions"].as_array().unwrap();
    assert_eq!(contradictions.len(), 1, "{other:#}");
    assert_eq!(contradictions[0]["kind"], "same_premises", "{other:#}");

    // The operator confirms one step and rejects the other; a preflight pass
    // records both.
    set_run_rows(
        &doc,
        &format!("- confirm [#{id}]\n- [#{other_id}]: reject — the JWS already expires\n"),
    );
    let output = run_agent_doc(dir.path(), &["preflight", path]);
    assert_ok(&output, "preflight");

    // The rows are pruned; the store still remembers the reviews.
    set_run_rows(&doc, "");
    let confirmed = derive(dir.path(), path, "No cap.");
    assert_eq!(
        confirmed["derivation"]["status"], "operator_confirmed",
        "{confirmed:#}"
    );
    assert!(
        confirmed["contradictions"].as_array().unwrap().is_empty(),
        "{confirmed:#}"
    );
    let rejected = derive(dir.path(), path, "Cap at one hour.");
    assert_eq!(rejected["derivation"]["status"], "rejected", "{rejected:#}");
    assert_eq!(
        rejected["derivation"]["review_note"], "the JWS already expires",
        "{rejected:#}"
    );

    let output = run_agent_doc(dir.path(), &["decision", "graph", path, "--json"]);
    assert_ok(&output, "decision graph");
    let graph: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let edges = graph["edges"].as_array().unwrap();
    assert!(
        edges
            .iter()
            .filter(|e| e["from"] == id.as_str())
            .all(|e| e["source"] == "operator"),
        "{graph:#}"
    );
    // Rejecting a step never rejects its premises.
    assert!(
        graph["nodes"]
            .as_array()
            .unwrap()
            .iter()
            .any(|n| n["id"] == "fact:jws"),
        "{graph:#}"
    );
}
