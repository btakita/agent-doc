use super::*;
use std::io::Write;
use std::process::Command;
use tempfile::TempDir;

// #install-stale-guard: the staleness rule flags only artifacts whose mtime
// predates the source commit by more than the grace window; absent artifacts
// (`None`) and artifacts within grace never fire, so the normal
// build → install → commit ordering does not self-trip the guard.
#[test]
fn stale_install_classifier_flags_only_artifacts_older_than_source_commit() {
    let commit = 10_000u64;
    let grace = 60u64;

    // All artifacts newer than the commit → nothing stale.
    assert!(
        classify_stale_install_artifacts(
            commit,
            &[("bin", Some(commit + 5)), ("cdylib", Some(commit + 1))],
            grace,
        )
        .is_empty()
    );

    // One artifact older than the commit by more than the grace → flagged.
    let stale = classify_stale_install_artifacts(
        commit,
        &[("bin", Some(commit - 600)), ("cdylib", Some(commit + 1))],
        grace,
    );
    assert_eq!(stale, vec!["bin"]);

    // Built just inside the grace window (install-then-commit seconds apart)
    // → not flagged; older than grace → flagged.
    assert!(
        classify_stale_install_artifacts(commit, &[("bin", Some(commit - 30))], grace).is_empty()
    );
    assert_eq!(
        classify_stale_install_artifacts(commit, &[("bin", Some(commit - 61))], grace),
        vec!["bin"]
    );

    // Absent artifacts (not installed) never fire.
    assert!(
        classify_stale_install_artifacts(commit, &[("bin", None), ("cdylib", None)], grace)
            .is_empty()
    );
}

// The source-repo locator accepts the document's git root when it is the
// `agent-doc` crate, the `src/agent-doc` dogfood submodule layout, and
// returns `None` (silent no-op) when no `agent-doc` Cargo.toml is present.
#[test]
fn locate_agent_doc_source_repo_matches_root_and_dogfood_layout() {
    let agent_doc_manifest = "[package]\nname = \"agent-doc\"\nversion = \"0.0.0\"\n";

    // Standalone checkout: the git root itself is the crate.
    let root = TempDir::new().unwrap();
    std::fs::write(root.path().join("Cargo.toml"), agent_doc_manifest).unwrap();
    assert_eq!(
        locate_agent_doc_source_repo(root.path()).as_deref(),
        Some(root.path())
    );

    // Dogfood superproject: source lives under src/agent-doc.
    let superproject = TempDir::new().unwrap();
    let src = superproject.path().join("src/agent-doc");
    std::fs::create_dir_all(&src).unwrap();
    std::fs::write(src.join("Cargo.toml"), agent_doc_manifest).unwrap();
    assert_eq!(locate_agent_doc_source_repo(superproject.path()), Some(src));

    // Unrelated repo (no agent-doc crate) → no warning source.
    let other = TempDir::new().unwrap();
    std::fs::write(
        other.path().join("Cargo.toml"),
        "[package]\nname = \"something-else\"\n",
    )
    .unwrap();
    assert!(locate_agent_doc_source_repo(other.path()).is_none());
}

// #per-cycle-protocol-output-overhead: empty Vec fields must not spend
// per-cycle context bytes. A healthy/default PreflightOutput omits the empty
// `claims` and `layout_issues` arrays from its JSON, and still round-trips
// back to empty Vecs (serde default) so consumers reading the struct are safe.
#[test]
fn preflight_output_omits_empty_claims_and_layout_issues() {
    let output = PreflightOutput::default();
    let json = serde_json::to_string(&output).unwrap();
    assert!(
        !json.contains("\"claims\""),
        "empty claims must be omitted from per-cycle output: {json}"
    );
    assert!(
        !json.contains("\"layout_issues\""),
        "empty layout_issues must be omitted from per-cycle output: {json}"
    );
    let round_trip: PreflightOutput = serde_json::from_str(&json).unwrap();
    assert!(round_trip.claims.is_empty());
    assert!(round_trip.layout_issues.is_empty());

    // Non-empty values are still emitted and round-trip intact.
    let populated = PreflightOutput {
        claims: vec!["claimed pane %1".to_string()],
        layout_issues: vec!["stash overflow".to_string()],
        ..PreflightOutput::default()
    };
    let json = serde_json::to_string(&populated).unwrap();
    assert!(json.contains("\"claims\""), "{json}");
    assert!(json.contains("\"layout_issues\""), "{json}");
    let round_trip: PreflightOutput = serde_json::from_str(&json).unwrap();
    assert_eq!(round_trip.claims, vec!["claimed pane %1".to_string()]);
    assert_eq!(round_trip.layout_issues, vec!["stash overflow".to_string()]);
}

#[test]
fn detect_identity_collisions_flags_preset_vs_backlog_id() {
    // #preset-item-id-collision: monsterrodholders.md repro — a #next-steps
    // prompt preset AND an active #next-steps backlog item collide.
    let content = concat!(
        "---\n",
        "prompt_presets:\n",
        "  '#next-steps': Any follow-up items?\n",
        "  '#commit-push': commit + push\n",
        "---\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#next-steps] do the next steps\n",
        "- [ ] [#other1] unrelated work\n",
        "<!-- /agent:backlog -->\n",
    );
    let collisions = detect_identity_collisions(content);
    assert_eq!(collisions.len(), 1, "{collisions:?}");
    assert!(collisions[0].contains("#next-steps"), "{collisions:?}");
    assert!(collisions[0].contains("prompt_presets"), "{collisions:?}");
    assert!(collisions[0].contains("agent:backlog"), "{collisions:?}");
}

#[test]
fn detect_identity_collisions_flags_duplicate_active_ids_across_components() {
    // The same active id in backlog and review is also ambiguous.
    let content = concat!(
        "---\nagent_doc_session: t\n---\n\n",
        "<!-- agent:backlog -->\n- [ ] [#dup7] in backlog\n<!-- /agent:backlog -->\n\n",
        "<!-- agent:review -->\n- [/] [#dup7] also gated in review\n<!-- /agent:review -->\n",
    );
    let collisions = detect_identity_collisions(content);
    assert_eq!(collisions.len(), 1, "{collisions:?}");
    assert!(collisions[0].contains("#dup7"), "{collisions:?}");
}

#[test]
fn detect_identity_collisions_ignores_done_ids_and_clean_docs() {
    // A clean doc (unique ids) and done items must not flag.
    let content = concat!(
        "---\nprompt_presets:\n  '#next-steps': x\n---\n\n",
        "<!-- agent:backlog -->\n- [ ] [#alpha] active\n<!-- /agent:backlog -->\n\n",
        // A done item reusing the preset name is archived, not an active target.
        "<!-- agent:review -->\n- [x] [#next-steps] completed long ago\n<!-- /agent:review -->\n",
    );
    assert!(
        detect_identity_collisions(content).is_empty(),
        "done ids and unique active ids must not collide"
    );
}

#[test]
fn identity_collision_for_new_id_reports_existing_sources() {
    // #preset-item-id-collision-enforce: a candidate id matching a preset
    // key or active item id reports the existing source(s); a free id is None.
    let content = concat!(
        "---\nprompt_presets:\n  '#next-steps': x\n---\n\n",
        "<!-- agent:backlog -->\n- [ ] [#alpha] active\n<!-- /agent:backlog -->\n",
    );
    assert_eq!(
        identity_collision_for_new_id(content, "next-steps"),
        Some(vec!["prompt_presets".to_string()])
    );
    // Normalization: leading `#` and case are ignored.
    assert_eq!(
        identity_collision_for_new_id(content, "#ALPHA"),
        Some(vec!["agent:backlog".to_string()])
    );
    assert_eq!(identity_collision_for_new_id(content, "fresh01"), None);
    assert_eq!(identity_collision_for_new_id(content, ""), None);
}

#[test]
fn strike_done_queue_head_prompts_marks_done_items_completed() {
    let entries = vec![
        crate::queue::QueueEntry::Preset("#spec-test-build-install-commit-push".to_string()),
        crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
            text: "do [#jbrsrbusyint]".to_string(),
            multiline: false,
        }),
        crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
            text: "do [#jbrsrbusysim]".to_string(),
            multiline: false,
        }),
    ];
    let done_ids: std::collections::HashSet<String> =
        ["jbrsrbusyint".to_string()].into_iter().collect();

    let (rewritten, struck) =
        super::strike_done_queue_head_prompts(&entries, &done_ids).expect("expected strike");

    assert_eq!(struck.len(), 1);
    assert_eq!(struck[0].text, "do [#jbrsrbusyint]");
    match &rewritten[1] {
        crate::queue::QueueEntry::Completed(prompt) => {
            assert_eq!(prompt.text, "do [#jbrsrbusyint]");
        }
        other => panic!("expected Completed for head prompt, got {:?}", other),
    }
    // The live head (`#jbrsrbusysim`) must stay intact for the normal
    // consumption path.
    match &rewritten[2] {
        crate::queue::QueueEntry::Prompt(prompt) => {
            assert_eq!(prompt.text, "do [#jbrsrbusysim]");
        }
        other => panic!("expected Prompt for live head, got {:?}", other),
    }
}

#[test]
fn strike_done_queue_head_prompts_returns_none_when_head_is_live() {
    let entries = vec![crate::queue::QueueEntry::Prompt(
        crate::queue::QueuePrompt {
            text: "do [#stillopen]".to_string(),
            multiline: false,
        },
    )];
    let done_ids: std::collections::HashSet<String> =
        ["somethingelse".to_string()].into_iter().collect();

    assert!(super::strike_done_queue_head_prompts(&entries, &done_ids).is_none());
}

#[test]
fn strike_done_queue_prompts_strikes_non_head_resolved_ref() {
    // #ynra: a resolved (done) ref behind a live head must be struck, not
    // left as an orphaned ref (which trips the shadow-backlog guard). The
    // live head is preserved in place.
    let entries = vec![
        crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
            text: "do [#liveone]".to_string(),
            multiline: false,
        }),
        crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
            text: "do [#donetail]".to_string(),
            multiline: false,
        }),
    ];
    let done_ids: std::collections::HashSet<String> =
        ["donetail".to_string()].into_iter().collect();

    let (rewritten, struck) =
        super::strike_done_queue_head_prompts(&entries, &done_ids).expect("expected a strike");
    assert_eq!(struck.len(), 1);
    assert_eq!(struck[0].text, "do [#donetail]");
    // Live head preserved as a Prompt; the trailing resolved ref struck.
    assert!(matches!(
        &rewritten[0],
        crate::queue::QueueEntry::Prompt(p) if p.text == "do [#liveone]"
    ));
    assert!(matches!(
        &rewritten[1],
        crate::queue::QueueEntry::Completed(p) if p.text == "do [#donetail]"
    ));
}

#[test]
fn collect_agent_review_gated_ids_extracts_only_gated_marker() {
    let content = "\
<!-- agent:review -->
- [/] [#alpha] First gated item with a plan reference.
- [x] [#beta] Already-done item in review (legacy).
- [ ] [#charlie] Open item in review — not gated.
- [/] [#delta] [partial] Another gated item.
- [/] no id here.
<!-- /agent:review -->
";
    let ids = super::collect_agent_review_gated_ids(content);
    assert!(
        ids.contains("alpha"),
        "expected gated [/] item to be collected, got {:?}",
        ids
    );
    assert!(
        ids.contains("delta"),
        "expected second gated [/] item to be collected, got {:?}",
        ids
    );
    assert!(
        !ids.contains("beta"),
        "[x] marker is not gated, must not be collected"
    );
    assert!(
        !ids.contains("charlie"),
        "[ ] marker is not gated, must not be collected"
    );
    assert_eq!(
        ids.len(),
        2,
        "only [/] items should be collected: {:?}",
        ids
    );
}

#[test]
fn collect_agent_review_gated_ids_returns_empty_when_no_review_component() {
    let content = "<!-- agent:backlog -->\n- [ ] [#alpha] backlog only\n<!-- /agent:backlog -->\n";
    let ids = super::collect_agent_review_gated_ids(content);
    assert!(ids.is_empty(), "no review component → empty: {:?}", ids);
}

#[test]
fn collect_agent_review_gated_ids_ignores_backlog_open_items() {
    let content = "\
<!-- agent:backlog -->
- [ ] [#openbk] open in backlog
<!-- /agent:backlog -->
<!-- agent:review -->
- [/] [#gatedrv] gated in review
<!-- /agent:review -->
";
    let ids = super::collect_agent_review_gated_ids(content);
    assert!(ids.contains("gatedrv"));
    assert!(
        !ids.contains("openbk"),
        "backlog open items must NOT be collected as gated"
    );
}

#[test]
fn strike_done_queue_head_prompts_strikes_review_gated_items() {
    // Queue head matches a gated `[/]` item in agent:review — auto-strike
    // must advance the queue past it just like an agent:done item.
    let entries = vec![
        crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
            text: "do [#gatedphase]".to_string(),
            multiline: false,
        }),
        crate::queue::QueueEntry::Prompt(crate::queue::QueuePrompt {
            text: "do [#stillopen]".to_string(),
            multiline: false,
        }),
    ];
    let eligible_ids: std::collections::HashSet<String> =
        ["gatedphase".to_string()].into_iter().collect();

    let (rewritten, struck) = super::strike_done_queue_head_prompts(&entries, &eligible_ids)
        .expect("expected gated head to be struck");
    assert_eq!(struck.len(), 1);
    assert_eq!(struck[0].text, "do [#gatedphase]");
    match &rewritten[1] {
        crate::queue::QueueEntry::Prompt(prompt) => {
            assert_eq!(prompt.text, "do [#stillopen]");
        }
        other => panic!("expected live head to remain Prompt, got {:?}", other),
    }
}

#[test]
fn collect_agent_done_ids_extracts_from_done_component() {
    let content = "<!-- agent:done -->\n- [x] [#alpha] One thing\n- [x] [#bravo] Another\n<!-- /agent:done -->\n";
    let ids = super::collect_agent_done_ids(content);
    assert!(ids.contains("alpha"));
    assert!(ids.contains("bravo"));
    assert_eq!(ids.len(), 2);
}

#[test]
fn collect_agent_done_ids_reads_archive_attr_when_present() {
    let dir = TempDir::new().unwrap();
    let archive_rel = "tasks/done-archive.md";
    let archive_path = dir.path().join(archive_rel);
    std::fs::create_dir_all(archive_path.parent().unwrap()).unwrap();
    std::fs::write(
        &archive_path,
        "- [x] [#archived1] First archived item\n- [x] [#archived2] Second\n",
    )
    .unwrap();
    let content = format!(
        "<!-- agent:done archive={} -->\n<!-- /agent:done -->\n",
        archive_rel
    );
    let ids = super::collect_agent_done_ids_with_root(&content, Some(dir.path()));
    assert!(
        ids.contains("archived1"),
        "expected ids to include archived1 from archive file: {:?}",
        ids
    );
    assert!(ids.contains("archived2"));
    // Without the root, the archive path cannot be resolved → empty.
    let ids_no_root = super::collect_agent_done_ids(&content);
    assert!(ids_no_root.is_empty());
}

#[test]
fn queue_prompt_done_id_parses_canonical_bracket_form() {
    assert_eq!(
        super::queue_prompt_done_id("do [#jbrsrbusyint]"),
        Some("jbrsrbusyint".to_string())
    );
    assert_eq!(
        super::queue_prompt_done_id("do #jbrsrbusyint more text"),
        Some("jbrsrbusyint".to_string())
    );
    assert_eq!(super::queue_prompt_done_id("plain prompt"), None);
}

struct EnvGuard {
    key: &'static str,
    prev: Option<String>,
    _lock: std::sync::MutexGuard<'static, ()>,
}

impl EnvGuard {
    fn set(key: &'static str, value: &str) -> Self {
        let lock = crate::harness_prompt::TEST_ENV_LOCK.lock().unwrap();
        let prev = std::env::var(key).ok();
        unsafe { std::env::set_var(key, value) };
        Self {
            key,
            prev,
            _lock: lock,
        }
    }
}

impl Drop for EnvGuard {
    fn drop(&mut self) {
        if let Some(value) = &self.prev {
            unsafe { std::env::set_var(self.key, value) };
        } else {
            unsafe { std::env::remove_var(self.key) };
        }
    }
}

/// Set up a minimal project directory with .agent-doc/ structure and a git repo.
fn setup_project() -> TempDir {
    let dir = TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/snapshots")).unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/pending")).unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/locks")).unwrap();

    // Initialize a bare git repo so `git commit` doesn't fail fatally.
    Command::new("git")
        .current_dir(dir.path())
        .args(["init"])
        .output()
        .ok();
    Command::new("git")
        .current_dir(dir.path())
        .args(["config", "user.email", "test@test.com"])
        .output()
        .ok();
    Command::new("git")
        .current_dir(dir.path())
        .args(["config", "user.name", "Test"])
        .output()
        .ok();

    dir
}

fn commit_all(root: &Path, message: &str, commit_date: Option<&str>) {
    Command::new("git")
        .current_dir(root)
        .args(["add", "."])
        .output()
        .unwrap();
    let mut commit = Command::new("git");
    commit
        .current_dir(root)
        .args(["commit", "-m", message, "--no-verify"]);
    if let Some(date) = commit_date {
        commit
            .env("GIT_COMMITTER_DATE", date)
            .env("GIT_AUTHOR_DATE", date);
    }
    let output = commit.output().unwrap();
    assert!(
        output.status.success(),
        "git commit {message:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

fn initialize_git_head(root: &Path) {
    let readme = root.join("README.md");
    std::fs::write(&readme, "# project\n").unwrap();
    commit_all(root, "initial", None);
}

fn write_committed_doc(
    root: &Path,
    rel: &str,
    content: &str,
    message: &str,
    commit_date: Option<&str>,
) -> PathBuf {
    let doc = root.join(rel);
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();
    commit_all(root, message, commit_date);
    doc
}

fn write_sessions_json(root: &Path, entries: &[(&str, &str, &Path, &str, &str)]) {
    let mut sessions = serde_json::Map::new();
    for (session_id, pane, file, window, started) in entries {
        sessions.insert(
            (*session_id).to_string(),
            serde_json::json!({
                "pane": pane,
                "pid": 9999,
                "cwd": root.to_string_lossy(),
                "started": started,
                "file": file.strip_prefix(root).unwrap().to_string_lossy(),
                "window": window
            }),
        );
    }
    std::fs::write(
        root.join(".agent-doc/sessions.json"),
        serde_json::to_string_pretty(&serde_json::Value::Object(sessions)).unwrap(),
    )
    .unwrap();
}

fn age_cycle_state(file: &Path, age_secs: u64) {
    let canonical = file.canonicalize().unwrap();
    let root = crate::snapshot::find_project_root(&canonical).unwrap();
    let hash = crate::snapshot::doc_hash(&canonical).unwrap();
    let path = root
        .join(".agent-doc/state/cycles")
        .join(format!("{hash}.json"));
    let mut state: crate::cycle_state::CycleState =
        serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    state.started_at = state.started_at.saturating_sub(age_secs);
    state.updated_at = state.updated_at.saturating_sub(age_secs);
    std::fs::write(path, serde_json::to_string_pretty(&state).unwrap()).unwrap();
}

fn write_cycles_log(doc: &Path, entries: &[crate::ops_log::CycleEntry]) {
    let log_path = doc.parent().unwrap().join(".agent-doc/logs/cycles.jsonl");
    std::fs::create_dir_all(log_path.parent().unwrap()).unwrap();
    let mut file = std::fs::File::create(log_path).unwrap();
    for entry in entries {
        writeln!(file, "{}", serde_json::to_string(entry).unwrap()).unwrap();
    }
}

#[test]
fn preflight_produces_valid_json() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello\n").unwrap();

    // Snapshot matches document → no_changes = true.
    snapshot::save(&doc, &std::fs::read_to_string(&doc).unwrap()).unwrap();

    run(&doc).unwrap();
    // If run() returns Ok(()), the JSON was printed to stdout without error.
    // The test verifies no panic and no error return.
}

#[test]
fn preflight_fails_closed_when_required_ssh_doc_mapping_resolves_no_targets() {
    let dir = setup_project();
    std::fs::create_dir_all(dir.path().join("tasks")).unwrap();
    std::fs::write(
        dir.path().join(".agent-doc/config.toml"),
        "[ssh.docs.\"tasks/monsterrodholders.md\"]\nprofile = \"missing\"\n",
    )
    .unwrap();
    let doc = dir.path().join("tasks/monsterrodholders.md");
    std::fs::write(&doc, "---\nagent: codex\n---\n\n## User\n\nHello\n").unwrap();

    let err = run(&doc).unwrap_err();
    assert!(err.to_string().contains("requires SSH profile `missing`"));
}

#[test]
fn preflight_fails_closed_on_uncommitted_closeout_drift_even_without_diff() {
    let dir = setup_project();
    let root = dir.path();
    std::fs::create_dir_all(root.join("news/2026-05-01")).unwrap();

    let doc = root.join("session.md");
    let news_index = root.join("news/README.md");
    let news_day = root.join("news/2026-05-01/README.md");
    let old_doc = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nold body\n<!-- /agent:exchange -->\n";
    std::fs::write(&doc, old_doc).unwrap();
    std::fs::write(&news_index, "old news index\n").unwrap();
    std::fs::write(&news_day, "old news day\n").unwrap();
    snapshot::save(&doc, old_doc).unwrap();
    Command::new("git")
        .current_dir(root)
        .args([
            "add",
            "session.md",
            "news/README.md",
            "news/2026-05-01/README.md",
        ])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .output()
        .unwrap();

    let new_doc = "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n## Exchange\n\n<!-- agent:exchange patch=append -->\nold body\n### Re: create today's news — codex\nresponse\n<!-- /agent:exchange -->\n";
    std::fs::write(&doc, new_doc).unwrap();
    snapshot::save(&doc, new_doc).unwrap();
    std::fs::write(&news_index, "new news index\n").unwrap();
    std::fs::write(&news_day, "new news day\n").unwrap();

    let err = run(&doc).expect_err("preflight should fail before diffing hidden closeout drift");
    let message = err.to_string();
    assert!(message.contains("snapshot differs from HEAD"));
    assert!(message.contains("tracked side-effect edits"));
    assert!(message.contains("news/README.md"));
    assert!(message.contains("news/2026-05-01/README.md"));
    assert!(message.contains("agent-doc write --commit"));
}

#[test]
fn preflight_fails_closed_on_uncommitted_exchange_drift_without_response_heading() {
    let dir = setup_project();
    let root = dir.path();

    let doc = root.join("monsterrodholders.md");
    let committed = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ deploy v0.4.9\n",
        "### Re: shopcozi mobile CSS fix — glm-5.1\n\n",
        "Patched the mobile CSS and deployed v0.4.9.\n",
        "<!-- /agent:exchange -->\n",
    );
    std::fs::write(&doc, committed).unwrap();
    snapshot::save(&doc, committed).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "monsterrodholders.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .output()
        .unwrap();

    let dirty = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ deploy v0.4.9\n",
        "### Re: shopcozi mobile CSS fix — glm-5.1\n\n",
        "Patched the mobile CSS and deployed v0.4.9.\n\n",
        "Verification:\n",
        "- npm test\n",
        "- docker compose run post-deploy\n",
        "<!-- /agent:exchange -->\n",
    );
    std::fs::write(&doc, dirty).unwrap();

    let err = run(&doc).expect_err("preflight should block uncommitted exchange drift");
    let message = err.to_string();
    assert!(message.contains("uncommitted exchange changes"));
    assert!(message.contains("agent-doc write --commit"));
    assert!(
        !message.contains("snapshot differs from HEAD"),
        "body-only exchange drift should be diagnosed before generic snapshot drift: {message}"
    );
}

#[test]
fn preflight_file_not_found() {
    let err = run(Path::new("/nonexistent/missing.md")).unwrap_err();
    assert!(err.to_string().contains("file not found"));
}

#[test]
fn preflight_detects_diff() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
    std::fs::write(&doc, original).unwrap();

    // Save snapshot of original, then add new content.
    snapshot::save(&doc, original).unwrap();
    std::fs::write(
        &doc,
        "---\nsession: test\n---\n\n## User\n\nHello\n\nNew question here.\n",
    )
    .unwrap();

    // diff::compute should detect changes → no_changes = false.
    let diff_result = diff::compute(&doc).unwrap();
    assert!(diff_result.is_some(), "diff should detect new content");
}

#[test]
fn preflight_closes_stale_starting_actors_even_when_daily_gc_stamp_is_fresh() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = "---\nagent_doc_session: test\n---\n\n## User\n\nHello\n";
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();
    std::fs::write(dir.path().join(".agent-doc/gc.stamp"), "").unwrap();

    let stale_doc = dir.path().join("tasks/stale-starting.md");
    std::fs::create_dir_all(stale_doc.parent().unwrap()).unwrap();
    std::fs::write(&stale_doc, "body").unwrap();
    let stale_record = crate::session_actor::ActorRecord {
        document_id: stale_doc.to_string_lossy().to_string(),
        session_id: "session-stale-starting".to_string(),
        generation: 1,
        pane_id: "%71".to_string(),
        window_id: "@7".to_string(),
        harness: "codex".to_string(),
        state: crate::session_actor::ActorState::Starting,
        last_transition: crate::session_actor::ActorLastTransition {
            caller: "start".to_string(),
            reason: "session_start".to_string(),
            timestamp: 1,
            prior_generation: 0,
            new_generation: 1,
        },
    };
    crate::project_controller::store_actor_record(dir.path(), Some(0), &stale_record).unwrap();

    run(&doc).unwrap();

    let updated =
        crate::project_controller::load_actor_record(dir.path(), &stale_record.document_id)
            .unwrap()
            .unwrap();
    assert_eq!(updated.state, crate::session_actor::ActorState::Closed);
    assert_eq!(updated.last_transition.caller, "preflight");
    assert_eq!(updated.last_transition.reason, "stale_starting_actor");
}

#[test]
fn preflight_opens_cycle_from_harness_prompt_when_document_has_no_diff() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "prompt_presets:\n",
        "  '#code-review': Please review the codebase. '#follow-up-backlog'\n",
        "  '#follow-up-backlog': Any follow-up items to place in the backlog?\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let _prompt = EnvGuard::set(
        "AGENT_DOC_HARNESS_PROMPT",
        &format!("agent-doc {} #code-review", doc.display()),
    );

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
    assert!(
        state.requires_backlog_capture,
        "harness prompt preset expansion should record backlog capture requirement"
    );
}

#[test]
fn preflight_opens_cycle_from_active_queue_when_document_has_no_diff() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#oobpmt]\n",
        "<!-- /agent:queue -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#oobpmt] Fix OOB prompt absorption.\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted,
        "active queue prompt should open a cycle even when the file matches the snapshot"
    );
}

#[test]
fn preflight_does_not_open_cycle_from_active_queue_slash_command() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "-   /clear  \n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run(&doc).unwrap();

    if let Some(state) = crate::cycle_state::load(&doc).unwrap() {
        assert!(
            !state.is_open(),
            "slash-only active queue heads must be supervisor handoffs, not response cycles: {:?}",
            state
        );
    }
    assert_eq!(
        crate::queue_continuation::detect(&doc)
            .unwrap()
            .map(|continuation| continuation.head_prompt),
        Some("  /clear  ".to_string()),
        "the literal queue head must stay live for the supervisor"
    );
}

#[test]
fn preflight_probe_does_not_open_cycle_even_with_dispatchable_diff() {
    // #preflight-probe-side-effect-free: the SAME active-queue input that
    // opens a `preflight_started` cycle in the dispatch path (see
    // `preflight_opens_cycle_from_active_queue_when_document_has_no_diff`)
    // must leave NO open cycle when run as a pure inspection probe, so a
    // diagnostic preflight never wedges a later `session-check`.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#oobpmt]\n",
        "<!-- /agent:queue -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#oobpmt] Fix OOB prompt absorption.\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run_with_options(&doc, PreflightOptions { probe: true }).unwrap();

    // The probe must not leave an OPEN cycle (`preflight_started` /
    // `response_captured` / `write_applied`) — that is the state that wedges
    // a later `session-check`. A terminal `committed`/`abandoned` cycle from
    // the (idempotent) commit step is acceptable.
    if let Some(state) = crate::cycle_state::load(&doc).unwrap() {
        assert!(
            matches!(
                state.phase,
                crate::cycle_state::CyclePhase::Committed
                    | crate::cycle_state::CyclePhase::Abandoned
            ),
            "a probe preflight must not leave an open cycle, got {:?}",
            state.phase
        );
    }
}

#[test]
fn run_queue_maintenance_syncs_backlog_into_empty_queue() {
    // #backlog-queue-sync-attr: a backlog carrying `queue=sync` regenerates
    // the (empty) queue with `do [#id]` for active items; gated/done excluded.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=sync -->\n",
        "- [ ] [#alpha] first\n",
        "- [/] [#gated] blocked\n",
        "- [ ] [#beta] second\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(
        state.synced_queue_ids,
        vec!["alpha".to_string(), "beta".to_string()]
    );
    assert!(
        updated.contains("- do [#alpha]"),
        "synced queue:\n{updated}"
    );
    assert!(updated.contains("- do [#beta]"));
    assert!(
        !updated.contains("- do [#gated]"),
        "gated item must not be queued:\n{updated}"
    );
    assert!(
        state
            .warnings
            .iter()
            .any(|w| w.code == "backlog_queue_sync_pending"),
        "empty-queue-before-sync must emit backlog_queue_sync_pending warning, got {:?}",
        state.warnings
    );
}

#[test]
fn run_queue_maintenance_does_not_sync_icebox_into_empty_queue() {
    // Parked icebox work must not become the next active prompt just because the
    // queue and backlog are drained. Move the item to backlog or mark the item
    // with an explicit enqueue token when it should run.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue: go\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=append -->\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:icebox queue=append -->\n",
        "- [ ] [#parked] parked follow-up\n",
        "<!-- /agent:icebox -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();
    let updated = std::fs::read_to_string(&doc).unwrap();

    assert!(
        !updated.contains("- do [#parked]"),
        "icebox queue attr must not auto-populate a drained queue:\n{updated}"
    );
    assert!(
        state.synced_queue_ids.is_empty(),
        "icebox ids must not be reported as synced queue ids: {:?}",
        state.synced_queue_ids
    );
}

#[test]
fn run_queue_maintenance_enqueue_marker_populates_queue_without_backlog_attr() {
    // #queue-enqueue-action: a single marked backlog item appends to the
    // queue without a component-level `queue` attr. Explicit markers bypass
    // the active-loop fresh-item hold because the user is directly enqueueing
    // that one id.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#running]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#alpha] :inbox_tray: queue this now\n",
        "- [ ] [#beta] leave this unqueued\n",
        "- [/] [#gated] :inbox_tray: blocked\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();
    let updated = std::fs::read_to_string(&doc).unwrap();

    assert_eq!(state.synced_queue_ids, vec!["alpha".to_string()]);
    assert!(
        updated.contains("- do [#running]"),
        "running head stays:\n{updated}"
    );
    assert!(
        updated.contains("- do [#alpha]"),
        "marked item should append:\n{updated}"
    );
    assert!(
        !updated.contains("- do [#beta]"),
        "unmarked item must not append:\n{updated}"
    );
    assert!(
        !updated.contains("- do [#gated]"),
        "gated marked item must not append:\n{updated}"
    );
}

#[test]
fn run_queue_maintenance_holds_fresh_backlog_item_out_of_active_queue() {
    // #backlog-queue-sync-pending-add-amplification (decision B/C): a backlog
    // item added while the auto-queue is already running (queue_active: true)
    // must NOT be promoted into the live queue this cycle — it waits for the
    // next activation. Prevents unbounded queue growth + pending_done_guard
    // churn when an agent captures follow-ups mid-loop.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=append -->\n",
        "- [ ] [#alpha] already running\n",
        "- [ ] [#beta] freshly added mid-loop\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();
    let updated = std::fs::read_to_string(&doc).unwrap();

    assert!(
        updated.contains("- do [#alpha]"),
        "the already-running head stays:\n{updated}"
    );
    assert!(
        !updated.contains("- do [#beta]"),
        "a freshly-added backlog item must NOT be promoted into the active queue mid-loop:\n{updated}"
    );
    assert!(
        !state.synced_queue_ids.contains(&"beta".to_string()),
        "beta must not be a newly-synced queue id while the loop is active: {:?}",
        state.synced_queue_ids
    );
}

#[test]
fn run_queue_maintenance_go_mode_repopulates_drained_active_queue() {
    // #backlog-queue-empty-active-repopulate: with the `go` control
    // (`queue: go`, continuous-backlog-loop) and a fully drained live queue
    // (0 un-struck prompts), the amplification hold is skipped and the full
    // active backlog repopulates the queue so the loop keeps working it.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue: go\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=append -->\n",
        "- [ ] [#alpha] first\n",
        "- [ ] [#beta] second\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();
    let updated = std::fs::read_to_string(&doc).unwrap();

    assert!(
        updated.contains("- do [#alpha]"),
        "go-mode must repopulate a drained active queue:\n{updated}"
    );
    assert!(
        updated.contains("- do [#beta]"),
        "go-mode must repopulate ALL open backlog ids:\n{updated}"
    );
    assert!(
        state.synced_queue_ids.contains(&"alpha".to_string())
            && state.synced_queue_ids.contains(&"beta".to_string()),
        "both ids must be newly synced under go-mode repopulation: {:?}",
        state.synced_queue_ids
    );
}

#[test]
fn run_queue_maintenance_go_mode_appends_fresh_backlog_into_nondrained_queue() {
    // #backlog-queue-attr-populates-in-go-mode: with the `go` control and a
    // NON-drained live queue, a freshly-added backlog `queue`-attr item still
    // appends to the queue immediately (the operator opted into the
    // continuous-backlog-loop, so the `queue` attribute must populate it).
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue: go\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=append -->\n",
        "- [ ] [#alpha] already running\n",
        "- [ ] [#beta] freshly added mid-loop\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let updated_state = run_queue_maintenance(&doc, None).unwrap();
    let updated = std::fs::read_to_string(&doc).unwrap();

    assert!(
        updated.contains("- do [#alpha]"),
        "the running head stays:\n{updated}"
    );
    assert!(
        updated.contains("- do [#beta]"),
        "go-mode must append a fresh backlog `queue`-attr item even when the queue is not drained:\n{updated}"
    );
    assert!(
        updated_state.synced_queue_ids.contains(&"beta".to_string()),
        "beta must be a newly-synced queue id under go-mode: {:?}",
        updated_state.synced_queue_ids
    );
}

#[test]
fn run_queue_maintenance_normalizes_boolean_true_queue_attrs() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue priority=true preset=\"#spec-test-build-install-commit-push\"=true go=true -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#alpha] run the alpha task\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();
    let updated = std::fs::read_to_string(&doc).unwrap();

    assert_eq!(state.queue_active, Some(true));
    assert!(
        updated.contains(
            "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->"
        ),
        "queue tag should be canonical:\n{updated}"
    );
    assert!(
        !updated.contains("=true"),
        "malformed attrs repaired:\n{updated}"
    );

    let snap = snapshot::load(&doc).unwrap().unwrap();
    assert!(
        snap.contains(
            "<!-- agent:queue priority preset=\"#spec-test-build-install-commit-push\" go -->"
        ),
        "snapshot queue tag should be canonical:\n{snap}"
    );
    assert!(
        !snap.contains("=true"),
        "snapshot malformed attrs repaired:\n{snap}"
    );
}

#[test]
fn run_queue_maintenance_no_go_keeps_drain_then_stop_on_empty_active_queue() {
    // #backlog-queue-empty-active-repopulate: WITHOUT the `go` control, a
    // drained persisted-active queue stays drained (drain-then-stop). The
    // amplification hold drops every backlog id because none are already
    // live queue heads, so nothing repopulates.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=append -->\n",
        "- [ ] [#alpha] first\n",
        "- [ ] [#beta] second\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();
    let updated = std::fs::read_to_string(&doc).unwrap();

    assert!(
        !updated.contains("- do [#alpha]") && !updated.contains("- do [#beta]"),
        "without `go`, a drained active queue must stay drained:\n{updated}"
    );
    assert!(
        state.synced_queue_ids.is_empty(),
        "no ids may be synced into a drained active queue without `go`: {:?}",
        state.synced_queue_ids
    );
}

#[test]
fn run_queue_maintenance_no_warning_when_queue_already_synced() {
    // When the queue already matches the backlog, no backlog_queue_sync_pending
    // warning should fire (sync_backlog_into_queue returns None → no warning path).
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=sync -->\n",
        "- [ ] [#alpha] first\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert!(
        !state
            .warnings
            .iter()
            .any(|w| w.code == "backlog_queue_sync_pending"),
        "already-synced queue must NOT emit backlog_queue_sync_pending warning, got {:?}",
        state.warnings
    );
}

#[test]
fn run_queue_maintenance_marker_go_activates_like_auto() {
    // #queue-state-unify: a `go`/`start` marker control freshly activates the
    // queue through the Auto trigger, identical to the legacy `auto` attribute.
    for token in ["go", "start"] {
        let dir = setup_project();
        let doc = dir.path().join("session.md");
        let content = format!(
            concat!(
                "---\nagent_doc_session: test\nagent_doc_format: template\n",
                "agent_doc_write: crdt\n---\n\n",
                "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
                "<!-- /agent:exchange -->\n\n",
                "<!-- agent:queue {} -->\n- please do the thing\n<!-- /agent:queue -->\n",
            ),
            token
        );
        std::fs::write(&doc, &content).unwrap();
        snapshot::save(&doc, &content).unwrap();

        let state = run_queue_maintenance(&doc, None).unwrap();
        assert_eq!(
            state.queue_active,
            Some(true),
            "marker `{token}` must activate the queue"
        );
        assert_eq!(state.queue_trigger, Some(crate::queue::QueueTrigger::Auto));
        let updated = std::fs::read_to_string(&doc).unwrap();
        assert!(
            updated.contains("queue: start"),
            "marker `{token}` must persist queue_active:\n{updated}"
        );
    }
}

#[test]
fn run_queue_maintenance_marker_stop_halts_active_queue() {
    // #queue-state-unify: a `stop` marker control forces an otherwise-active
    // queue inactive and clears persisted queue_active.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n",
        "agent_doc_write: crdt\nqueue_active: true\n---\n\n",
        "<!-- agent:exchange patch=append -->\n### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue stop -->\n- please do the thing\n<!-- /agent:queue -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();
    assert_eq!(
        state.queue_active,
        Some(false),
        "marker `stop` must halt the active queue"
    );
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("queue: stop"),
        "marker `stop` must clear queue_active:\n{updated}"
    );
    assert!(
        !updated.contains("agent:queue stop"),
        "marker `stop` token must be stripped after halt:\n{updated}"
    );
}

#[test]
fn run_queue_maintenance_excludes_done_ids_from_backlog_sync() {
    // #ynra: a lingering active backlog `[ ]` bullet whose id is also archived
    // in `agent:done` must NOT be re-minted into the queue (it would be struck
    // every cycle and re-injected the next → forever churn). The fresh active
    // id is still minted.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=sync -->\n",
        "- [ ] [#na3x] completed-but-lingering\n",
        "- [ ] [#fresh] genuinely open\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done -->\n",
        "- 2026-06-01 [#na3x] completed-but-lingering\n",
        "<!-- /agent:done -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        !updated.contains("[#na3x]") || !updated.contains("do [#na3x]"),
        "completed id must not be minted into the queue:\n{updated}"
    );
    assert!(
        !updated.contains("do [#na3x]"),
        "completed id must not appear as a queue do-prompt:\n{updated}"
    );
    assert!(
        updated.contains("do [#fresh]"),
        "fresh active id must still be queued:\n{updated}"
    );
    assert_eq!(state.synced_queue_ids, vec!["fresh".to_string()]);
}

#[test]
fn run_queue_maintenance_excludes_external_archive_done_ids() {
    // #ynra (external-archive variant): a completed id reaped to the EXTERNAL
    // `agent:done archive=<file>` (not inline) must also be excluded from the
    // backlog→queue sync and struck from the queue. Done-id collection reads
    // the archive file, so the queue must not churn on an externally-archived
    // completed ref.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let archive_rel = "session.done.md";
    std::fs::write(
        dir.path().join(archive_rel),
        "# Done\n\n- 2026-06-01 [#extdone] archived externally\n",
    )
    .unwrap();
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#extdone]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=sync -->\n",
        "- [ ] [#extdone] lingering active dup of an externally-archived id\n",
        "- [ ] [#fresh] genuinely open\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done archive=session.done.md -->\n",
        "<!-- /agent:done -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run_queue_maintenance(&doc, None).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        !updated.contains("- do [#extdone]"),
        "externally-archived completed ref must be struck/excluded, not left live:\n{updated}"
    );
    assert!(
        updated.contains("do [#fresh]"),
        "fresh active id must still be queued:\n{updated}"
    );
}

#[test]
fn run_queue_maintenance_strikes_external_archive_done_queue_prompt() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    std::fs::write(
        dir.path().join("session.done.md"),
        "# Done\n\n- 2026-06-01 [#extdone] archived externally\n",
    )
    .unwrap();
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:queue -->\n",
        "- do [#extdone]\n",
        "- do [#fresh]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:done archive=session.done.md -->\n",
        "<!-- /agent:done -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run_queue_maintenance(&doc, None).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("- ~~do [#extdone]~~"),
        "externally-archived live queue mirror must be struck:\n{updated}"
    );
    assert!(
        updated.contains("- do [#fresh]"),
        "fresh live queue prompt must remain:\n{updated}"
    );
}

#[test]
fn run_queue_maintenance_backlog_sync_is_idempotent() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:queue -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=append -->\n",
        "- [ ] [#alpha] first\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        state.synced_queue_ids.is_empty(),
        "idempotent sync should not report freshly-added ids"
    );
    assert_eq!(
        updated.matches("- do [#alpha]").count(),
        1,
        "append must not duplicate an already-queued id:\n{updated}"
    );
}

#[test]
fn run_queue_maintenance_records_only_newly_synced_ids() {
    // The existing queue head must stay outside the synced-id exclusion set
    // so pending_done_guard still requires the consumed `do [#worked]` item
    // to be done/gated.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#worked]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=prepend -->\n",
        "- [ ] [#worked] the real queue head\n",
        "- [ ] [#alpha] freshly synced\n",
        "- [ ] [#beta] freshly synced\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(
        state.synced_queue_ids,
        vec!["alpha".to_string(), "beta".to_string()]
    );
    let open_backlog: std::collections::HashSet<String> = ["worked", "alpha", "beta"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    let synced_queue_ids = state
        .synced_queue_ids
        .iter()
        .cloned()
        .collect::<std::collections::HashSet<String>>();
    let result = filter_expect_done_or_gate_ids(
        &[
            "worked".to_string(),
            "alpha".to_string(),
            "beta".to_string(),
        ],
        &open_backlog,
        &synced_queue_ids,
    );
    assert_eq!(result, vec!["worked".to_string()]);
}

#[test]
fn run_queue_maintenance_backlog_queue_priority_sorts_and_marks_promoted_item() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:queue auto -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=sync priority -->\n",
        "- [ ] [#slow] slower follow-up priority=9\n",
        "- [ ] [#fast] fast follow-up priority=1\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(state.queue_active, Some(true));
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("- :round_pushpin: do [#fast]\n- do [#slow]"),
        "backlog `queue priority` must sort synced queue prompts and mark the promoted item:\n{updated}"
    );
}

#[test]
fn run_queue_maintenance_pins_operator_moved_priority_queue_item() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let snapshot_content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:queue priority auto -->\n",
        "- do [#fast]\n",
        "- do [#medium]\n",
        "- do [#slow]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog priority -->\n",
        "- [ ] [#fast] priority=1 first by rank\n",
        "- [ ] [#medium] priority=5 middle by rank\n",
        "- [ ] [#slow] priority=9 operator moved this up\n",
        "<!-- /agent:backlog -->\n",
    );
    let current_content = snapshot_content.replace(
        "- do [#fast]\n- do [#medium]\n- do [#slow]",
        "- do [#slow]\n- do [#fast]\n- do [#medium]",
    );
    std::fs::write(&doc, &current_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(state.queue_active, Some(true));
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("- :pushpin: do [#slow]\n- do [#fast]\n- do [#medium]"),
        "operator-moved queue prompt should become sticky with :pushpin::\n{updated}"
    );
}

#[test]
fn run_queue_maintenance_auto_dag_intersperses_blocker_with_pinned_batch() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:queue priority auto -->\n",
        "- :pushpin: do [#ops]\n",
        "- :pushpin: do [#ship]\n",
        "- :pushpin: do [#notify]\n",
        "- :round_pushpin: do [#setup]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog priority -->\n",
        "- [ ] [#ops] priority=5 independent operator-pinned task\n",
        "- [ ] [#ship] priority=1 after=#setup depends on setup\n",
        "- [ ] [#notify] priority=2 after=#ship depends on ship\n",
        "- [ ] [#setup] priority=9 required setup work\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(state.queue_active, Some(true));
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains(
            "- :pushpin: do [#ops]\n\
                 - :round_pushpin: do [#setup]\n\
                 - :pushpin: do [#ship]\n\
                 - :pushpin: do [#notify]"
        ),
        "auto-dag must let dependency blockers intersperse a pinned batch:\n{updated}"
    );
}

#[test]
fn preflight_new_auto_queue_from_inactive_snapshot_does_not_halt_on_changed_head() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let snapshot_content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "dispatch #spec-test-build-install-commit-push\n",
        "- do [#oldhead]\n",
        "<!-- /agent:queue -->\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#newhead] Run the newly queued head.\n",
        "- [ ] [#nexthead] Run the next queued item.\n",
        "<!-- /agent:backlog -->\n"
    );
    let current_content = snapshot_content
        .replace("<!-- agent:queue -->", "<!-- agent:queue auto -->")
        .replace("- do [#oldhead]", "- do [#newhead]\n- do [#nexthead]");
    std::fs::write(&doc, &current_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(state.queue_active, Some(true));
    assert_eq!(state.queue_halted, None);
    assert_eq!(
        state.queue_prompts,
        vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()]
    );

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("queue: start"));
    assert!(updated.contains("<!-- agent:queue auto -->"));
    assert!(updated.contains("- do [#newhead]"));
    assert!(!updated.contains("- do [#oldhead]"));

    let snap = snapshot::load(&doc).unwrap().unwrap();
    assert!(
        snap.contains("queue: start")
            && snap.contains("<!-- agent:queue auto -->")
            && snap.contains("- do [#newhead]")
            && !snap.contains("- do [#oldhead]"),
        "newly activated queue must be snapshotted as the closeout baseline:\n{snap}"
    );

    let done_ids = vec!["newhead".to_string()];
    let outcome = crate::write::consume_queue_prompts_for_done_ids_with_outcome(&doc, &done_ids)
        .unwrap()
        .expect("newly activated queue head should be consumable");
    assert_eq!(outcome.consumed_count, 1);
    assert_eq!(outcome.remaining, 1);

    let consumed = std::fs::read_to_string(&doc).unwrap();
    assert!(consumed.contains("- ~~do [#newhead]~~"));
    assert!(consumed.contains("- do [#nexthead]"));
}

#[test]
fn queue_maintenance_drains_all_done_queue_without_item_modified_halt() {
    // #drained-done-queue-clear: a fully resolved auto-queue (every `do
    // [#id]` already in agent:done) plus a batch dispatch directive must
    // drain — not false-halt as `item_modified`. Before the fix the
    // strike pass converted every live head to Completed, leaving the
    // post-strike head `None` vs a still-live snapshot head, which
    // detect_head_prompt_modified read as an edit and halted before the
    // drain-cleanup path ran. The Corky live-repro shape: template doc,
    // dispatch preset, multiple bracketed `do [#id]` prompts, no diff.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "dispatch #spec-test-build-install-commit-push\n",
        "- do [#alpha]\n",
        "- do [#beta]\n",
        "<!-- /agent:queue -->\n\n",
        "## Completed / Reaped\n\n",
        "<!-- agent:done -->\n",
        "- [x] [#alpha] First done.\n",
        "- [x] [#beta] Second done.\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(
        state.queue_halted, None,
        "fully-resolved queue must drain, not halt as item_modified"
    );
    assert_eq!(state.queue_active, Some(false));
    assert!(state.queue_prompts.is_empty());

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("queue: stop"), "file: {updated}");
    assert!(
        !updated.contains("agent:queue auto"),
        "auto must be stripped on drain: {updated}"
    );
    assert!(
        !updated.contains("- do [#alpha]") && !updated.contains("- do [#beta]"),
        "drained queue body must be cleared: {updated}"
    );

    // Snapshot matches the drained file so the closeout commit boundary
    // does not strand the maintenance mutation.
    let snap = snapshot::load(&doc).unwrap().unwrap();
    assert!(snap.contains("queue: stop"));
    assert!(!snap.contains("agent:queue auto"));
    assert!(!snap.contains("- do [#alpha]"));
}

#[test]
fn queue_maintenance_partial_done_strike_advances_to_live_head_without_halt() {
    // #drained-done-queue-clear (partial case): a leading queue head that
    // is already done must be struck and the queue advanced to the next
    // live head — without false-halting as item_modified. The snapshot is
    // struck the same way before the head-modified comparison so only a
    // genuine operator head edit can halt.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#alpha]\n",
        "- do [#beta]\n",
        "<!-- /agent:queue -->\n\n",
        "## Completed / Reaped\n\n",
        "<!-- agent:done -->\n",
        "- [x] [#alpha] First done.\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(
        state.queue_halted, None,
        "striking a done head must not halt while a live head remains"
    );
    assert_eq!(state.queue_active, Some(true));
    assert_eq!(state.queue_prompts, vec!["do [#beta]".to_string()]);

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("- ~~do [#alpha]~~"),
        "done head struck to completed: {updated}"
    );
    assert!(updated.contains("- do [#beta]"));
    assert!(updated.contains("agent:queue auto"));
    assert!(updated.contains("queue_active: true"));
}

#[test]
fn queue_maintenance_converges_live_ipc_buffer_on_item_modified_halt() {
    // SimWorld repro for #adoc-queue-ipc-buffer-divergence (root cause #2):
    // a live route-owned IPC listener owns the document. When an
    // already-active auto-queue's head prompt changes between cycles, queue
    // maintenance halts (item_modified), strips `auto`, and clears
    // `queue_active` on disk + snapshot. Without convergence the live editor
    // buffer would re-add `auto`/`queue_active: true` on its next flush and
    // the snapshot/HEAD drift loop regenerates every preflight. This test
    // proves maintenance pushes a queue-tag + frontmatter convergence message
    // to the listener, and that a follow-up maintenance pass is idempotent
    // (no second divergence, no second convergence send).
    use std::sync::{Arc, Mutex};

    let dir = setup_project();
    let root = dir.path().canonicalize().unwrap();
    let doc = root.join("session.md");

    let received: Arc<Mutex<Vec<serde_json::Value>>> = Arc::new(Mutex::new(Vec::new()));
    let received_clone = received.clone();
    let listener_root = root.clone();
    let server = std::thread::spawn(move || {
        crate::ipc_socket::start_listener(&listener_root, move |msg| {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(msg) {
                received_clone.lock().unwrap().push(v);
            }
            Some(serde_json::json!({"type": "ack", "status": "ok"}).to_string())
        })
        .ok();
    });
    std::thread::sleep(std::time::Duration::from_millis(150));

    let snapshot_content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#oldhead]\n",
        "- do [#nexthead]\n",
        "<!-- /agent:queue -->\n"
    );
    let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
    std::fs::write(&doc, &current_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    // A live editor is actively mid-edit on the head prompt, so the loop
    // must still pause/halt rather than adopt a half-typed head
    // (#queue-no-stall-on-head-edit gates adopt on a settled buffer).
    crate::debounce::document_changed(&doc.to_string_lossy());

    let state = run_queue_maintenance(&doc, None).unwrap();
    assert_eq!(state.queue_halted, Some("item_modified".into()));
    assert_eq!(state.queue_active, Some(false));

    // Disk converged to the inactive shape.
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("<!-- agent:queue -->"));
    assert!(!updated.contains("agent:queue auto"));
    assert!(updated.contains("queue: stop"));

    // Listener received exactly one queue convergence message carrying the
    // queue body plus the tag + frontmatter shape that a content-only patch
    // cannot deliver.
    std::thread::sleep(std::time::Duration::from_millis(100));
    {
        let msgs = received.lock().unwrap();
        let convergences: Vec<&serde_json::Value> = msgs
            .iter()
            .filter(|m| m.get("queue_auto").is_some())
            .collect();
        assert_eq!(
            convergences.len(),
            1,
            "expected exactly one queue convergence message, got: {msgs:?}"
        );
        let conv = convergences[0];
        assert_eq!(conv["queue_auto"], serde_json::json!(false));
        // #queue-active-deprecated-line-stuck: convergence carries the
        // canonical `queue:` control, never the deprecated `queue_active:`.
        assert_eq!(conv["frontmatter"], serde_json::json!("queue: stop"));
        assert_eq!(conv["patches"][0]["component"], serde_json::json!("queue"));
        assert_eq!(
            conv["patches"][0]["content"],
            serde_json::json!(
                crate::component::parse(&updated)
                    .unwrap()
                    .iter()
                    .find(|c| c.name == "queue")
                    .unwrap()
                    .content(&updated)
            )
        );
    }

    // Idempotency: a follow-up maintenance pass on the converged document
    // mutates nothing and sends no further convergence.
    let state2 = run_queue_maintenance(&doc, None).unwrap();
    assert_eq!(state2.queue_halted, None);
    std::thread::sleep(std::time::Duration::from_millis(100));
    {
        let msgs = received.lock().unwrap();
        let convergences = msgs
            .iter()
            .filter(|m| m.get("queue_auto").is_some())
            .count();
        assert_eq!(
            convergences, 1,
            "follow-up maintenance must not re-diverge / re-send convergence"
        );
    }

    let _ = std::fs::remove_file(crate::ipc_socket::socket_path(&root));
    drop(server);
}

#[test]
fn preflight_pauses_when_active_queue_head_changes_mid_edit() {
    // #queue-no-stall-on-head-edit (pause case): while a live editor is
    // actively mid-edit on the head prompt, the loop must still pause/halt
    // rather than grab a half-typed head. The settled-buffer adopt path is
    // covered separately by
    // `preflight_adopts_edited_queue_head_when_buffer_settled`.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let snapshot_content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#oldhead]\n",
        "- do [#nexthead]\n",
        "<!-- /agent:queue -->\n"
    );
    let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
    std::fs::write(&doc, &current_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    // Mark the document as actively being typed so the head edit reads as
    // a half-typed buffer.
    crate::debounce::document_changed(&doc.to_string_lossy());

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(state.queue_active, Some(false));
    assert_eq!(state.queue_halted.as_deref(), Some("item_modified"));
    assert!(state.queue_prompts.is_empty());

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("queue: stop"));
    assert!(updated.contains("<!-- agent:queue -->"));
    assert!(!updated.contains("agent:queue auto"));
    assert!(updated.contains("- do [#newhead]"));
}

#[test]
fn preflight_adopts_edited_queue_head_when_buffer_settled() {
    // #queue-no-stall-on-head-edit (adopt case): when an already-active
    // auto-queue's head prompt changes between cycles and the buffer is
    // settled (no live typing indicator), the loop must adopt the edited
    // head as the new prompt and stay armed — NOT strip `auto` / force
    // queue_active:false. The snapshot must absorb the edited head so
    // closeout queue-consume proves the same prompt and the next cycle sees
    // no spurious item_modified edit.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let snapshot_content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#oldhead]\n",
        "- do [#nexthead]\n",
        "<!-- /agent:queue -->\n"
    );
    let current_content = snapshot_content.replace("- do [#oldhead]", "- do [#newhead]");
    std::fs::write(&doc, &current_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();
    // No typing indicator written → buffer is settled.

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert_eq!(
        state.queue_halted, None,
        "settled head edit must adopt + continue, not halt"
    );
    assert_eq!(state.queue_active, Some(true));
    assert_eq!(
        state.queue_prompts,
        vec!["do [#newhead]".to_string(), "do [#nexthead]".to_string()],
        "loop continues with the edited head as the new prompt"
    );

    // File keeps the armed auto-queue with the edited head.
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("agent:queue auto"),
        "auto preserved: {updated}"
    );
    assert!(
        updated.contains("queue_active: true"),
        "active preserved: {updated}"
    );
    assert!(updated.contains("- do [#newhead]"));

    // Snapshot absorbed the edited head so a follow-up pass is idempotent
    // (no spurious item_modified on the now-converged head).
    let snap = snapshot::load(&doc).unwrap().unwrap();
    assert!(
        snap.contains("- do [#newhead]"),
        "snapshot must absorb the adopted head: {snap}"
    );
    assert!(
        !snap.contains("- do [#oldhead]"),
        "snapshot must drop the stale head: {snap}"
    );
    let state2 = run_queue_maintenance(&doc, None).unwrap();
    assert_eq!(
        state2.queue_halted, None,
        "converged head must not re-halt on the next pass"
    );
    assert_eq!(state2.queue_active, Some(true));
}

#[test]
fn preflight_preserves_intentional_duplicate_tracked_queue_prompt() {
    // #queue-dedup-destroys-intentional-duplicates / #md-ast-document-model:
    // duplicate `do [#id]` text can be intentional user queue intent. Preflight
    // must not collapse it by raw prompt/id matching; only duplicate AST node
    // keys are eligible for cleanup.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "preset #spec-test-build-install-commit-push\n",
        "- ~do [#adoc-sqlite-seam]~\n",
        "- do [#adoc-orch-shim-cleanup]\n",
        "- do [#adoc-orch-shim-cleanup]\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(
        updated.matches("- do [#adoc-orch-shim-cleanup]").count(),
        2,
        "duplicate tracked prompts must remain executable queue intent:\n{updated}"
    );
    assert_eq!(
        state.queue_prompts,
        vec![
            "do [#adoc-orch-shim-cleanup]".to_string(),
            "do [#adoc-orch-shim-cleanup]".to_string()
        ],
        "duplicate tracked prompts should remain queued: {state:?}"
    );
    // Re-running maintenance on the converged doc is a no-op (stable).
    let before = std::fs::read_to_string(&doc).unwrap();
    let _ = run_queue_maintenance(&doc, None).unwrap();
    let after = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(
        before, after,
        "queue maintenance must be idempotent after dedup"
    );
}

#[test]
fn preflight_keeps_intentional_duplicate_free_text_prompt() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do deploy\n",
        "- do deploy\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();
    assert_eq!(
        state.queue_prompts,
        vec!["do deploy".to_string(), "do deploy".to_string()],
        "intentional duplicate free-text prompts should remain queued: {state:?}"
    );
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(
        updated.matches("- do deploy").count(),
        2,
        "maintenance should preserve intentional duplicate free-text prompts:\n{updated}"
    );
}

#[test]
fn preflight_does_not_reflag_stable_inactive_queue_as_residue() {
    // #adoc-queue-ipc-drift root cause #1: after an `item_modified` halt the
    // queue goes inactive (queue_active: false, no `auto`) with a retained
    // live tail, and the halt synced that shape into the snapshot. On the
    // NEXT preflight the inactive queue is unchanged from the snapshot, so
    // re-emitting `inactive_queue_residue` every cycle (with no user edit)
    // is pure loop noise and must be suppressed.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    // Snapshot == file: a stable, already-committed inactive queue with a
    // retained tail (the post-halt steady state).
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- ~do [#first-done]~\n",
        "- do [#second-live]\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert!(
        !state
            .warnings
            .iter()
            .any(|w| w.code == "inactive_queue_residue"),
        "stable inactive queue (unchanged vs snapshot) must not re-warn residue: {:?}",
        state.warnings
    );
    // The retained tail is preserved, and maintenance is idempotent.
    let before = std::fs::read_to_string(&doc).unwrap();
    assert!(before.contains("- do [#second-live]"));
    let _ = run_queue_maintenance(&doc, None).unwrap();
    let after = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(before, after, "stable inactive queue must not be mutated");
}

#[test]
fn preflight_flags_inactive_queue_when_changed_this_cycle() {
    // Counterpart guard (Scenario B): when the operator adds content to an
    // inactive queue this cycle (snapshot empty queue, file has a new live
    // item), the residue warning must still fire so the user knows the
    // `do [#id]` they added will not run while the queue is inactive.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let snapshot_content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n"
    );
    let current_content = snapshot_content.replace(
        "<!-- agent:queue -->\n<!-- /agent:queue -->",
        "<!-- agent:queue -->\n- do [#freshly-added]\n<!-- /agent:queue -->",
    );
    std::fs::write(&doc, &current_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    let state = run_queue_maintenance(&doc, None).unwrap();

    assert!(
        state
            .warnings
            .iter()
            .any(|w| w.code == "inactive_queue_residue"),
        "inactive queue changed this cycle must warn residue: {:?}",
        state.warnings
    );
}

#[test]
fn preflight_clears_completed_auto_queue_when_no_prompts_remain() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "preset #spec-test-build-install-commit-push\n",
        "- ~do [#crossdocpend]~\n",
        "- ~do [#spfxnorm]~\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run(&doc).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"));
    assert!(!updated.contains("agent:queue auto"));
    assert!(!updated.contains("preset #spec-test-build-install-commit-push"));
    assert!(!updated.contains("[#crossdocpend]"));
    assert!(!updated.contains("[#spfxnorm]"));
    assert!(updated.contains("queue_active: false"));

    let snap = snapshot::load(&doc).unwrap().unwrap();
    assert!(snap.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"));
    assert!(!snap.contains("agent:queue auto"));
    assert!(!snap.contains("[#crossdocpend]"));
    assert!(!snap.contains("[#spfxnorm]"));
}

#[test]
fn preflight_clears_completed_non_auto_queue_without_snapshot_proof() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- ~do [#item-a]~\n",
        "- ~do [#item-b]~\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run(&doc).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"),
        "completed non-auto queue should be cleared without snapshot proof:\n{updated}"
    );
    assert!(!updated.contains("[#item-a]"));
    assert!(!updated.contains("[#item-b]"));
    assert!(updated.contains("queue_active: false"));
}

#[test]
fn preflight_does_not_clear_live_inactive_queue_without_snapshot_proof() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- ~do [#done-item]~\n",
        "- do [#still-live]\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run(&doc).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("- do [#still-live]"),
        "queue with live prompts must not be cleared:\n{updated}"
    );
}

#[test]
fn preflight_clears_completed_non_auto_queue_when_snapshot_was_active() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let snapshot_content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "dispatch #spec-test-build-install-commit-push\n",
        "- ~do [#cspe]~\n",
        "<!-- /agent:queue -->\n"
    );
    let current_content = snapshot_content.replace("queue_active: true", "queue_active: false");
    std::fs::write(&doc, &current_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    run(&doc).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"),
        "proven drained non-auto queue should be cleared:\n{updated}"
    );
    assert!(!updated.contains("dispatch #spec-test-build-install-commit-push"));
    assert!(!updated.contains("[#cspe]"));
    assert!(updated.contains("queue_active: false"));

    let snap = snapshot::load(&doc).unwrap().unwrap();
    assert!(snap.contains("<!-- agent:queue -->\n<!-- /agent:queue -->"));
    assert!(!snap.contains("dispatch #spec-test-build-install-commit-push"));
    assert!(!snap.contains("[#cspe]"));
    assert!(snap.contains("queue: stop"));
}

#[test]
fn preflight_does_not_swallow_user_prose_that_mentions_head() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let baseline = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- agent:boundary:abc123 -->\n",
        "<!-- /agent:exchange -->\n"
    );
    let current = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- agent:boundary:abc123 -->\n",
        "`❯ ` prompt prefix is being stripped away by the uncommitted user affordance that adds the ` (HEAD)` suffix. spec-test-build-install-commit-push\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, current).unwrap();
    snapshot::save(&doc, baseline).unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
}

#[test]
fn preflight_auto_commits_open_write_applied_cycle() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nAnswer\n";
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();
    crate::cycle_state::mark_write_applied(&doc, "write_template", Some(content), Some(content))
        .unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
    assert_eq!(state.last_event, "commit_success");
}

/// Phase 3 (#jbccc3): the jb_cache_conflict_cancel pattern leaves a cycle
/// marked `Committed` while the snapshot still has the visible response
/// and `HEAD` does not — the commit boundary never actually landed (e.g.
/// the user canceled the JB File Cache Conflict dialog mid-IPC, or a
/// sibling compact-exchange closed the cycle while a separate `finalize`
/// race lost its write). Without recovery, `preflight` bails on the next
/// invocation. With Phase 3, the recoverable pattern triggers an
/// automatic `git::commit` and the cycle lands cleanly.
#[test]
fn preflight_auto_recovers_jb_cache_conflict_cancel_committed_with_snapshot_drift() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
    std::fs::write(&doc, original).unwrap();
    snapshot::save(&doc, original).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    // Simulate the post-cancel state: snapshot and working tree both
    // contain the response, HEAD does not, cycle is marked Committed.
    let patched = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n";
    std::fs::write(&doc, patched).unwrap();
    snapshot::save(&doc, patched).unwrap();
    crate::cycle_state::mark_write_applied(&doc, "write_template", Some(patched), Some(patched))
        .unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(patched), Some(patched))
        .unwrap();
    let pre_state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(pre_state.phase, crate::cycle_state::CyclePhase::Committed);
    assert!(matches!(
        crate::git::verify_snapshot_committed(&doc).unwrap(),
        crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
    ));
    assert!(
        crate::session_check::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
        "preconditions: cancel pattern should be detected before recovery"
    );

    run(&doc).unwrap();

    assert!(matches!(
        crate::git::verify_snapshot_committed(&doc).unwrap(),
        crate::git::SnapshotCommitStatus::Committed
    ));
    let show = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&show.stdout).contains("Reply"),
        "HEAD should now contain the response after auto-recovery"
    );
}

/// #drained-done-queue-clear: a standalone no-diff preflight that drains a
/// fully-resolved auto-queue writes the drained shape to disk + snapshot
/// but leaves HEAD on the active-queue commit. The next preflight commit
/// step must self-heal that pure queue-maintenance drift via the route
/// queue commit-boundary recovery instead of stranding it for manual
/// `agent-doc commit`. The drained snapshot has no active prompts, so this
/// shape recovers only because HEAD proves the prior active auto-queue and
/// nothing but queue state differs.
#[test]
fn route_queue_commit_boundary_recovers_drained_queue_snapshot() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    let active = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, active).unwrap();
    snapshot::save(&doc, active).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "active queue", "--no-verify"])
        .output()
        .unwrap();

    // Standalone maintenance drained the queue: queue_active cleared, auto
    // stripped, body emptied — on disk and in the snapshot — but HEAD still
    // carries the active auto-queue.
    let drained = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, drained).unwrap();
    snapshot::save(&doc, drained).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(active), Some(active)).unwrap();

    assert!(matches!(
        crate::git::verify_snapshot_committed(&doc).unwrap(),
        crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
    ));
    let rc = crate::graph::RunContext::new(doc.clone());
    assert!(
        detect_route_queue_snapshot_commit_boundary_recoverable(&doc, &rc).unwrap(),
        "drained-queue maintenance drift must be recoverable"
    );

    assert!(recover_route_queue_snapshot_commit_boundary(&doc, &rc).unwrap());
    assert!(
        matches!(
            crate::git::verify_snapshot_committed(&doc).unwrap(),
            crate::git::SnapshotCommitStatus::Committed
        ),
        "drained queue must be committed after recovery"
    );
    let show = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    let head = String::from_utf8_lossy(&show.stdout);
    assert!(head.contains("queue_active: false"), "HEAD: {head}");
    assert!(!head.contains("agent:queue auto"), "HEAD: {head}");
}

/// #drained-done-queue-clear guard: the route queue commit-boundary
/// recovery must NOT fire when a real user edit rides alongside the queue
/// drain. Only pure queue-state churn is auto-committable.
#[test]
fn route_queue_commit_boundary_skips_drained_queue_with_user_edit() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    let active = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "queue_active: true\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, active).unwrap();
    snapshot::save(&doc, active).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "active queue", "--no-verify"])
        .output()
        .unwrap();

    // Drained queue PLUS an unrelated exchange edit — must not auto-commit.
    let drained_plus_edit = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\nDone.\n\nAn extra user line.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, drained_plus_edit).unwrap();
    snapshot::save(&doc, drained_plus_edit).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(active), Some(active)).unwrap();

    let rc = crate::graph::RunContext::new(doc.clone());
    assert!(
        !detect_route_queue_snapshot_commit_boundary_recoverable(&doc, &rc).unwrap(),
        "a user edit alongside the drain must block auto-commit"
    );
}

/// Phase 3 (#jbccc3): the direct Cancel shape can also leave the cycle at
/// `write_applied` rather than `committed`: the response is visible and
/// saved in the snapshot, but the post-write commit never landed in HEAD.
/// The next preflight must treat that as the same recoverable
/// jb_cache_conflict_cancel pattern and close the missing commit boundary.
#[test]
fn preflight_auto_recovers_jb_cache_conflict_cancel_write_applied_with_snapshot_drift() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
    std::fs::write(&doc, original).unwrap();
    snapshot::save(&doc, original).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let patched = "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n";
    std::fs::write(&doc, patched).unwrap();
    snapshot::save(&doc, patched).unwrap();
    crate::cycle_state::mark_write_applied(&doc, "write_template", Some(patched), Some(patched))
        .unwrap();

    let pre_state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        pre_state.phase,
        crate::cycle_state::CyclePhase::WriteApplied
    );
    assert!(matches!(
        crate::git::verify_snapshot_committed(&doc).unwrap(),
        crate::git::SnapshotCommitStatus::SnapshotDiffersFromHead { .. }
    ));
    assert!(
        crate::session_check::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
        "preconditions: write_applied cancel pattern should be detected before recovery"
    );

    run(&doc).unwrap();

    assert!(matches!(
        crate::git::verify_snapshot_committed(&doc).unwrap(),
        crate::git::SnapshotCommitStatus::Committed
    ));
    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
    let show = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        String::from_utf8_lossy(&show.stdout).contains("Reply"),
        "HEAD should now contain the response after write_applied auto-recovery"
    );
}

#[test]
fn preflight_recovers_jb_cache_conflict_cancel_orphaned_capture_once() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    let original = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #0ep7\n",
        "<!-- agent:boundary:test -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, original).unwrap();
    snapshot::save(&doc, original).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let response = concat!(
        "<!-- patch:exchange -->\n",
        "### Re: #0ep7 — gpt-5\n\n",
        "Recovered once.\n",
        "<!-- /patch:exchange -->\n"
    );
    crate::repair::save_pending(&doc, response).unwrap();
    let capture = crate::capture::load_active(&doc).unwrap().unwrap();
    let pending_path = snapshot::pending_path_for(&doc).unwrap();
    assert!(
        pending_path.exists(),
        "precondition: orphaned pending response"
    );

    let materialized = original.replace(
        "<!-- agent:boundary:test -->",
        concat!(
            "### Re: #0ep7 — gpt-5\n\n",
            "Recovered once.\n",
            "<!-- agent:boundary:test -->"
        ),
    );
    std::fs::write(&doc, &materialized).unwrap();
    snapshot::save(&doc, &materialized).unwrap();
    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some(&materialized),
        Some(&materialized),
    )
    .unwrap();

    assert!(
        crate::session_check::detect_jb_cache_conflict_cancel_recoverable(&doc).unwrap(),
        "preconditions: committed cancel pattern should be recoverable before preflight"
    );

    run(&doc).unwrap();

    let count = Command::new("git")
        .current_dir(root)
        .args(["rev-list", "--count", "HEAD"])
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&count.stdout).trim(), "2");
    assert!(
        !pending_path.exists(),
        "orphaned pending response should be retired"
    );

    let content = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(
        content.matches("### Re: #0ep7 — gpt-5").count(),
        1,
        "visible response must not be replayed a second time:\n{content}"
    );
    assert_eq!(
        content.matches("<!-- agent:queue -->").count(),
        1,
        "template queue scaffold should stay balanced:\n{content}"
    );
    assert!(matches!(
        crate::session_check::inspect(&doc).unwrap(),
        crate::session_check::SessionCheckStatus::Ok(_)
    ));

    let refreshed = crate::capture::load_by_id(&doc, &capture.capture_id)
        .unwrap()
        .unwrap();
    let snapshot_content = snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(refreshed.state, crate::capture::CaptureState::Committed);
    assert_eq!(
        refreshed.file_hash.as_deref(),
        Some(crate::ops_log::content_hash(&content).as_str()),
        "capture file hash should refresh to the recovered visible file"
    );
    assert_eq!(
        refreshed.snapshot_hash.as_deref(),
        Some(crate::ops_log::content_hash(&snapshot_content).as_str()),
        "capture snapshot hash should refresh to the recovered snapshot"
    );
}

#[test]
fn preflight_repairs_jb_cache_conflict_accept_duplicate_replay() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    let committed = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: #gsqlwrite — gpt-5\n\n",
        "Committed response.\n",
        "<!-- agent:boundary:committed -->\n",
        "<!-- /agent:exchange -->\n",
    );
    std::fs::write(&doc, committed).unwrap();
    snapshot::save(&doc, committed).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "committed response", "--no-verify"])
        .output()
        .unwrap();
    crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
        .unwrap();

    let replayed = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: #gsqlwrite — gpt-5 (HEAD)\n\nCommitted response.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
    std::fs::write(&doc, replayed).unwrap();
    assert!(
        crate::session_check::detect_jb_cache_conflict_accept_duplicate_replay(&doc)
            .unwrap()
            .is_some(),
        "preconditions: accepted-conflict duplicate replay should be detected"
    );

    run(&doc).unwrap();

    assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
    assert_eq!(snapshot::load(&doc).unwrap().unwrap(), committed);
    let diff = Command::new("git")
        .current_dir(root)
        .args(["diff", "--", "session.md"])
        .output()
        .unwrap();
    assert!(
        diff.stdout.is_empty(),
        "preflight repair should restore the working tree to committed HEAD"
    );
}

#[test]
fn preflight_repairs_late_ipc_response_overapplication() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    // HEAD has two distinct committed responses, A then B.
    let committed = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: first answer — opus-4-8\n\n",
        "Answer A.\n",
        "### Re: second answer — opus-4-8\n\n",
        "Answer B.\n",
        "<!-- agent:boundary:committed -->\n",
        "<!-- /agent:exchange -->\n",
    );
    std::fs::write(&doc, committed).unwrap();
    snapshot::save(&doc, committed).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "committed responses", "--no-verify"])
        .output()
        .unwrap();
    crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
        .unwrap();

    // Late-IPC replay re-inserts an EARLIER committed response (A) at the
    // tail, separated from its original by response B. This is NOT a
    // consecutive duplicate, so the JB-cache-conflict replay detector misses
    // it, but it is still a committed-response over-application.
    let overapplied = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: first answer — opus-4-8\n\nAnswer A.\n<!-- agent:boundary:replayed -->\n<!-- /agent:exchange -->",
        );
    std::fs::write(&doc, overapplied).unwrap();

    assert!(
        crate::session_check::detect_jb_cache_conflict_accept_duplicate_replay(&doc)
            .unwrap()
            .is_none(),
        "preconditions: non-adjacent duplicate is missed by the consecutive replay detector"
    );
    assert!(
        crate::session_check::detect_late_ipc_response_overapplication(&doc)
            .unwrap()
            .is_some(),
        "preconditions: late-IPC over-application should be detected"
    );

    run(&doc).unwrap();

    assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
    assert_eq!(snapshot::load(&doc).unwrap().unwrap(), committed);
    let diff = Command::new("git")
        .current_dir(root)
        .args(["diff", "--", "session.md"])
        .output()
        .unwrap();
    assert!(
        diff.stdout.is_empty(),
        "preflight repair should restore the working tree to committed HEAD"
    );
}

#[test]
fn preflight_repairs_stale_jb_cache_conflict_accept_replay() {
    // #jb-cache-conflict-stale-accept-replay: a JB File Cache Conflict
    // accepted hours later replayed a STALE queued IPC reposition patch — an
    // earlier draft of a response whose final version is already committed.
    // Disk becomes HEAD plus a surplus block with the same `### Re:` topic
    // (and a `(HEAD)` marker) but a DRIFTED body. The strict over-application
    // detector misses it (bodies differ); the topic-tolerant fallback must
    // still auto-repair to committed HEAD instead of accusing a patchback.
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    let committed = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: fix thing — opus-4-8\n\n",
        "Final answer.\n",
        "<!-- agent:boundary:committed -->\n",
        "<!-- /agent:exchange -->\n",
    );
    std::fs::write(&doc, committed).unwrap();
    snapshot::save(&doc, committed).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "committed response", "--no-verify"])
        .output()
        .unwrap();
    crate::cycle_state::start_preflight(&doc, Some(committed), Some(committed)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(committed), Some(committed))
        .unwrap();

    // Surplus STALE replay of the same topic, body drifted, `(HEAD)` marked.
    let replayed = committed.replace(
            "<!-- agent:boundary:committed -->\n<!-- /agent:exchange -->",
            "### Re: fix thing — opus-4-8 (HEAD)\n\nFinal answer.\nNote: stale draft paragraph the committed copy dropped.\n<!-- agent:boundary:stale -->\n<!-- /agent:exchange -->",
        );
    std::fs::write(&doc, &replayed).unwrap();

    assert!(
        !crate::dedupe::is_committed_response_overapplication(&replayed, committed),
        "preconditions: strict over-application must NOT match a drifted-body replay"
    );
    assert!(
        crate::session_check::detect_late_ipc_response_overapplication(&doc)
            .unwrap()
            .is_some(),
        "the stale-replay fallback should detect the over-application"
    );

    run(&doc).unwrap();

    assert_eq!(std::fs::read_to_string(&doc).unwrap(), committed);
    assert_eq!(snapshot::load(&doc).unwrap().unwrap(), committed);
    let diff = Command::new("git")
        .current_dir(root)
        .args(["diff", "--", "session.md"])
        .output()
        .unwrap();
    assert!(
        diff.stdout.is_empty(),
        "preflight repair should restore the working tree to committed HEAD"
    );
}

#[test]
fn preflight_refreshes_capture_after_user_committed_baseline_drift() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");

    let original = concat!(
        "---\n",
        "session: test\n",
        "agent_doc_format: template\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ do #bdauc\n",
        "<!-- agent:boundary:test -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#bdauc] Baseline drift task\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, original).unwrap();
    snapshot::save(&doc, original).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let response = concat!(
        "<!-- patch:exchange -->\n",
        "### Re: #bdauc — gpt-5\n\n",
        "Implemented and verified.\n",
        "❯ Submodule pointer updated.\n",
        "<!-- /patch:exchange -->\n"
    );
    let capture = crate::capture::capture_response(&doc, response).unwrap();

    let current = original
        .replace(
            "<!-- agent:boundary:test -->",
            concat!(
                "### Re: #bdauc — gpt-5\n\n",
                "Implemented and verified.\n",
                "Submodule pointer updated.\n",
                "<!-- agent:boundary:test -->"
            ),
        )
        .replace(
            "- [ ] [#bdauc] Baseline drift task\n",
            concat!(
                "- [ ] [#bdauc] Baseline drift task\n",
                "- [ ] [#manual] User committed unrelated follow-up\n"
            ),
        );
    std::fs::write(&doc, &current).unwrap();
    snapshot::save(&doc, &current).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "manual baseline drift", "--no-verify"])
        .output()
        .unwrap();

    run(&doc).unwrap();

    let refreshed = crate::capture::load_by_id(&doc, &capture.capture_id)
        .unwrap()
        .unwrap();
    assert_eq!(
        refreshed.file_hash.as_deref(),
        Some(crate::ops_log::content_hash(&current).as_str()),
        "preflight should refresh the capture file hash before replay"
    );
    assert_eq!(
        refreshed.snapshot_hash.as_deref(),
        Some(crate::ops_log::content_hash(&current).as_str()),
        "preflight should refresh the capture snapshot hash before replay"
    );
    let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("capture_baseline_refreshed_for_benign_drift"),
        "preflight must drive validate_replay's baseline refresh path:\n{log}"
    );
}

#[test]
fn preflight_resumes_commit_when_write_landed_without_open_cycle_state() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let original = "---\nsession: test\n---\n\n## User\n\nHello\n";
    std::fs::write(&doc, original).unwrap();
    snapshot::save(&doc, original).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let patched =
        "---\nsession: test\n---\n\n## User\n\nHello\n\n## Assistant\n\nRecovered answer\n";
    std::fs::write(&doc, patched).unwrap();
    snapshot::save(&doc, patched).unwrap();
    let ops = root.join(".agent-doc/logs/ops.log");
    std::fs::write(
        &ops,
        format!(
            "[100] snapshot_saved_file_ipc file={} snap_len={}\n",
            doc.display(),
            patched.len()
        ),
    )
    .unwrap();

    let (recovered, committed) = enforce_cycle_completion(&doc).unwrap();
    assert!(
        !recovered,
        "no replay should be needed when file already has the response"
    );
    assert!(
        committed,
        "commit boundary should resume and create a commit"
    );

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);

    let show = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(show.status.success(), "git show HEAD:session.md failed");
    let committed_doc = String::from_utf8_lossy(&show.stdout);
    assert!(
        committed_doc.contains("Recovered answer"),
        "HEAD should include the resumed response closeout:\n{committed_doc}"
    );

    let log = std::fs::read_to_string(ops).unwrap();
    assert!(
        log.contains("resume_commit_success file="),
        "resume commit success should be logged:\n{log}"
    );
}

#[test]
fn pending_maintenance_reaps_completed_items_from_file_and_snapshot() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Pending / Not Built\n\n",
        "<!-- agent:backlog -->\n",
        "- [x] [#reap1] Reap me\n",
        "- [ ] [#keep1] Keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let report = run_pending_maintenance(&doc).unwrap();
    assert!(!report.reordered);
    assert_eq!(report.pending_gated_count, 0);

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let file_backlog_after = crate::component::parse(&file_after)
        .unwrap()
        .into_iter()
        .find(|c| c.name == "backlog")
        .unwrap()
        .content(&file_after)
        .to_string();
    assert!(!file_backlog_after.contains("[#reap1]"));
    assert!(file_after.contains("[#keep1]"));
    assert!(file_after.contains("## Completed / Reaped"));
    assert!(file_after.contains("<!-- agent:done -->"));
    assert!(file_after.contains("[#reap1] Reap me"));

    let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
    let snapshot_backlog_after = crate::component::parse(&snapshot_after)
        .unwrap()
        .into_iter()
        .find(|c| c.name == "backlog")
        .unwrap()
        .content(&snapshot_after)
        .to_string();
    assert!(!snapshot_backlog_after.contains("[#reap1]"));
    assert!(snapshot_after.contains("[#keep1]"));
    assert!(snapshot_after.contains("## Completed / Reaped"));
    assert!(snapshot_after.contains("<!-- agent:done -->"));
    assert!(snapshot_after.contains("[#reap1] Reap me"));
}

#[test]
fn pending_maintenance_auto_reaps_ops_proof_done_items() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#doneci] #agent-doc-bug DONE 7b60fcdc (CI 27075841879 green): supervisor idle-queue watch self-heals stale busy state\n",
        "- [ ] [#partial] #agent-doc-bug PARTIAL SHIPPED 9df1244f: committed first slice. REMAINING: live proof gate\n",
        "- [ ] [#reopened] #agent-doc-bug REOPENED false closeout: previous closeout DONE 1234567 (CI 1 green)\n",
        "- [ ] [#noproof] DONE: lacks deterministic proof\n",
        "<!-- /agent:backlog -->\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "- [/] [#reviewdone] SHIPPED abcdef1 (CI 2 passed): review-gated shipped marker\n",
        "- [/] [#reviewkeep] Needs release review\n",
        "<!-- /agent:review -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let report = run_pending_maintenance(&doc).unwrap();
    assert_eq!(report.pending_gated_count, 0);
    assert_eq!(report.review_count, 1);
    assert_eq!(report.review_gated_count, 1);

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let backlog_after = crate::component::parse(&file_after)
        .unwrap()
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
        .unwrap()
        .content(&file_after)
        .to_string();
    let review_after = crate::component::parse(&file_after)
        .unwrap()
        .into_iter()
        .find(|c| is_review_component(&c.name))
        .unwrap()
        .content(&file_after)
        .to_string();

    assert!(!backlog_after.contains("[#doneci]"));
    assert!(!review_after.contains("[#reviewdone]"));
    assert!(backlog_after.contains("[#partial]"));
    assert!(backlog_after.contains("[#reopened]"));
    assert!(backlog_after.contains("[#noproof]"));
    assert!(review_after.contains("[#reviewkeep]"));
    assert!(file_after.contains("## Completed / Reaped"));
    assert!(file_after.contains("[#doneci] #agent-doc-bug DONE 7b60fcdc"));
    assert!(file_after.contains("[#reviewdone] SHIPPED abcdef1"));

    let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
    assert!(!snapshot_after.contains("- [ ] [#doneci]"));
    assert!(!snapshot_after.contains("- [/] [#reviewdone]"));
    assert!(snapshot_after.contains("[#partial]"));
    assert!(snapshot_after.contains("[#reopened]"));
    assert!(snapshot_after.contains("[#noproof]"));
    assert!(snapshot_after.contains("[#reviewkeep]"));

    let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(log.contains("auto_complete_ops_proof"));
    assert!(log.contains("id=doneci"));
    assert!(log.contains("id=reviewdone"));
}

// #opsproof-samecycle-add: a gated review/backlog item added THIS cycle (its
// text legitimately cites a shipped dependency commit) must NOT be ops-proof
// auto-completed on the same cycle it first appears — even though the
// write/finalize path already re-synced the on-disk snapshot to include it,
// which defeats the snapshot-only same-cycle guard. cycle_state records the
// added id; the reap must honor it.
#[test]
fn ops_proof_does_not_reap_same_cycle_added_gated_item() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "- [/] [#freshgate] operator live-verify the destructive path. Code SHIPPED 1edb20d2; this is the live gate only\n",
        "<!-- /agent:review -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    // The snapshot already contains the item — this models the finalize path
    // where the same invocation's --review-add re-synced the snapshot, so the
    // snapshot-only guard cannot tell this is a brand-new add.
    snapshot::save(&doc, content).unwrap();
    // cycle_state records #freshgate as added this cycle.
    crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
    crate::cycle_state::record_pending_added_ids(&doc, &["freshgate".to_string()]).unwrap();

    run_pending_maintenance(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    // The freshly added gated item survives — not reaped on its first cycle.
    assert!(
        file_after.contains("[#freshgate]"),
        "same-cycle-added gated item must not be ops-proof reaped: {file_after}"
    );
    let log =
        std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
    assert!(
        !log.contains("auto_complete_ops_proof"),
        "no ops-proof auto-completion should fire for a same-cycle add"
    );
}

// #opsproof-falsepos: an open actionable backlog item whose completion
// marker only describes already-landed *dependency* work (a cited commit
// hash in mid-sentence prose) must NOT be auto-reaped. Only a marker that is
// the item's own leading status verb proves the item itself is done.
#[test]
fn ops_proof_does_not_reap_cited_dependency_marker() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#citeddep] wire the predicate into dispatch. The predicate already shipped in 600797b3 and is unit-tested\n",
        "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate into dispatch\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run_pending_maintenance(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let backlog_after = crate::component::parse(&file_after)
        .unwrap()
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
        .unwrap()
        .content(&file_after)
        .to_string();

    // Cited-dependency marker stays open; leading-status marker is reaped.
    assert!(
        backlog_after.contains("[#citeddep]"),
        "cited-dependency item must not be reaped: {backlog_after}"
    );
    assert!(!backlog_after.contains("[#leadstatus]"));
    assert!(file_after.contains("[#leadstatus] DONE 7b60fcdc"));
}

// #opsproofgate: a live-verify / operator-drive gate that cites a shipped
// commit hash (e.g. "Code SHIPPED 1edb20d2") in its text must NOT be
// auto-completed on evidence=commit — even when it has existed for several
// cycles (not a same-cycle add). Only an anchored structured ops.log marker
// driven live by the operator may close it.
#[test]
fn ops_proof_does_not_reap_live_verify_gate_on_commit_hash() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "- [/] [#ktw8] [live-verify gate] destructive auto-/clear between queue turns. ",
        "Code SHIPPED 1edb20d2; a shipped commit is NOT proof, an operator drive is. ",
        "PASS = a genuine anchored ops.log line; current verdict UNDRIVEN.\n",
        "<!-- /agent:review -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run_pending_maintenance(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        file_after.contains("[#ktw8]"),
        "live-verify gate must not be ops-proof reaped on a cited commit hash: {file_after}"
    );
    let log =
        std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log")).unwrap_or_default();
    assert!(
        !log.contains("auto_complete_ops_proof"),
        "no ops-proof auto-completion should fire for a live-verify gate"
    );
}

// #opsproof-falsepos: never auto-archive an item on the same cycle it is
// added. A brand-new add is absent from the cycle-start snapshot, so even a
// leading-status completion marker must not reap it this cycle.
#[test]
fn pending_maintenance_does_not_reap_same_cycle_add() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    // Snapshot baseline: an existing leading-status done item + a keeper.
    let snapshot_content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate\n",
        "- [ ] [#keep] keep this open item\n",
        "<!-- /agent:backlog -->\n"
    );
    // File adds a brand-new same-cycle item with a leading-status marker that
    // would normally reap — but it is absent from the snapshot.
    let file_content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#freshdone] DONE abc1234: just landed this cycle\n",
        "- [ ] [#leadstatus] DONE 7b60fcdc: wired the predicate\n",
        "- [ ] [#keep] keep this open item\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, file_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    run_pending_maintenance(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let backlog_after = crate::component::parse(&file_after)
        .unwrap()
        .into_iter()
        .find(|c| is_backlog_component(&c.name))
        .unwrap()
        .content(&file_after)
        .to_string();

    // Same-cycle add survives; pre-existing leading-status item is reaped.
    assert!(
        backlog_after.contains("[#freshdone]"),
        "same-cycle add must not be reaped: {backlog_after}"
    );
    assert!(backlog_after.contains("[#keep]"));
    assert!(!backlog_after.contains("[#leadstatus]"));
}

#[test]
fn pending_maintenance_reaps_inline_done_backlog_and_review_mirrors() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#done1] stale backlog mirror\n",
        "- [ ] [#keep1] keep backlog\n",
        "<!-- /agent:backlog -->\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "- [/] [#done2] stale review mirror\n",
        "- [/] [#keep2] keep review\n",
        "<!-- /agent:review -->\n\n",
        "## Completed / Reaped\n\n",
        "<!-- agent:done -->\n",
        "- [x] [#done1] already archived backlog\n",
        "- [x] [#done2] already archived review\n",
        "<!-- /agent:done -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let report = run_pending_maintenance(&doc).unwrap();
    assert_eq!(report.pending_gated_count, 0);
    assert_eq!(report.review_count, 1);
    assert_eq!(report.review_gated_count, 1);

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let file_components = crate::component::parse(&file_after).unwrap();
    let file_backlog = file_components
        .iter()
        .find(|c| c.name == "backlog")
        .unwrap()
        .content(&file_after)
        .to_string();
    let file_review = file_components
        .iter()
        .find(|c| c.name == "review")
        .unwrap()
        .content(&file_after)
        .to_string();
    assert!(!file_backlog.contains("[#done1]"));
    assert!(file_backlog.contains("[#keep1] keep backlog"));
    assert!(!file_review.contains("[#done2]"));
    assert!(file_review.contains("[#keep2] keep review"));
    assert_eq!(file_after.matches("[#done1]").count(), 1);
    assert_eq!(file_after.matches("[#done2]").count(), 1);

    let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
    let snapshot_components = crate::component::parse(&snapshot_after).unwrap();
    let snapshot_backlog = snapshot_components
        .iter()
        .find(|c| c.name == "backlog")
        .unwrap()
        .content(&snapshot_after)
        .to_string();
    let snapshot_review = snapshot_components
        .iter()
        .find(|c| c.name == "review")
        .unwrap()
        .content(&snapshot_after)
        .to_string();
    assert!(!snapshot_backlog.contains("[#done1]"));
    assert!(!snapshot_review.contains("[#done2]"));
    assert_eq!(snapshot_after.matches("[#done1]").count(), 1);
    assert_eq!(snapshot_after.matches("[#done2]").count(), 1);
}

#[test]
fn pending_maintenance_reaps_external_done_archive_backlog_and_review_mirrors() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let archive_rel = "session.done.md";
    let archive_path = dir.path().join(archive_rel);
    let archive_content = concat!(
        "# Done\n\n",
        "- [x] [#extdone1] externally archived backlog\n",
        "- [x] [#extdone2] externally archived review\n",
    );
    std::fs::write(&archive_path, archive_content).unwrap();
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#extdone1] stale backlog mirror\n",
        "- [ ] [#fresh1] fresh backlog\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:review -->\n",
        "- [/] [#extdone2] stale review mirror\n",
        "- [/] [#fresh2] fresh review\n",
        "<!-- /agent:review -->\n\n",
        "<!-- agent:done archive=session.done.md -->\n",
        "<!-- /agent:done -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let report = run_pending_maintenance(&doc).unwrap();
    assert_eq!(report.review_count, 1);
    assert_eq!(report.review_gated_count, 1);

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let file_components = crate::component::parse(&file_after).unwrap();
    let file_backlog = file_components
        .iter()
        .find(|c| c.name == "backlog")
        .unwrap()
        .content(&file_after)
        .to_string();
    let file_review = file_components
        .iter()
        .find(|c| c.name == "review")
        .unwrap()
        .content(&file_after)
        .to_string();
    assert!(!file_backlog.contains("[#extdone1]"));
    assert!(file_backlog.contains("[#fresh1] fresh backlog"));
    assert!(!file_review.contains("[#extdone2]"));
    assert!(file_review.contains("[#fresh2] fresh review"));
    assert_eq!(
        std::fs::read_to_string(&archive_path).unwrap(),
        archive_content
    );

    let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
    assert!(!snapshot_after.contains("stale backlog mirror"));
    assert!(!snapshot_after.contains("stale review mirror"));
    assert!(snapshot_after.contains("[#fresh1] fresh backlog"));
    assert!(snapshot_after.contains("[#fresh2] fresh review"));
}

#[test]
fn archive_pending_done_inserts_canonical_done_component() {
    let dir = setup_project();
    let file = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&file, content).unwrap();
    let archived = archive_pending_done(
        &file,
        content,
        &[crate::pending::PendingItem {
            marker: crate::pending::PendingListMarker::Bullet,
            id: "done1".to_string(),
            state: crate::pending::PendingState::Done,
            gate_type: None,
            text: "completed item".to_string(),
            continuation: String::new(),
        }],
    )
    .unwrap()
    .unwrap();

    assert!(archived.contains("<!-- agent:done -->"));
    assert!(archived.contains("<!-- /agent:done -->"));
    assert!(!archived.contains("<!-- agent:backlog-done -->"));
    assert!(!archived.contains("<!-- agent:pending-done -->"));
    assert!(archived.contains("[#done1] completed item"));
}

#[test]
fn archive_pending_done_ignores_removed_pending_done_alias() {
    let dir = setup_project();
    let file = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:pending-done -->\n",
        "<!-- /agent:pending-done -->\n"
    );
    std::fs::write(&file, content).unwrap();
    let archived = archive_pending_done(
        &file,
        content,
        &[crate::pending::PendingItem {
            marker: crate::pending::PendingListMarker::Bullet,
            id: "done1".to_string(),
            state: crate::pending::PendingState::Done,
            gate_type: None,
            text: "completed item".to_string(),
            continuation: String::new(),
        }],
    )
    .unwrap()
    .unwrap();

    assert!(archived.contains("<!-- agent:pending-done -->"));
    assert!(archived.contains("<!-- agent:done -->"));
    assert!(!archived.contains("<!-- agent:backlog-done -->"));
    assert!(archived.contains("[#done1] completed item"));
}

#[test]
fn archive_pending_done_appends_to_external_done_archive() {
    let dir = setup_project();
    let file = dir.path().join("tasks/session.md");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done archive=tasks/session.done.md -->\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&file, content).unwrap();

    let archived = archive_pending_done(
        &file,
        content,
        &[crate::pending::PendingItem {
            marker: crate::pending::PendingListMarker::Bullet,
            id: "done1".to_string(),
            state: crate::pending::PendingState::Done,
            gate_type: None,
            text: "completed externally".to_string(),
            continuation: String::new(),
        }],
    )
    .unwrap()
    .unwrap();

    let external = std::fs::read_to_string(dir.path().join("tasks/session.done.md")).unwrap();
    assert!(external.contains("[#done1] completed externally"));
    assert!(!archived.contains("[#done1]"));
    assert!(archived.contains("completed work archived in tasks/session.done.md"));

    archive_pending_done(
        &file,
        &archived,
        &[crate::pending::PendingItem {
            marker: crate::pending::PendingListMarker::Bullet,
            id: "done1".to_string(),
            state: crate::pending::PendingState::Done,
            gate_type: None,
            text: "completed externally".to_string(),
            continuation: String::new(),
        }],
    )
    .unwrap()
    .unwrap();
    let external_after = std::fs::read_to_string(dir.path().join("tasks/session.done.md")).unwrap();
    assert_eq!(external_after.matches("[#done1]").count(), 1);
}

#[test]
fn archive_pending_done_rejects_invalid_external_archive_paths() {
    let dir = setup_project();
    let file = dir.path().join("session.md");
    let item = crate::pending::PendingItem {
        marker: crate::pending::PendingListMarker::Bullet,
        id: "done1".to_string(),
        state: crate::pending::PendingState::Done,
        gate_type: None,
        text: "completed item".to_string(),
        continuation: String::new(),
    };
    for archive_path in [
        "/tmp/session.done.md",
        "../session.done.md",
        "tasks/session.md",
    ] {
        let content = format!(
            "<!-- agent:backlog -->\n<!-- /agent:backlog -->\n\n<!-- agent:done archive={} -->\n<!-- /agent:done -->\n",
            archive_path
        );
        std::fs::write(&file, &content).unwrap();
        let err = archive_pending_done(&file, &content, std::slice::from_ref(&item))
            .unwrap_err()
            .to_string();
        assert!(
            err.contains("agent:done archive="),
            "unexpected error for {archive_path}: {err}"
        );
    }
}

#[test]
fn external_done_archive_ids_satisfy_dropped_history_guard() {
    let dir = setup_project();
    let file = dir.path().join("tasks/session.md");
    std::fs::create_dir_all(file.parent().unwrap()).unwrap();
    std::fs::write(
        dir.path().join("tasks/session.done.md"),
        "# Agent Doc Completed Work\n\n- 2026-05-13 [#item1] Was open\n",
    )
    .unwrap();
    let baseline = concat!(
        "<!-- agent:backlog -->\n",
        "- [ ] [#item1] Was open\n",
        "<!-- /agent:backlog -->\n"
    );
    let current = concat!(
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done archive=tasks/session.done.md -->\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&file, current).unwrap();

    let external_ids = external_done_archive_ids(&file, current).unwrap();
    let report = crate::pending::detect_dropped_from_history_with_extra_current_ids(
        current,
        baseline,
        &HashSet::new(),
        &external_ids,
    )
    .unwrap();

    assert!(report.dropped.is_empty());
}

#[test]
fn preflight_allows_user_marked_done_item_reaped_in_same_cycle() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let baseline = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Pending / Not Built\n\n",
        "<!-- agent:backlog -->\n",
        "- [/] [#done1] Waiting on manual validation\n",
        "- [ ] [#keep1] Keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, baseline).unwrap();
    snapshot::save(&doc, baseline).unwrap();
    Command::new("git")
        .current_dir(dir.path())
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(dir.path())
        .args(["commit", "-m", "baseline", "--no-verify"])
        .output()
        .unwrap();

    let current = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Pending / Not Built\n\n",
        "<!-- agent:backlog -->\n",
        "- [x] [#done1] Waiting on manual validation\n",
        "- [ ] [#keep1] Keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, current).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(baseline), Some(current)).unwrap();

    let report = run_pending_maintenance(&doc).unwrap();
    assert!(!report.reordered);
    assert_eq!(report.pending_gated_count, 0);
    let rc = crate::graph::RunContext::new(doc.clone());
    enforce_no_dropped_backlog(&doc, &rc)
        .expect("same-cycle reap should count as intentional completion");
}

#[test]
fn pending_maintenance_reaps_completed_icebox_items_from_file_and_snapshot() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Pending / Not Built\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep1] Keep me\n",
        "<!-- /agent:backlog -->\n\n",
        "## Icebox\n\n",
        "<!-- agent:icebox -->\n",
        "- [x] [#ice01] Reap me from icebox\n",
        "- [ ] [#keep2] Keep me parked\n",
        "<!-- /agent:icebox -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let report = run_pending_maintenance(&doc).unwrap();
    assert!(!report.reordered);
    assert_eq!(report.pending_gated_count, 0);

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let file_icebox_after = crate::component::parse(&file_after)
        .unwrap()
        .into_iter()
        .find(|c| c.name == "icebox")
        .unwrap()
        .content(&file_after)
        .to_string();
    assert!(!file_icebox_after.contains("[#ice01]"));
    assert!(file_after.contains("[#keep2]"));
    assert!(file_after.contains("## Completed / Reaped"));
    assert!(file_after.contains("[#ice01] Reap me from icebox"));

    let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
    let snapshot_icebox_after = crate::component::parse(&snapshot_after)
        .unwrap()
        .into_iter()
        .find(|c| c.name == "icebox")
        .unwrap()
        .content(&snapshot_after)
        .to_string();
    assert!(!snapshot_icebox_after.contains("[#ice01]"));
    assert!(snapshot_after.contains("[#keep2]"));
    assert!(snapshot_after.contains("## Completed / Reaped"));
    assert!(snapshot_after.contains("[#ice01] Reap me from icebox"));
}

#[test]
fn pending_maintenance_syncs_snapshot_for_write_phase_gate_without_reap() {
    // #pending-gate-snapshot-desync: the write phase moved #g1 from backlog
    // to review (a --pending-gate) on the FILE, but the content_ours snapshot
    // still shows #g1 in backlog and an empty review. Maintenance makes no
    // reap/backfill change, yet it must re-sync the snapshot's tracked
    // surfaces to the file so the upcoming commit stages the gate instead of
    // stranding it as post-commit drift.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let file_content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep1] Keep me\n",
        "<!-- /agent:backlog -->\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "- [/] [#g1] Gated, awaiting review\n",
        "<!-- /agent:review -->\n"
    );
    // Snapshot lags the file: #g1 still in backlog, review empty (the
    // baseline+response content_ours saved before the gate mutation).
    let snapshot_content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep1] Keep me\n",
        "- [ ] [#g1] Gated, awaiting review\n",
        "<!-- /agent:backlog -->\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "<!-- /agent:review -->\n"
    );
    std::fs::write(&doc, file_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    run_pending_maintenance(&doc).unwrap();

    let snapshot_after = snapshot::load(&doc).unwrap().unwrap();
    let comps = crate::component::parse(&snapshot_after).unwrap();
    let snap_backlog = comps
        .iter()
        .find(|c| c.name == "backlog")
        .unwrap()
        .content(&snapshot_after)
        .to_string();
    let snap_review = comps
        .iter()
        .find(|c| c.name == "review")
        .unwrap()
        .content(&snapshot_after)
        .to_string();
    // Snapshot now matches the file: #g1 gated into review, gone from backlog.
    assert!(
        !snap_backlog.contains("[#g1]"),
        "snapshot backlog must drop the gated item: {snap_backlog}"
    );
    assert!(
        snap_review.contains("[/] [#g1]"),
        "snapshot review must carry the gated item: {snap_review}"
    );
    assert!(snap_backlog.contains("[#keep1]"));
}

fn write_optverify_doc(dir: &TempDir, predicate_annotation: &str) -> std::path::PathBuf {
    let doc = dir.path().join("session.md");
    let file_content = format!(
        concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "## Review\n\n",
            "<!-- agent:review -->\n",
            "- [/] [#saev] early-ack live verify {}\n",
            "<!-- /agent:review -->\n"
        ),
        predicate_annotation
    );
    std::fs::write(&doc, &file_content).unwrap();
    snapshot::save(&doc, &file_content).unwrap();
    doc
}

fn write_ops_log(dir: &TempDir, body: &str) {
    let logs = dir.path().join(".agent-doc/logs");
    std::fs::create_dir_all(&logs).unwrap();
    std::fs::write(logs.join("ops.log"), body).unwrap();
}

#[test]
fn gate_verify_surfaces_provable_without_flipping_when_optin_off() {
    let dir = setup_project();
    let pred = crate::gate_verify::render_annotation(&crate::gate_verify::GatePredicate {
        verify: Some("early_ack_pending".to_string()),
        disproof: Some("false ack-timeout".to_string()),
        set_at: Some(100),
    });
    let doc = write_optverify_doc(&dir, &pred);
    write_ops_log(&dir, "[150] early_ack_pending emitted ok\n");

    let results = run_gate_verify(&doc, false).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].id, "saev");
    assert_eq!(results[0].status, "provable");
    assert!(
        !results[0].auto_resolved,
        "opt-in off must not flip the gate"
    );

    // The document still shows the gated item — never silently flipped.
    let after = std::fs::read_to_string(&doc).unwrap();
    assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
}

#[test]
fn gate_verify_auto_resolves_provable_when_optin_on() {
    let dir = setup_project();
    let pred = crate::gate_verify::render_annotation(&crate::gate_verify::GatePredicate {
        verify: Some("early_ack_pending".to_string()),
        disproof: None,
        set_at: Some(100),
    });
    let doc = write_optverify_doc(&dir, &pred);
    write_ops_log(&dir, "[150] early_ack_pending emitted ok\n");

    let results = run_gate_verify(&doc, true).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, "provable");
    assert!(results[0].auto_resolved);

    let after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains("[x] [#saev]"),
        "gate must be flipped: {after}"
    );
    // Snapshot kept in lockstep for the upcoming commit.
    let snap = snapshot::load(&doc).unwrap().unwrap();
    assert!(
        snap.contains("[x] [#saev]"),
        "snapshot must flip too: {snap}"
    );
}

#[test]
fn gate_verify_failed_never_auto_resolves_even_with_optin() {
    let dir = setup_project();
    let pred = crate::gate_verify::render_annotation(&crate::gate_verify::GatePredicate {
        verify: Some("early_ack_pending".to_string()),
        disproof: Some("manual cleanup".to_string()),
        set_at: Some(100),
    });
    let doc = write_optverify_doc(&dir, &pred);
    write_ops_log(
        &dir,
        "[150] early_ack_pending emitted\n[160] looks like a manual cleanup\n",
    );

    let results = run_gate_verify(&doc, true).unwrap();
    assert_eq!(results[0].status, "failed", "disproof wins");
    assert!(!results[0].auto_resolved);
    let after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains("- [/] [#saev]"),
        "failed gate must remain: {after}"
    );
}

#[test]
fn gate_verify_empty_without_predicate() {
    let dir = setup_project();
    let doc = write_optverify_doc(&dir, "");
    write_ops_log(&dir, "[150] early_ack_pending emitted\n");
    let results = run_gate_verify(&doc, true).unwrap();
    assert!(results.is_empty(), "no predicate → no results");
}

#[test]
fn gate_verify_ignores_marker_quoted_in_content_logging_lines() {
    // #gng8: queue_diff_active_prompt_differs embeds document prose via
    // {:?}; a gate must not auto-prove from its own backlog description.
    let dir = setup_project();
    let pred = crate::gate_verify::render_annotation(&crate::gate_verify::GatePredicate {
        verify: Some("early_ack_pending".to_string()),
        disproof: None,
        set_at: Some(100),
    });
    let doc = write_optverify_doc(&dir, &pred);
    write_ops_log(
        &dir,
        "[150] queue_diff_active_prompt_differs file=doc.md prompt_changes=[\"expect early_ack_pending emitted before apply\"] queue_head=\"[#saev]\"\n",
    );

    let results = run_gate_verify(&doc, true).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, "pending", "quoted prose must not prove");
    assert!(!results[0].auto_resolved);
    let after = std::fs::read_to_string(&doc).unwrap();
    assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
}

#[test]
fn gate_verify_s760_builtin_ignores_queue_diff_prose_only() {
    // #ktw8: the destructive clear gate is proven only by an anchored
    // structured [s760] line, never by prose embedded in queue_diff logs.
    let dir = setup_project();
    let pred = crate::gate_verify::render_annotation(&crate::gate_verify::GatePredicate {
        verify: Some(crate::gate_verify::S760_CLEAR_DECISION_CLEAR_TRUE_MARKER.to_string()),
        disproof: None,
        set_at: Some(100),
    });
    let doc = write_optverify_doc(&dir, &pred);
    write_ops_log(
        &dir,
        "[150] queue_diff_active_prompt_differs file=doc.md prompt_changes=[\"PASS requires [s760] clear-decision optIn=true threshold=50 pct=50.0 clear=true\"] queue_head=\"[#ktw8]\"\n",
    );

    let results = run_gate_verify(&doc, true).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, "pending", "quoted prose must not prove");
    assert!(!results[0].auto_resolved);
    let after = std::fs::read_to_string(&doc).unwrap();
    assert!(after.contains("- [/] [#saev]"), "gate must remain: {after}");
}

#[test]
fn gate_verify_s760_builtin_auto_resolves_on_anchored_clear_true() {
    let dir = setup_project();
    let pred = crate::gate_verify::render_annotation(&crate::gate_verify::GatePredicate {
        verify: Some(crate::gate_verify::S760_CLEAR_DECISION_CLEAR_TRUE_MARKER.to_string()),
        disproof: None,
        set_at: Some(100),
    });
    let doc = write_optverify_doc(&dir, &pred);
    write_ops_log(
        &dir,
        "[150] [s760] clear-decision optIn=true threshold=50 pct=50.0 clear=true\n",
    );

    let results = run_gate_verify(&doc, true).unwrap();
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].status, "provable");
    assert!(results[0].auto_resolved);
    let after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        after.contains("[x] [#saev]"),
        "gate must be flipped: {after}"
    );
}

#[test]
fn pending_maintenance_fails_closed_when_snapshot_backlog_cannot_be_synced() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let file_content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Pending / Not Built\n\n",
        "<!-- agent:backlog -->\n",
        "- [x] [#reap1] Reap me\n",
        "<!-- /agent:backlog -->\n"
    );
    let snapshot_content =
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\nNo backlog here.\n";
    std::fs::write(&doc, file_content).unwrap();
    snapshot::save(&doc, snapshot_content).unwrap();

    let err = run_pending_maintenance(&doc).unwrap_err();
    assert!(
        err.to_string()
            .contains("snapshot is missing the backlog component")
    );
}

#[test]
fn preflight_fails_closed_when_open_backlog_item_exists_only_in_shadow_copy() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Pending / Not Built\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep1] Keep me live\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- parked digest\n",
        "- [ ] [#lost1] Drifted out of backlog\n",
        "-->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    let err = run(&doc).unwrap_err();
    assert!(
        err.to_string()
            .contains("open backlog item(s) exist only outside")
    );
    assert!(err.to_string().contains("#lost1"));
}

#[test]
fn preflight_allows_shadow_copy_when_live_backlog_entry_still_exists() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Pending / Not Built\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep1] Keep me live\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- parked digest\n",
        "- [ ] [#keep1] Duplicate parked copy\n",
        "-->\n"
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run(&doc).unwrap();
}

#[test]
fn preflight_closes_response_captured_cycle_when_snapshot_already_matches_head() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let committed = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
    std::fs::write(&doc, committed).unwrap();
    snapshot::save(&doc, committed).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let visible_snapshot = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:test-boundary -->\n\
            <!-- /agent:exchange -->\n";
    snapshot::save(&doc, visible_snapshot).unwrap();

    let with_user_edit = format!("{visible_snapshot}\n❯ follow-up question\n");
    std::fs::write(&doc, &with_user_edit).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(visible_snapshot), Some(&with_user_edit))
        .unwrap();
    crate::cycle_state::mark_response_captured(
        &doc,
        "response_captured",
        Some(visible_snapshot),
        Some(&with_user_edit),
        "sha256",
        None,
    )
    .unwrap();

    let (recovered, committed) = enforce_cycle_completion(&doc).unwrap();
    assert!(
        recovered,
        "the missing commit boundary should be recovered from already-committed HEAD"
    );
    assert!(
        !committed,
        "HEAD-current closeout should not create a duplicate git commit"
    );

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
    assert_eq!(state.last_event, "commit_already_current");

    let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("commit_already_current file="),
        "preflight should record the no-op closeout instead of failing:\n{log}"
    );
    assert!(
        !log.contains("commit_failed"),
        "preflight should not log a false commit_failed for HEAD-current closeout:\n{log}"
    );
}

#[test]
fn preflight_reruns_cleanly_after_open_preflight_started_cycle() {
    let dir = setup_project();
    let root = dir.path();
    let doc = dir.path().join("session.md");
    let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();
    crate::git::commit(&doc).unwrap();
    let prior = crate::cycle_state::start_preflight(&doc, Some(content), Some(content)).unwrap();
    std::fs::write(&doc, "---\nsession: test\n---\n\n## User\n\nHello again\n").unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
    assert_ne!(
        state.cycle_id, prior.cycle_id,
        "rerun should close the old preflight and open a fresh one"
    );
    let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("commit_already_current file="),
        "rerun should close the previous preflight via the no-op commit path:\n{log}"
    );
}

#[test]
fn preflight_abandons_stale_empty_preflight_started_prompt_drift_without_capture() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_format: template\nagent_doc_session: test\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: older — gpt-5\n",
        "old body\n",
        "<!-- agent:boundary:abc123 -->\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();
    let prior = crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

    let live = snapshot.replace(
            "<!-- agent:boundary:abc123 -->\n",
            "do [#root-empty-preflight]. spec-test-build-install-commit-push\n<!-- agent:boundary:abc123 -->\n",
        );
    std::fs::write(&doc, &live).unwrap();
    age_cycle_state(&doc, crate::repair::STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
    assert_ne!(
        state.cycle_id, prior.cycle_id,
        "preflight should abandon the stale empty cycle and open a fresh cycle for the prompt"
    );
    assert_eq!(state.last_event, "preflight_started");

    let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("repair_preflight_stale_prompt_cycle_abandoned file="),
        "preflight should log the abandoned empty cycle:\n{log}"
    );
}

#[test]
fn preflight_abandoned_stale_next_steps_prompt_stays_actionable() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "agent_doc_session: test\n",
        "prompt_presets:\n",
        "  '#next-steps': Any follow-up items to place in the backlog?\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n",
        "Compacted.\n",
        "<!-- agent:boundary:abc123 -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();
    let prior = crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

    let prompt = "Left/Right buttons still do not work with agent-doc opencode. #next-steps";
    let live = snapshot.replace(
        "<!-- agent:boundary:abc123 -->\n",
        &format!("{prompt}\n<!-- agent:boundary:abc123 -->\n"),
    );
    std::fs::write(&doc, &live).unwrap();
    age_cycle_state(&doc, crate::repair::STALE_EMPTY_PREFLIGHT_TTL_SECS + 1);

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
    assert_ne!(
        state.cycle_id, prior.cycle_id,
        "preflight should abandon the stale empty cycle and open a fresh cycle for the prompt"
    );
    assert!(
        state.requires_backlog_capture,
        "the inline #next-steps prompt should still require backlog capture"
    );
    let diff = crate::diff::compute(&doc).unwrap().unwrap();
    let prompt_targets = crate::diff::classify_prompt_bearing_changes(&diff)
        .into_iter()
        .filter(|change| change.kind == crate::diff::PromptBearingChangeKind::PromptTarget)
        .map(|change| change.text)
        .collect::<Vec<_>>();
    assert!(
        prompt_targets.iter().any(|target| target.contains(prompt)),
        "fresh preflight should surface the abandoned #next-steps prompt as actionable, got {prompt_targets:?}"
    );

    let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("repair_preflight_stale_prompt_cycle_abandoned file="),
        "preflight should log the abandoned empty cycle:\n{log}"
    );
    assert!(
        log.contains("post_commit_user_follow_up file="),
        "step-2 commit should classify the prompt-bearing drift as a follow-up, not absorb it:\n{log}"
    );
}

#[test]
fn preflight_compact_follow_up_next_steps_is_not_swallowed_by_commit_recovery() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "agent_doc_session: test\n",
        "prompt_presets:\n",
        "  '#next-steps': Any follow-up items to place in the backlog?\n",
        "---\n\n",
        "## Status\n\n",
        "<!-- agent:status patch=replace -->\n",
        "Compacted.\n",
        "<!-- /agent:status -->\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n",
        "Compacted content archived.\n",
        "<!-- agent:boundary:compact -->\n",
        "<!-- /agent:exchange -->\n\n",
        "## Pending\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "compact exchange", "--no-verify"])
        .output()
        .unwrap();

    let live = snapshot.replace(
        "<!-- agent:boundary:compact -->\n",
        "#next-steps\n<!-- agent:boundary:compact -->\n",
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted,
        "compact follow-up should open a response cycle instead of becoming no_changes"
    );
    assert!(
        state.requires_backlog_capture,
        "compact follow-up #next-steps should carry the backlog-capture contract"
    );
    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snapshot_after, snapshot,
        "preflight must not absorb the compact follow-up prompt into the snapshot"
    );
    let head = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(head.status.success(), "git show HEAD:session.md failed");
    assert_eq!(
        String::from_utf8_lossy(&head.stdout).as_ref(),
        snapshot,
        "step-2 commit must not silently commit the compact follow-up prompt"
    );
}

#[test]
fn preflight_commits_route_queue_snapshot_before_live_prompt_edit() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let original_prompt = "Run Agent Doc queued this prompt. #spec-test-build-install-commit-push";
    let edited_prompt = "Run Agent Doc queued this prompt. Same with this file. #spec-test-build-install-commit-push";
    let head = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "agent_doc_session: test\n",
        "queue_active: false\n",
        "---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "<!-- agent:boundary:abc123 -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n"
    );
    let queued = head
        .replace("queue_active: false", "queue_active: true")
        .replace(
            "<!-- agent:boundary:abc123 -->\n",
            &format!("{original_prompt}\n<!-- agent:boundary:abc123 -->\n"),
        )
        .replace(
            "<!-- agent:queue -->\n<!-- /agent:queue -->",
            &format!("<!-- agent:queue auto -->\n- {original_prompt}\n<!-- /agent:queue -->"),
        );
    let live = queued.replacen(original_prompt, edited_prompt, 1);

    std::fs::write(&doc, head).unwrap();
    crate::snapshot::save(&doc, head).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    crate::snapshot::save(&doc, &queued).unwrap();
    std::fs::write(&doc, &live).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(head), Some(head)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(&queued), Some(&queued))
        .unwrap();

    run(&doc).unwrap();

    let committed = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(
        committed.status.success(),
        "git show HEAD:session.md failed"
    );
    let committed = String::from_utf8_lossy(&committed.stdout);
    assert!(
        committed.contains(original_prompt),
        "route queued prompt should be committed from the saved snapshot:\n{committed}"
    );
    assert!(
        !committed.contains("Same with this file"),
        "live prompt edit must not be swallowed into the queue snapshot commit:\n{committed}"
    );
    let working = std::fs::read_to_string(&doc).unwrap();
    assert!(
        working.contains(edited_prompt),
        "later live prompt edit should remain visible for the fresh preflight cycle:\n{working}"
    );
    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        snapshot_after.contains(original_prompt),
        "snapshot should stay on the route queued prompt:\n{snapshot_after}"
    );
    assert!(
        !snapshot_after.contains("Same with this file"),
        "preflight must not absorb the live edit into the committed snapshot:\n{snapshot_after}"
    );
    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted,
        "after committing the queued snapshot, preflight should open a fresh cycle for the live edit"
    );
    let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        log.contains("route_queue_snapshot_auto_recovery_succeeded file="),
        "route queue commit-boundary recovery should be logged:\n{log}"
    );
}

#[test]
fn preflight_started_cycle_does_not_revert_stale_snapshot_head() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = "---\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let live = "---\nsession: test\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n### Re: newer\nnew body\n❯ follow-up question\n<!-- /agent:exchange -->\n";
    std::fs::write(&doc, live).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "manual patchback", "--no-verify"])
        .output()
        .unwrap();

    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(live)).unwrap();

    run(&doc).unwrap();

    let show = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(show.status.success(), "git show HEAD:session.md failed");
    let committed = String::from_utf8_lossy(&show.stdout);
    assert!(
        committed.contains("### Re: newer"),
        "HEAD should stay at the newer manual content instead of reverting:\n{committed}"
    );
    assert!(
        committed.contains("❯ follow-up question"),
        "HEAD should keep the live follow-up question instead of reverting:\n{committed}"
    );

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
}

#[test]
fn preflight_fails_closed_on_ambiguous_preflight_started_patchback_without_artifact() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "<!-- agent:boundary:abc123 -->\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();

    let live = concat!(
        "---\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "### Re: topic — gpt-5\n",
        "Recovered body.\n",
        "<!-- agent:boundary:abc123 -->\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, live).unwrap();

    let err = run(&doc).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains(crate::repair::AMBIGUOUS_PREFLIGHT_STARTED_PATCHBACK_ERROR),
        "expected fail-closed ambiguous patchback error, got: {message}"
    );

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted,
        "ambiguous patchback must not be auto-committed"
    );
}

#[test]
fn preflight_started_repair_fails_when_matching_cycle_file_has_uncommitted_patchback() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "<!-- agent:boundary:abc123 -->\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let live = concat!(
        "---\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ Please reply\n",
        "### Re: topic — gpt-5\n",
        "Recovered body.\n",
        "<!-- agent:boundary:abc123 -->\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, live).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(live)).unwrap();

    let err = run(&doc).unwrap_err();
    let message = err.to_string();
    assert!(
        message.contains(crate::repair::RESPONSE_PATCHBACK_UNCOMMITTED_ERROR),
        "expected uncommitted response patchback error, got: {message}"
    );

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted,
        "recovery must not mark the stale cycle committed while HEAD lacks the visible response"
    );
}

#[test]
fn preflight_completed_backlog_reap_does_not_swallow_live_prompt() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: do #scopeid — gpt-5\n",
        "Implemented.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [x] [#scopeid] completed item\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done -->\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let live = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: do #scopeid — gpt-5\n",
        "Implemented.\n",
        "do #statusws. spec-test-build-install-commit-push\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [x] [#scopeid] completed item\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done -->\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted,
        "preflight should still open a response cycle for the live prompt"
    );

    let file_after = std::fs::read_to_string(&doc).unwrap();
    assert!(file_after.contains("do #statusws. spec-test-build-install-commit-push"));
    assert!(!file_after.contains("- [x] [#scopeid] completed item"));

    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        !snapshot_after.contains("do #statusws. spec-test-build-install-commit-push"),
        "snapshot must not absorb the live prompt during backlog reap"
    );
    assert!(!snapshot_after.contains("- [x] [#scopeid] completed item"));

    let head = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(head.status.success(), "git show HEAD:session.md failed");
    let head_text = String::from_utf8_lossy(&head.stdout);
    assert!(
        !head_text.contains("do #statusws. spec-test-build-install-commit-push"),
        "repair/commit must not silently commit the live prompt:\n{head_text}"
    );
}

#[test]
fn preflight_relocates_out_of_exchange_prompt_without_swallowing_live_diff() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n",
        "Done.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "###\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let live = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n",
        "Done.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "do [#oobprompt]. spec-test-build-install-commit-push\n",
        "###\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted,
        "preflight should still open a response cycle for the relocated prompt"
    );

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let exchange_close = file_after.find("<!-- /agent:exchange -->").unwrap();
    let prompt = file_after
        .find("❯ do [#oobprompt]. spec-test-build-install-commit-push")
        .unwrap();
    let gap_marker = file_after.find("\n###\n\n").unwrap();
    assert!(
        prompt < exchange_close,
        "preflight should move the prompt back inside exchange:\n{file_after}"
    );
    assert!(
        gap_marker > exchange_close,
        "preflight should leave the gap marker outside exchange:\n{file_after}"
    );
    assert!(
        !file_after.contains(
            "\n<!-- /agent:exchange -->\n\ndo [#oobprompt]. spec-test-build-install-commit-push"
        ),
        "out-of-exchange prompt should not remain in the gap:\n{file_after}"
    );

    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        !snapshot_after.contains("oobprompt"),
        "snapshot must not absorb the live prompt during preflight relocation:\n{snapshot_after}"
    );
}

#[test]
fn preflight_does_not_relocate_prompt_text_inside_post_exchange_html_comment() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n",
        "Done.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "###\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let live = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n",
        "Done.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "###\n\n",
        "<!--\n",
        "Content that I added into the html comment below agent:exchange in this doc was deleted by agent-doc.\n",
        "spec-test-build-install-commit-push\n",
        "---\n",
        "older scratch note\n",
        "-->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let exchange_close = file_after.find("<!-- /agent:exchange -->").unwrap();
    let hidden_prompt = file_after
        .find("Content that I added into the html comment below agent:exchange")
        .unwrap();
    let comment_open = file_after.find("\n<!--\n").unwrap();
    let comment_close = file_after.find("\n-->\n\n<!-- agent:backlog -->").unwrap();
    assert!(
        hidden_prompt > exchange_close,
        "scratch-comment prompt text must stay outside exchange:\n{file_after}"
    );
    assert!(
        hidden_prompt > comment_open && hidden_prompt < comment_close,
        "scratch-comment prompt text must remain inside the ordinary HTML comment:\n{file_after}"
    );
    assert!(
            !file_after.contains(
                "\nContent that I added into the html comment below agent:exchange in this doc was deleted by agent-doc.\nspec-test-build-install-commit-push\n<!-- /agent:exchange -->"
            ),
            "preflight must not move scratch-comment text into exchange:\n{file_after}"
        );
}

#[test]
fn preflight_warns_on_prompt_preset_text_inside_post_exchange_html_comment() {
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "prompt_presets:\n",
        "  '#spec-test-build-install-commit-push': update spec + tests\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!--\n",
        "Scratch note while testing.\n",
        "dispatch #spec-test-build-install-commit-push\n",
        "-->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n",
    );
    let (fm, _) = crate::frontmatter::parse(content).unwrap();
    let warning = post_exchange_comment_prompt_preset_warning(
        Path::new("session.md"),
        content,
        &fm.prompt_presets,
    )
    .expect("known prompt preset in ordinary post-exchange comment should warn");

    assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
    assert!(
        warning
            .message
            .contains("#spec-test-build-install-commit-push")
    );
    assert!(warning.message.contains("non-executable user note"));
}

#[test]
fn preflight_comment_prompt_preset_warning_ignores_agent_components() {
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "prompt_presets:\n",
        "  '#spec-test-build-install-commit-push': update spec + tests\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "dispatch #spec-test-build-install-commit-push\n",
        "<!-- /agent:queue -->\n",
        "<!-- agent:done -->\n",
        "<!-- archived #spec-test-build-install-commit-push -->\n",
        "<!-- /agent:done -->\n",
    );
    let (fm, _) = crate::frontmatter::parse(content).unwrap();

    assert!(
        post_exchange_comment_prompt_preset_warning(
            Path::new("session.md"),
            content,
            &fm.prompt_presets,
        )
        .is_none(),
        "agent-owned queue directives remain executable state, not ordinary scratch comments"
    );
}

#[test]
fn misplaced_component_attr_warning_flags_auto_on_backlog() {
    // #backlog-auto-marker-misfire: `auto` is a queue-only attribute; on the
    // backlog it must be surfaced (no longer silently tolerated).
    let content = concat!(
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog auto -->\n",
        "- [ ] [#x1] keep this\n",
        "<!-- /agent:backlog -->\n",
    );
    let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
        .expect("`auto` on agent:backlog should warn");
    assert_eq!(warning.code, "misplaced_component_attr");
    assert!(warning.message.contains("queue-only attribute"));
    assert!(warning.message.contains("agent:backlog"));
    assert!(warning.message.contains("agent:queue auto"));
    assert!(warning.message.contains("no mutation"));
}

#[test]
fn misplaced_component_attr_warning_flags_unknown_attr_typo() {
    // The reported trigger was the typo `auot`; an unrecognized key must warn.
    let content = concat!(
        "<!-- agent:backlog auot -->\n",
        "- [ ] [#x1] keep this\n",
        "<!-- /agent:backlog -->\n",
    );
    let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
        .expect("typo'd attribute on a component should warn");
    assert_eq!(warning.code, "misplaced_component_attr");
    assert!(
        warning
            .message
            .contains("not a recognized component attribute")
    );
    assert!(warning.message.contains("auot"));
}

#[test]
fn misplaced_component_attr_warning_allows_queue_sync_attr_on_backlog() {
    // #backlog-queue-sync-attr: `queue`, `queue=sync|append|prepend` on the
    // backlog are recognized sync attributes and must not warn.
    for marker in [
        "<!-- agent:backlog queue -->",
        "<!-- agent:backlog queue=sync -->",
        "<!-- agent:backlog queue=append -->",
    ] {
        let content = format!("{marker}\n- [ ] [#x1] keep this\n<!-- /agent:backlog -->\n");
        assert!(
            misplaced_component_attr_warning(Path::new("session.md"), &content).is_none(),
            "recognized queue sync attr must not warn: {marker}"
        );
    }
}

#[test]
fn misplaced_component_attr_warning_flags_queue_sync_attr_on_icebox() {
    let content = concat!(
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:icebox queue=append -->\n",
        "- [ ] [#x1] parked work\n",
        "<!-- /agent:icebox -->\n",
    );
    let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
        .expect("`queue` on agent:icebox should warn");
    assert_eq!(warning.code, "misplaced_component_attr");
    assert!(warning.message.contains("agent:icebox"));
    assert!(warning.message.contains("does not auto-populate"));
    assert!(warning.message.contains("per-item enqueue"));
}

#[test]
fn misplaced_component_attr_warning_allows_priority_attr() {
    // #backlog-priority-attribute: bare `priority` on backlog/icebox/queue
    // is a recognized ordering attribute and must not warn.
    for content in [
        "<!-- agent:backlog priority -->\n- [ ] [#a] x\n<!-- /agent:backlog -->\n",
        "<!-- agent:backlog priority queue -->\n- [ ] [#a] x\n<!-- /agent:backlog -->\n",
        "<!-- agent:queue priority -->\n- do [#a]\n<!-- /agent:queue -->\n",
    ] {
        assert!(
            misplaced_component_attr_warning(Path::new("session.md"), content).is_none(),
            "priority attr must not warn: {content}"
        );
    }
}

#[test]
fn run_pending_maintenance_sorts_backlog_by_priority() {
    // #backlog-priority-attribute: a backlog carrying `priority` stable-sorts
    // items by their per-item priority token each cycle.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:backlog priority -->\n",
        "- [ ] [#low] priority=5 later\n",
        "- [ ] [#high] priority=1 first\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run_pending_maintenance(&doc).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    let high = updated.find("[#high]").unwrap();
    let low = updated.find("[#low]").unwrap();
    assert!(
        high < low,
        "priority=1 item must sort before priority=5:\n{updated}"
    );
}

#[test]
fn run_queue_maintenance_orders_synced_queue_by_priority() {
    // #backlog-priority-attribute + #backlog-queue-sync-attr: a priority queue
    // synced from a priority backlog comes out prioritized.
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "agent_doc_write: crdt\n",
        "---\n\n",
        "<!-- agent:queue priority -->\n",
        "- do [#low]\n",
        "- do [#high]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog priority queue -->\n",
        "- [ ] [#low] priority=5 later\n",
        "- [ ] [#high] priority=1 first\n",
        "<!-- /agent:backlog -->\n",
    );
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();

    run_queue_maintenance(&doc, None).unwrap();

    let updated = std::fs::read_to_string(&doc).unwrap();
    let q = updated.find("<!-- agent:queue").unwrap();
    let qend = updated[q..].find("<!-- /agent:queue").unwrap() + q;
    let queue_region = &updated[q..qend];
    let high = queue_region.find("do [#high]").unwrap();
    let low = queue_region.find("do [#low]").unwrap();
    assert!(
        queue_region.contains(":round_pushpin: do [#high]"),
        "auto-promoted queue item should carry an agent-priority marker:\n{queue_region}"
    );
    assert!(
        high < low,
        "priority=1 must sort before priority=5 in queue:\n{queue_region}"
    );
}

#[test]
fn misplaced_component_attr_warning_flags_invalid_queue_mode() {
    let content = concat!(
        "<!-- agent:backlog queue=nope -->\n",
        "- [ ] [#x1] keep this\n",
        "<!-- /agent:backlog -->\n",
    );
    let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
        .expect("unrecognized queue mode should warn");
    assert_eq!(warning.code, "misplaced_component_attr");
    assert!(warning.message.contains("not a recognized sync mode"));
    assert!(warning.message.contains("queue=nope"));
}

#[test]
fn collect_backlog_queue_sync_reads_mode_and_active_ids() {
    let content = concat!(
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog queue=sync -->\n",
        "- [ ] [#a] one\n",
        "- [/] [#g] gated\n",
        "- [ ] [#b] two\n",
        "<!-- /agent:backlog -->\n",
    );
    let components = crate::component::parse(content).unwrap();
    let request = collect_backlog_queue_sync(&components, content)
        .expect("backlog with queue attr should produce a sync request");
    assert_eq!(request.mode, crate::queue::BacklogQueueSyncMode::Sync);
    assert_eq!(request.ids, vec!["a".to_string(), "b".to_string()]);
    assert!(request.enqueue_ids.is_empty());
}

#[test]
fn collect_backlog_queue_sync_reads_enqueue_markers_without_attr() {
    let content = concat!(
        "<!-- agent:queue -->\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#a] :inbox_tray: one\n",
        "- [/] [#g] :inbox_tray: gated\n",
        "- [ ] [#b] unmarked\n",
        "- [ ] [#c] **enqueue** marked\n",
        "<!-- /agent:backlog -->\n",
    );
    let components = crate::component::parse(content).unwrap();
    let request = collect_backlog_queue_sync(&components, content)
        .expect("enqueue markers should produce an append request");
    assert_eq!(request.mode, crate::queue::BacklogQueueSyncMode::Append);
    assert_eq!(request.ids, vec!["a".to_string(), "c".to_string()]);
    assert_eq!(request.enqueue_ids, vec!["a".to_string(), "c".to_string()]);
}

#[test]
fn filter_expect_done_or_gate_excludes_synced_queue_ids() {
    // #queue-sync-auto-pending-done-guard-misfire: a cycle that works one
    // directive (#worked) while the backlog→queue sync auto-populated
    // do[#a]/do[#b]/#worked into the active queue must demand only the
    // genuine worked directive, never the freshly-synced siblings.
    let directive_ids = vec!["worked".to_string(), "a".to_string(), "b".to_string()];
    let open_backlog: std::collections::HashSet<String> =
        ["worked", "a", "b"].iter().map(|s| s.to_string()).collect();
    let synced_queue_ids: std::collections::HashSet<String> =
        ["a", "b"].iter().map(|s| s.to_string()).collect();
    let result = filter_expect_done_or_gate_ids(&directive_ids, &open_backlog, &synced_queue_ids);
    assert_eq!(result, vec!["worked".to_string()]);
}

#[test]
fn filter_expect_done_or_gate_keeps_open_directives_without_sync() {
    // No sync attribute → no exclusion; an open user directive stays demanded,
    // a directive whose backlog item is already resolved drops out, and
    // duplicates collapse.
    let directive_ids = vec![
        "open".to_string(),
        "open".to_string(),
        "resolved".to_string(),
    ];
    let open_backlog: std::collections::HashSet<String> =
        ["open"].iter().map(|s| s.to_string()).collect();
    let synced_queue_ids = std::collections::HashSet::new();
    let result = filter_expect_done_or_gate_ids(&directive_ids, &open_backlog, &synced_queue_ids);
    assert_eq!(result, vec!["open".to_string()]);
}

#[test]
fn collect_backlog_queue_sync_none_without_attr() {
    let content = concat!(
        "<!-- agent:backlog -->\n",
        "- [ ] [#a] one\n",
        "<!-- /agent:backlog -->\n",
    );
    let components = crate::component::parse(content).unwrap();
    assert!(collect_backlog_queue_sync(&components, content).is_none());
}

#[test]
fn misplaced_component_attr_warning_allows_queue_auto_and_known_attrs() {
    // `auto` on the queue and known attrs elsewhere must not warn.
    let content = concat!(
        "<!-- agent:queue auto -->\n",
        "- do #fix1\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:exchange patch=append max_lines=50 -->\n",
        "### Re: prior — gpt-5\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#x1] keep this\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done archive=tasks/x.done.md -->\n",
        "<!-- /agent:done -->\n",
    );
    assert!(
        misplaced_component_attr_warning(Path::new("session.md"), content).is_none(),
        "`auto` on queue plus recognized attrs elsewhere must not warn"
    );
}

#[test]
fn misplaced_component_attr_warning_allows_queue_control_markers() {
    // `start` / `go` / `stop` are recognized queue-only control markers
    // (#queue-state-unify) — preflight migrates them into `queue:` frontmatter.
    for token in ["start", "go", "stop"] {
        let content = format!(
            "<!-- agent:queue preset=\"#p\" {token} -->\n- do #fix1\n<!-- /agent:queue -->\n",
        );
        assert!(
            misplaced_component_attr_warning(Path::new("session.md"), &content).is_none(),
            "`{token}` on queue must be a recognized control marker, not a typo warning"
        );
    }
}

#[test]
fn misplaced_component_attr_warning_allows_preset_on_queue() {
    let content = concat!(
        "<!-- agent:queue preset=\"#spec-test-build-install-commit-push\" -->\n",
        "- do #fix1\n",
        "<!-- /agent:queue -->\n",
    );
    assert!(
        misplaced_component_attr_warning(Path::new("session.md"), content).is_none(),
        "`preset` on queue is a recognized queue-only attribute"
    );
}

#[test]
fn misplaced_component_attr_warning_flags_preset_on_non_queue() {
    let content = concat!(
        "<!-- agent:backlog preset=\"#spec-test-build-install-commit-push\" -->\n",
        "- [ ] [#x1] keep this\n",
        "<!-- /agent:backlog -->\n",
    );
    let warning = misplaced_component_attr_warning(Path::new("session.md"), content)
        .expect("`preset` on backlog should warn as a queue-only attribute on wrong component");
    assert_eq!(warning.code, "misplaced_component_attr");
    assert!(
        warning.message.contains("queue-only"),
        "warning should mention queue-only: {}",
        warning.message
    );
}

#[test]
fn auto_on_backlog_does_not_activate_queue() {
    // #backlog-auto-marker-misfire regression: the auto-loop reads `auto`
    // only from the queue component, never from the backlog.
    let content = concat!(
        "<!-- agent:queue -->\n",
        "- do #fix1\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog auto -->\n",
        "- [ ] [#x1] keep this\n",
        "<!-- /agent:backlog -->\n",
    );
    let components = crate::component::parse(content).unwrap();
    let queue = components.iter().find(|c| c.name == "queue").unwrap();
    assert!(
        !crate::queue::has_auto_attr(&queue.attrs),
        "queue has no auto attribute"
    );
    let backlog = components.iter().find(|c| c.name == "backlog").unwrap();
    assert!(
        crate::queue::has_auto_attr(&backlog.attrs),
        "backlog carries the misplaced auto attribute"
    );
    let body = &content[queue.open_end..queue.close_start];
    let entries = crate::queue::parse(body).unwrap();
    // Activation is driven solely by the queue component's auto flag.
    let activation = crate::queue::resolve_activation(&entries, false, false, false);
    assert!(
        !activation.active,
        "backlog `auto` must never activate the auto-loop"
    );
}

#[test]
fn preflight_warns_on_dispatch_text_inside_post_exchange_html_comment_without_presets() {
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "<!--\n",
        "dispatch #manual-review\n",
        "/clear\n",
        "-->\n",
    );
    let (fm, _) = crate::frontmatter::parse(content).unwrap();
    let warning = post_exchange_comment_prompt_preset_warning(
        Path::new("session.md"),
        content,
        &fm.prompt_presets,
    )
    .expect("dispatch-looking text in ordinary post-exchange comment should warn");

    assert_eq!(warning.code, "post_exchange_comment_prompt_preset");
    assert!(warning.message.contains("dispatch #manual-review"));
    assert!(warning.message.contains("/clear"));
}

#[test]
fn post_exchange_comment_with_horizontal_rule_and_prose_is_user_note() {
    let content = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "prompt_presets:\n",
        "  '#spec-test-build-install-commit-push': update spec + tests\n",
        "  '#next-steps': Any follow-up items?\n",
        "---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n\n",
        "Done.\n",
        "<!-- /agent:exchange -->\n\n",
        "###\n\n",
        "<!--\n",
        "The last run had a code fence stripped away by agent-doc.\n",
        "#spec-test-build-install-commit-push\n",
        "---\n",
        "What are #next-steps to fix bugs?\n",
        "-->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n",
    );
    let (fm, _) = crate::frontmatter::parse(content).unwrap();
    let warning = post_exchange_comment_prompt_preset_warning(
        Path::new("session.md"),
        content,
        &fm.prompt_presets,
    );
    assert!(
        warning.is_none(),
        "post-exchange comment with horizontal rule and prose is a user note, not a directive: {:?}",
        warning
    );
}

#[test]
fn preflight_preserves_post_exchange_duplicate_prompt_comment_before_diff() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n",
        "Done.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "###\n\n",
        "<!--\n",
        "Keep this unrelated scratch note hidden.\n",
        "-->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push";
    let live = format!(
        concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "{prompt}\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "{prompt}\n",
            "-->\n\n",
            "<!--\n",
            "Keep this unrelated scratch note hidden.\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        ),
        prompt = prompt
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let duplicate_comment = format!("\n<!--\n{prompt}\n-->\n");
    assert!(
        file_after.contains(&duplicate_comment),
        "preflight must preserve visible post-exchange scratch comments even when they duplicate prompt text:\n{file_after}"
    );
    assert!(
        file_after.contains("Keep this unrelated scratch note hidden."),
        "unrelated scratch comments must remain outside exchange:\n{file_after}"
    );
    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        !snapshot_after.contains(prompt),
        "snapshot must not absorb the live prompt during preflight:\n{snapshot_after}"
    );
}

#[test]
fn preflight_preserves_duplicate_prompt_comment_from_snapshot() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
    let snapshot = format!(
        concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ {prompt}\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "{prompt}\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        ),
        prompt = prompt
    );
    std::fs::write(&doc, &snapshot).unwrap();
    snapshot::save(&doc, &snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let rc = crate::graph::RunContext::new(doc.clone());
    let changed = remove_post_exchange_duplicate_prompt_comments_for_preflight(&doc, &rc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        !changed,
        "preflight cleanup should not rewrite baseline-owned scratch comments"
    );
    assert!(
        file_after.contains(&format!("<!--\n{prompt}\n-->")),
        "preflight must not scrub post-exchange scratch text that already existed in HEAD:\n{file_after}"
    );
}

#[test]
fn preflight_preserves_unrelated_lines_in_mixed_post_exchange_duplicate_prompt_comment() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior - gpt-5\n",
        "Done.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "###\n",
        "<!--\n",
        "-->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let exchange_prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
    let duplicate_prompt_line = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
    let live = format!(
        concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior - gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "{exchange_prompt}\n",
            "#spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n",
            "<!--\n",
            "{duplicate_prompt_line}\n",
            "#spec-test-build-install-commit-push\n",
            "---\n",
            "Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        ),
        exchange_prompt = exchange_prompt,
        duplicate_prompt_line = duplicate_prompt_line,
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        file_after.contains(&format!("<!--\n{duplicate_prompt_line}")),
        "preflight must preserve visible duplicate-looking lines in post-exchange scratch comments:\n{file_after}"
    );
    assert!(
        file_after.contains("Look through the Claude + Codex + agent-doc session logs"),
        "preflight must preserve unrelated scratch lines in the same ordinary comment:\n{file_after}"
    );
    assert!(
        file_after.contains(&format!(
            "<!--\n{duplicate_prompt_line}\n#spec-test-build-install-commit-push\n---\nLook through"
        )),
        "preflight must keep the full mixed ordinary comment body:\n{file_after}"
    );
    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        !snapshot_after.contains(exchange_prompt),
        "snapshot must not absorb the live prompt during preflight:\n{snapshot_after}"
    );
}

#[test]
fn preflight_scrubs_duplicate_answered_prompt_tail_before_diff() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. Should we diff line by line?";
    let snapshot = format!(
        concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ {prompt}\n",
            "❯ #spec-test-build-install-commit-push\n",
            "### Re: mixed scratch comment deletion - gpt-5\n\n",
            "Answered already.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n",
            "<!--\n",
            "Keep this scratch note.\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        ),
        prompt = prompt
    );
    std::fs::write(&doc, &snapshot).unwrap();
    snapshot::save(&doc, &snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    // Genuine replay residue carries the `❯ ` answered-form marker — that is
    // the ownership proof that lets the scrub remove it without eating a live
    // re-typed prompt (#ipcfullprompt-recur).
    let live = snapshot.replace(
            "<!-- agent:boundary:head -->\n<!-- /agent:exchange -->",
            &format!(
                "<!-- agent:boundary:head -->\n❯ {prompt}\n❯ #spec-test-build-install-commit-push\n<!-- /agent:exchange -->"
            ),
        );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        !file_after.contains(&format!(
            "head -->\n❯ {prompt}\n❯ #spec-test-build-install-commit-push"
        )),
        "preflight should scrub duplicate answered-form prompt tails before diffing:\n{file_after}"
    );
    assert!(
        file_after.contains("Keep this scratch note."),
        "preflight cleanup must preserve unrelated scratch comments:\n{file_after}"
    );
    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert!(
        !snapshot_after.contains(&format!(
            "head -->\n❯ {prompt}\n❯ #spec-test-build-install-commit-push"
        )),
        "snapshot must not absorb the duplicate tail cleanup prompt"
    );
}

#[test]
fn preflight_preserves_duplicate_prompt_comment_after_typing_settles() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Very tired of playing whack-a-mole. Reproduce bugs with tests first that fail and fix the implementation. #spec-test-build-install-commit-push";
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_debounce: 3000\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: prior — gpt-5\n",
        "Done.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "###\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] keep me\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let live = format!(
        concat!(
            "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_debounce: 3000\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Re: prior — gpt-5\n",
            "Done.\n",
            "<!-- agent:boundary:head -->\n",
            "❯ {prompt}\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "{prompt}\n",
            "-->\n\n",
            "<!-- agent:backlog -->\n",
            "- [ ] keep me\n",
            "<!-- /agent:backlog -->\n"
        ),
        prompt = prompt
    );
    std::fs::write(&doc, &live).unwrap();

    let doc_for_thread = doc.clone();
    let doc_str = doc.to_string_lossy().to_string();
    crate::debounce::document_changed(&doc_str);
    let handle = std::thread::spawn(move || run(&doc_for_thread));
    std::thread::sleep(std::time::Duration::from_millis(500));
    let during_debounce = std::fs::read_to_string(&doc).unwrap();
    let result = handle.join().unwrap();
    result.unwrap();

    let duplicate_comment = format!("<!--\n{prompt}\n-->");
    assert!(
        during_debounce.contains(&duplicate_comment),
        "preflight must not mutate duplicate prompt comments while the editor typing indicator is active:\n{during_debounce}"
    );

    let file_after = std::fs::read_to_string(&doc).unwrap();
    assert!(
        file_after.contains(&duplicate_comment),
        "preflight must preserve visible scratch comments after typing settles:\n{file_after}"
    );
}

#[test]
fn preflight_session_accretion_does_not_auto_compact_exchange() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n\nExisting summary.\n\n",
        "### Re: first topic — gpt-5\n\nFirst response.\n\n",
        "### Re: second topic — gpt-5\n\nSecond response.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let relative = doc
        .strip_prefix(root)
        .unwrap()
        .to_string_lossy()
        .to_string();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    write_cycles_log(
        &doc,
        &[
            crate::ops_log::CycleEntry {
                timestamp: now.saturating_sub(10).to_string(),
                file: relative.clone(),
                op: "commit_noop".to_string(),
                commit_hash: None,
                snapshot_hash: None,
                file_hash: None,
            },
            crate::ops_log::CycleEntry {
                timestamp: now.saturating_sub(5).to_string(),
                file: relative,
                op: "commit_noop".to_string(),
                commit_hash: None,
                snapshot_hash: None,
                file_hash: None,
            },
        ],
    );

    let live = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\nagent_doc_write: crdt\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n\nExisting summary.\n\n",
        "### Re: first topic — gpt-5\n\nFirst response.\n\n",
        "### Re: second topic — gpt-5\n\nSecond response.\n",
        "<!-- agent:boundary:head -->\n",
        "do #autocmp. spec-test-build-install-commit-push\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    assert!(!file_after.contains("1 earlier topic(s) archived"));
    assert!(file_after.contains("### Re: second topic — gpt-5"));
    assert!(file_after.contains("### Re: first topic — gpt-5"));
    assert!(file_after.contains("do #autocmp. spec-test-build-install-commit-push"));

    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(snapshot_after, snapshot);
}

#[test]
fn preflight_reaps_flush_left_spill_with_completed_backlog_item() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: do #scopeid — gpt-5\n",
        "Implemented.\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [x] [#scopeid] completed item\n",
        "Commands:\n",
        "  cargo test -p agent-doc pending::\n",
        "Diff:\n",
        "@@ -1 +1 @@\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done -->\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let live = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: do #scopeid — gpt-5\n",
        "Implemented.\n",
        "do #statusws. spec-test-build-install-commit-push\n",
        "<!-- agent:boundary:head -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [x] [#scopeid] completed item\n",
        "Commands:\n",
        "  cargo test -p agent-doc pending::\n",
        "Diff:\n",
        "@@ -1 +1 @@\n",
        "<!-- /agent:backlog -->\n\n",
        "<!-- agent:done -->\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let file_after = std::fs::read_to_string(&doc).unwrap();
    let backlog_after = crate::component::parse(&file_after).unwrap();
    let backlog_after = backlog_after
        .iter()
        .find(|component| crate::component::is_backlog_component(&component.name))
        .map(|component| component.content(&file_after))
        .unwrap();
    assert!(file_after.contains("do #statusws. spec-test-build-install-commit-push"));
    assert!(!backlog_after.contains("- [x] [#scopeid] completed item"));
    assert!(!backlog_after.contains("Commands:"));
    assert!(!backlog_after.contains("@@ -1 +1 @@"));

    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    let snapshot_backlog = crate::component::parse(&snapshot_after).unwrap();
    let snapshot_backlog = snapshot_backlog
        .iter()
        .find(|component| crate::component::is_backlog_component(&component.name))
        .map(|component| component.content(&snapshot_after))
        .unwrap();
    assert!(!snapshot_backlog.contains("- [x] [#scopeid] completed item"));
    assert!(!snapshot_backlog.contains("Commands:"));
    assert!(!snapshot_backlog.contains("@@ -1 +1 @@"));
    assert!(
        !snapshot_after.contains("do #statusws. spec-test-build-install-commit-push"),
        "snapshot must not absorb the live prompt during backlog reap"
    );
}

#[test]
fn preflight_status_prompt_preset_addition_does_not_swallow_diff() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let snapshot = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "prompt_presets:\n",
        "  '#next-steps': Print the top backlog item.\n",
        "---\n\n",
        "## Status\n\n",
        "<!-- agent:status patch=replace -->\n",
        "Compacted.\n",
        "<!-- /agent:status -->\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n",
        "Compacted.\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, snapshot).unwrap();
    snapshot::save(&doc, snapshot).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let live = concat!(
        "---\n",
        "agent_doc_session: test\n",
        "agent_doc_format: template\n",
        "prompt_presets:\n",
        "  '#next-steps': Print the top backlog item.\n",
        "---\n\n",
        "## Status\n\n",
        "<!-- agent:status patch=replace -->\n",
        "Compacted.\n",
        "#next-steps for calibrating session benchmarks with expected scores\n",
        "<!-- /agent:status -->\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Session Summary\n",
        "Compacted.\n",
        "<!-- /agent:exchange -->\n"
    );
    std::fs::write(&doc, live).unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted,
        "preflight should still open a response cycle for the prompt-preset status edit"
    );

    let snapshot_after = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snapshot_after, snapshot,
        "snapshot must not absorb prompt-bearing status drift"
    );

    let head = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:session.md"])
        .output()
        .unwrap();
    assert!(head.status.success(), "git show HEAD:session.md failed");
    let head_text = String::from_utf8_lossy(&head.stdout);
    assert_eq!(
        head_text.as_ref(),
        snapshot,
        "step 2 commit must not silently commit the prompt-preset status edit:\n{head_text}"
    );
}

#[test]
fn preflight_boundary_artifact_only_diff_does_not_start_cycle() {
    let dir = setup_project();
    let root = dir.path();
    let doc = root.join("session.md");
    let tracked = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer\n\
            new body\n\
            <!-- agent:boundary:head -->\n\
            <!-- /agent:exchange -->\n";
    std::fs::write(&doc, tracked).unwrap();
    snapshot::save(&doc, tracked).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "session.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add doc", "--no-verify"])
        .output()
        .unwrap();

    let visible = "---\nsession: test\n---\n\n\
            <!-- agent:exchange patch=append -->\n\
            ### Re: older\n\
            old body\n\
            ### Re: newer (HEAD)\n\
            new body\n\
            <!-- agent:boundary:live -->\n\
            <!-- /agent:exchange -->\n";
    std::fs::write(&doc, visible).unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap();
    assert!(
        state.as_ref().is_none_or(|state| !state.is_open()),
        "boundary-artifact-only preflight must not leave an open cycle"
    );

    let log = std::fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap_or_default();
    assert!(
        !log.contains("preflight_diff_start file="),
        "boundary-artifact-only diff must not log preflight_diff_start:\n{log}"
    );
    match crate::session_check::inspect(&doc).unwrap() {
        crate::session_check::SessionCheckStatus::Ok(_) => {}
        status => {
            panic!("expected clean closeout after boundary-artifact-only preflight, got {status:?}")
        }
    }
}

#[test]
fn preflight_recovers_response_captured_cycle_without_pending_file() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    let content = "---\nsession: test\n---\n\n## User\n\nHello\n";
    std::fs::write(&doc, content).unwrap();
    snapshot::save(&doc, content).unwrap();
    crate::repair::save_pending(&doc, "Recovered answer.").unwrap();
    let pending = snapshot::pending_path_for(&doc).unwrap();
    std::fs::remove_file(&pending).unwrap();

    run(&doc).unwrap();

    let state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert_eq!(state.phase, crate::cycle_state::CyclePhase::Committed);
    let result = std::fs::read_to_string(&doc).unwrap();
    assert!(result.contains("Recovered answer."));
}

#[test]
fn preflight_claims_read_and_truncated() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Doc\n").unwrap();
    snapshot::save(&doc, "# Doc\n").unwrap();

    // Write a claims log.
    let log_path = dir.path().join(".agent-doc/claims.log");
    std::fs::write(&log_path, "claim A\nclaim B\n").unwrap();

    let claims = read_and_truncate_claims(&doc);
    assert_eq!(claims, vec!["claim A", "claim B"]);

    // Log should be truncated.
    let after = std::fs::read_to_string(&log_path).unwrap();
    assert!(after.is_empty(), "claims log should be empty after read");
}

#[test]
fn preflight_no_claims_log_returns_empty() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Doc\n").unwrap();

    // No claims.log exists.
    let claims = read_and_truncate_claims(&doc);
    assert!(claims.is_empty());
}

#[test]
fn preflight_output_serializes_correctly() {
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: true,
        claims: vec!["foo".to_string()],
        diff: Some("+new line\n".to_string()),
        no_changes: false,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["recovered"], false);
    assert_eq!(parsed["committed"], true);
    assert_eq!(parsed["claims"][0], "foo");
    assert_eq!(parsed["no_changes"], false);
    assert!(parsed["diff"].as_str().is_some());
    assert!(
        parsed.get("document").is_none(),
        "document field must be absent"
    );
}

#[test]
fn preflight_output_includes_orchestration_request() {
    let output = PreflightOutput {
        no_changes: false,
        orchestration_request: Some(crate::diff::OrchestrationRequest {
            mode: crate::diff::OrchestrationRequestMode::Sequential,
            trigger_text: "Synchronous orcestra.".to_string(),
            task_count: 5,
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["orchestration_request"]["mode"], "sequential");
    assert_eq!(parsed["orchestration_request"]["task_count"], 5);
    assert_eq!(
        parsed["orchestration_request"]["trigger_text"],
        "Synchronous orcestra."
    );
}

#[test]
fn preflight_output_omits_orchestration_request_when_absent() {
    let output = PreflightOutput {
        no_changes: false,
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("orchestration_request").is_none(),
        "orchestration_request should be omitted when absent"
    );
}

#[test]
fn preflight_output_includes_prompt_presets_requested() {
    let output = PreflightOutput {
        no_changes: false,
        prompt_presets_requested: vec!["#1".to_string(), "release-check".to_string()],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["prompt_presets_requested"][0], "#1");
    assert_eq!(parsed["prompt_presets_requested"][1], "release-check");
}

#[test]
fn preflight_output_omits_prompt_presets_requested_when_empty() {
    let output = PreflightOutput {
        no_changes: false,
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("prompt_presets_requested").is_none(),
        "prompt_presets_requested should be omitted when empty"
    );
}

#[test]
fn harness_mismatch_warning_normalizes_aliases() {
    assert!(
        harness_mismatch_warning(Some("claude"), "claude-code").is_none(),
        "claude and claude-code are the same canonical harness"
    );
    let warning = harness_mismatch_warning(Some("codex"), "claude-code").unwrap();
    assert_eq!(warning.code, "harness_mismatch");
    assert_eq!(warning.document_agent.as_deref(), Some("codex"));
    assert_eq!(warning.active_harness.as_deref(), Some("claude-code"));
    assert!(warning.message.contains("Document declares agent: codex"));
}

#[test]
fn harness_mismatch_warning_skips_unknown_active_harness() {
    assert!(harness_mismatch_warning(Some("codex"), "default").is_none());
    assert!(harness_mismatch_warning(None, "claude-code").is_none());
}

#[test]
fn codex_network_access_warning_for_non_codex_harness() {
    let content = "---\nagent_doc_session: test\nagent: opencode\ncodex_network_access: enabled\n---\n\ntest\n";
    let (fm, _) = crate::frontmatter::parse(content).unwrap();
    assert!(
        fm.codex_network_access.is_some(),
        "frontmatter should have codex_network_access"
    );
    let active = "opencode";
    assert_ne!(
        canonical_harness_name(active).as_deref(),
        Some("codex"),
        "opencode should not be canonical codex"
    );
    assert!(
        canonical_harness_name(&active).is_some(),
        "opencode is a known harness"
    );
    let has_guard = canonical_harness_name("codex").as_deref() == Some("codex")
        && canonical_harness_name(active).as_deref() != Some("codex")
        && fm.codex_network_access.is_some();
    assert!(
        has_guard,
        "guard condition should fire for opencode + codex_network_access: enabled"
    );
}

#[test]
fn preflight_output_includes_warnings() {
    let output = PreflightOutput {
        warnings: vec![PreflightWarning {
            code: "harness_mismatch".to_string(),
            message: "Document declares agent: codex but active harness is claude-code."
                .to_string(),
            document_agent: Some("codex".to_string()),
            active_harness: Some("claude-code".to_string()),
        }],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();

    assert_eq!(parsed["warnings"][0]["code"], "harness_mismatch");
    assert_eq!(parsed["warnings"][0]["document_agent"], "codex");
    assert_eq!(parsed["warnings"][0]["active_harness"], "claude-code");
}

#[test]
fn preflight_output_omits_warnings_when_empty() {
    let output = PreflightOutput::default();
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("warnings").is_none(),
        "warnings should be omitted when empty"
    );
}

#[test]
fn preflight_output_null_diff_when_no_changes() {
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: false,
        claims: vec![],
        diff: None,
        no_changes: true,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(parsed["diff"].is_null());
    assert_eq!(parsed["no_changes"], true);
}

#[test]
fn check_layout_returns_empty_outside_tmux() {
    // When TMUX env var is not set (typical in CI / test), check_layout
    // should return an empty vec silently.
    let _env_guard = crate::test_support::env_lock();
    let saved = std::env::var("TMUX").ok();
    // SAFETY: test is single-threaded; we restore the value immediately after.
    unsafe { std::env::remove_var("TMUX") };
    let issues = check_layout();
    // Restore if it was set.
    if let Some(val) = saved {
        unsafe { std::env::set_var("TMUX", val) };
    }
    assert!(
        issues.is_empty(),
        "expected no issues outside tmux, got: {:?}",
        issues
    );
}

#[test]
fn preflight_output_includes_layout_issues() {
    let output = PreflightOutput {
        layout_issues: vec!["window index 0 missing".to_string()],
        recovered: false,
        committed: false,
        claims: vec![],
        diff: None,
        no_changes: true,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["layout_issues"].as_array().unwrap().len(), 1);
    assert_eq!(parsed["layout_issues"][0], "window index 0 missing");
}

#[test]
fn maybe_auto_repair_base_index_removes_stale_counter_without_tmux() {
    let dir = tempfile::tempdir().unwrap();
    let agent_doc_dir = dir.path().join(".agent-doc");
    std::fs::create_dir_all(agent_doc_dir.join("state")).unwrap();
    let counter_path = agent_doc_dir.join("state/base-index-repair.count");
    std::fs::write(&counter_path, "1").unwrap();
    let file = dir.path().join("session.md");
    std::fs::write(&file, "---\n---\n").unwrap();
    let issues = vec!["window index 0 missing in session '0' (base-index compliance)".to_string()];

    let _env_guard = crate::test_support::env_lock();
    let saved_tmux = std::env::var("TMUX").ok();
    // SAFETY: this test restores the process env before returning.
    unsafe { std::env::remove_var("TMUX") };
    let repaired = maybe_auto_repair_base_index(&file, &issues);
    if let Some(val) = saved_tmux {
        unsafe { std::env::set_var("TMUX", val) };
    }
    assert!(!repaired, "outside tmux no repair should run");
    assert!(
        !counter_path.exists(),
        "stale deferred-repair counter should be removed"
    );
}

#[test]
fn maybe_auto_repair_base_index_noop_without_issue() {
    let dir = tempfile::tempdir().unwrap();
    let agent_doc_dir = dir.path().join(".agent-doc");
    std::fs::create_dir_all(agent_doc_dir.join("state")).unwrap();
    let file = dir.path().join("session.md");
    std::fs::write(&file, "---\n---\n").unwrap();
    let issues: Vec<String> = vec![];
    maybe_auto_repair_base_index(&file, &issues);
    let counter_path = agent_doc_dir.join("state/base-index-repair.count");
    assert!(
        !counter_path.exists(),
        "no counter file should be created when no base-index issue"
    );
}

#[test]
fn detect_duplicate_claims_empty_registry() {
    let registry = tmux_router::Registry::new();
    assert!(detect_duplicate_claims(&registry).is_empty());
}

#[test]
fn detect_duplicate_claims_no_duplicates() {
    let mut registry = tmux_router::Registry::new();
    registry.insert(
        "session-a".to_string(),
        tmux_router::RegistryEntry {
            pane: "%1".to_string(),
            pid: 100,
            cwd: "/work".to_string(),
            started: "2026-01-01".to_string(),
            session_id: "session-a".to_string(),
            file: "tasks/foo.md".to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        },
    );
    registry.insert(
        "session-b".to_string(),
        tmux_router::RegistryEntry {
            pane: "%2".to_string(),
            pid: 101,
            cwd: "/work".to_string(),
            started: "2026-01-01".to_string(),
            session_id: "session-b".to_string(),
            file: "tasks/bar.md".to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        },
    );
    assert!(detect_duplicate_claims(&registry).is_empty());
}

#[test]
fn detect_duplicate_claims_two_sessions_same_file() {
    let mut registry = tmux_router::Registry::new();
    registry.insert(
        "session-a".to_string(),
        tmux_router::RegistryEntry {
            pane: "%1".to_string(),
            pid: 100,
            cwd: "/work".to_string(),
            started: "2026-01-01".to_string(),
            session_id: "session-a".to_string(),
            file: "tasks/shared.md".to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        },
    );
    registry.insert(
        "session-b".to_string(),
        tmux_router::RegistryEntry {
            pane: "%2".to_string(),
            pid: 101,
            cwd: "/work".to_string(),
            started: "2026-01-01".to_string(),
            session_id: "session-b".to_string(),
            file: "tasks/shared.md".to_string(),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        },
    );
    let issues = detect_duplicate_claims(&registry);
    assert_eq!(issues.len(), 1);
    assert!(issues[0].contains("duplicate claims"));
    assert!(issues[0].contains("tasks/shared.md"));
    assert!(issues[0].contains("session-a"));
    assert!(issues[0].contains("session-b"));
}

#[test]
fn detect_duplicate_claims_skips_empty_file_entries() {
    let mut registry = tmux_router::Registry::new();
    registry.insert(
        "session-a".to_string(),
        tmux_router::RegistryEntry {
            pane: "%1".to_string(),
            pid: 100,
            cwd: "/work".to_string(),
            started: "2026-01-01".to_string(),
            session_id: "session-a".to_string(),
            file: String::new(), // legacy entry — no file
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        },
    );
    registry.insert(
        "session-b".to_string(),
        tmux_router::RegistryEntry {
            pane: "%2".to_string(),
            pid: 101,
            cwd: "/work".to_string(),
            started: "2026-01-01".to_string(),
            session_id: "session-b".to_string(),
            file: String::new(),
            window: "@1".to_string(),
            supervisor_instance_id: String::new(),
        },
    );
    assert!(detect_duplicate_claims(&registry).is_empty());
}

#[test]
fn is_url_detects_http() {
    assert!(is_url("http://example.com"));
    assert!(is_url("https://example.com/path"));
    assert!(!is_url("../relative/path.md"));
    assert!(!is_url("tasks/software/agent-doc.md"));
    assert!(!is_url(""));
}

#[test]
fn is_html_content_detects_html() {
    assert!(is_html_content("text/html; charset=utf-8"));
    assert!(is_html_content("text/html"));
    assert!(is_html_content("application/xhtml+xml"));
    assert!(!is_html_content("application/json"));
    assert!(!is_html_content("text/plain"));
}

#[test]
fn html_to_markdown_converts_basic_html() {
    let html = "<h1>Title</h1><p>Hello <strong>world</strong>.</p>";
    let md = html_to_markdown(html);
    assert!(md.contains("Title"), "should contain heading text");
    assert!(md.contains("**world**"), "should convert bold");
}

#[test]
fn html_to_markdown_strips_script_and_style() {
    let html =
        "<p>Visible</p><script>alert('xss')</script><style>.foo{}</style><p>Also visible</p>";
    let md = html_to_markdown(html);
    assert!(md.contains("Visible"));
    assert!(md.contains("Also visible"));
    assert!(!md.contains("alert"), "script content should be stripped");
    assert!(!md.contains(".foo"), "style content should be stripped");
}

#[test]
fn html_to_markdown_strips_nav_and_footer() {
    let html =
        "<nav><a href='/'>Home</a></nav><main><p>Content</p></main><footer>Copyright</footer>";
    let md = html_to_markdown(html);
    assert!(md.contains("Content"));
    assert!(!md.contains("Home"), "nav content should be stripped");
    assert!(
        !md.contains("Copyright"),
        "footer content should be stripped"
    );
}

#[test]
fn url_cache_path_is_deterministic() {
    let dir = TempDir::new().unwrap();
    let p1 = url_cache_path(dir.path(), "https://example.com");
    let p2 = url_cache_path(dir.path(), "https://example.com");
    assert_eq!(p1, p2, "same URL should produce same cache path");

    let p3 = url_cache_path(dir.path(), "https://other.com");
    assert_ne!(
        p1, p3,
        "different URLs should produce different cache paths"
    );
    assert!(p1.extension().unwrap() == "txt");
}

#[test]
fn links_cache_dir_creates_directory() {
    let dir = setup_project();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Doc\n").unwrap();

    let cache = links_cache_dir(&doc);
    assert!(cache.is_some());
    let cache_path = cache.unwrap();
    assert!(cache_path.exists());
    assert!(cache_path.ends_with("links_cache"));
}

#[test]
fn preflight_output_includes_baseline_file() {
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: true,
        claims: vec![],
        diff: None,
        no_changes: true,
        linked_changes: vec![],
        baseline_file: Some("/tmp/baseline.md".to_string()),
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["baseline_file"], "/tmp/baseline.md");
}

#[test]
fn preflight_output_omits_baseline_file_when_none() {
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: false,
        claims: vec![],
        diff: None,
        no_changes: true,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("baseline_file").is_none(),
        "baseline_file should be omitted when None"
    );
}

#[test]
fn preflight_output_includes_diff_type_when_set() {
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: true,
        claims: vec![],
        diff: Some("+go\n".to_string()),
        no_changes: false,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: Some("approval".to_string()),
        diff_type_reason: Some("single approval word: \"go\"".to_string()),
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["diff_type"], "approval");
    assert!(parsed["diff_type_reason"].as_str().unwrap().contains("go"));
}

#[test]
fn preflight_output_omits_diff_type_when_none() {
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: false,
        claims: vec![],
        diff: None,
        no_changes: true,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("diff_type").is_none(),
        "diff_type should be omitted when None"
    );
    assert!(
        parsed.get("diff_type_reason").is_none(),
        "diff_type_reason should be omitted when None"
    );
}

#[test]
fn preflight_output_includes_annotated_diff_when_set() {
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: true,
        claims: vec![],
        diff: Some("+line\n".to_string()),
        no_changes: false,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: Some("[user+] line".to_string()),
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["annotated_diff"], "[user+] line");
}

#[test]
fn preflight_output_omits_annotated_diff_when_none() {
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: false,
        claims: vec![],
        diff: None,
        no_changes: true,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("annotated_diff").is_none(),
        "annotated_diff should be omitted when None"
    );
}

#[test]
fn semantic_diff_summary_reports_components_nodes_and_prompt_previews() {
    let before = concat!(
        "---\n",
        "queue: stop\n",
        "---\n\n",
        "<!-- agent:queue -->\n",
        "- do [#alpha]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#task] old wording\n",
        "<!-- /agent:backlog -->\n"
    );
    let current = concat!(
        "---\n",
        "queue: go\n",
        "---\n\n",
        "<!-- agent:queue -->\n",
        "- do [#alpha]\n",
        "- do [#beta]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#task] new wording\n",
        "<!-- /agent:backlog -->\n"
    );
    let prompt_changes = vec![crate::diff::PromptBearingChange {
        kind: crate::diff::PromptBearingChangeKind::PromptTarget,
        text: "do [#beta]".to_string(),
    }];

    let summary = semantic_diff_summary(before, current, &prompt_changes).unwrap();

    assert_eq!(summary.schema_version, 1);
    assert!(
        summary
            .changed_components
            .contains(&"frontmatter".to_string())
    );
    assert!(summary.changed_components.contains(&"queue".to_string()));
    assert!(summary.changed_components.contains(&"backlog".to_string()));
    assert!(summary.changed_components.contains(&"exchange".to_string()));
    assert!(summary.component_changes.iter().any(|change| {
        change.component == "frontmatter" && change.op == SemanticComponentOp::Changed
    }));
    assert!(summary.component_changes.iter().any(|change| {
        change.component == "queue"
            && change.op == SemanticComponentOp::Changed
            && change
                .after
                .as_ref()
                .is_some_and(|target| target.handle == "component:after:queue:0")
    }));
    assert!(summary.component_changes.iter().any(|change| {
        change.component == "backlog" && change.op == SemanticComponentOp::Changed
    }));
    assert!(summary.node_events.iter().any(|event| {
        event.component == "queue"
            && event.op == "insert"
            && event.node_key == "queue:0:beta:0"
            && event.after_preview.as_deref() == Some("- do [#beta]")
    }));
    assert_eq!(
        summary.prompt_changes[0].kind,
        crate::diff::PromptBearingChangeKind::PromptTarget
    );
    assert_eq!(summary.prompt_changes[0].text_preview, "do [#beta]");
}

#[test]
fn semantic_diff_summary_omits_empty_summary() {
    assert!(semantic_diff_summary("same\n", "same\n", &[]).is_none());
}

#[test]
fn sibling_queue_insert_beside_driver_is_independent() {
    // The motivating case: the turn answers queue item A while the user
    // inserts queue item B beside it. B must classify Independent and the
    // turn must not be affected (#op-scoped-drift-3).
    let before = "<!-- agent:queue -->\n- do [#driver-a]\n<!-- /agent:queue -->\n";
    let after =
        "<!-- agent:queue -->\n- do [#driver-a]\n- do [#sibling-b]\n<!-- /agent:queue -->\n";
    let summary = semantic_diff_summary(before, after, &[]).unwrap();
    let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary);
    // The turn is answering driver-a.
    let scope = derive_turn_scope(after, &["do [#driver-a]".to_string()]).unwrap();
    let affectedness = agent_doc_core::turn_scope::classify_cycle(&ops, &scope);
    assert!(
        !affectedness.turn_affected,
        "a sibling queue insert must not affect the turn"
    );
    assert!(
        affectedness
            .classified
            .iter()
            .all(|op| op.class == agent_doc_core::turn_scope::AffectednessClass::Independent)
    );
}

#[test]
fn exchange_old_block_edit_is_independent_but_tail_append_affects() {
    // #loop-guard-exchange-node-granularity end-to-end: while the turn answers
    // a queue driver, an edit to an OLD bulleted exchange block must classify
    // Independent (must not preempt the auto-loop drain), while a genuine new
    // bulleted prompt appended at the exchange tail must still affect the turn.
    let base = "\
<!-- agent:exchange -->
### Re: prior topic

- old context bullet one
- old context bullet two
<!-- agent:boundary:b1 -->
<!-- /agent:exchange -->

<!-- agent:queue go -->
- do [#driver]
<!-- /agent:queue -->
";
    let targets = vec!["do [#driver]".to_string()];
    let scope = derive_turn_scope(base, &targets).expect("scope derived");
    assert_eq!(
        scope.exchange_tail_floor,
        Some(2),
        "two committed exchange bullets => tail floor 2"
    );

    // Old-block edit: change the FIRST (index 0) exchange bullet.
    let old_edit = base.replace(
        "- old context bullet one",
        "- old context bullet one EDITED",
    );
    let summary = semantic_diff_summary(base, &old_edit, &[]).unwrap();
    let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary);
    let affectedness = agent_doc_core::turn_scope::classify_cycle(&ops, &scope);
    assert!(
        !affectedness.turn_affected,
        "editing an old exchange block must not affect the turn: {:?}",
        affectedness.classified
    );

    // Tail append: a new bulleted prompt after the last committed bullet.
    let tail_append = base.replace(
        "- old context bullet two\n",
        "- old context bullet two\n- please also cover the retry path\n",
    );
    let summary2 = semantic_diff_summary(base, &tail_append, &[]).unwrap();
    let ops2 = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "", &summary2);
    let affectedness2 = agent_doc_core::turn_scope::classify_cycle(&ops2, &scope);
    assert!(
        affectedness2.turn_affected,
        "a new tail-appended exchange prompt must still affect the turn: {:?}",
        affectedness2.classified
    );
}

#[test]
fn derive_turn_scope_resolves_queue_driver_and_sets() {
    let content =
        "<!-- agent:queue -->\n- do [#op-scoped-drift-2]\n- do [#later]\n<!-- /agent:queue -->\n";
    let targets = vec!["do [#op-scoped-drift-2]".to_string()];
    let scope = derive_turn_scope(content, &targets).expect("scope derived");
    let driver = scope.driver.as_ref().expect("driver resolved");
    assert_eq!(driver.component, "queue");
    assert_eq!(
        driver.node_key.as_deref(),
        Some("queue:0:op-scoped-drift-2:0")
    );
    // driver is read (input) and written (the strike).
    assert!(scope.read_set.contains(driver));
    assert!(scope.write_set.contains(driver));
    assert!(
        scope
            .write_set
            .contains(&agent_doc_core::turn_scope::Address::component(
                "backlog", 0
            ))
    );
}

#[test]
fn derive_turn_scope_none_without_prompt_targets() {
    let content = "<!-- agent:queue -->\n- do [#x]\n<!-- /agent:queue -->\n";
    assert!(derive_turn_scope(content, &[]).is_none());
}

#[test]
fn derive_turn_scope_without_matching_queue_node_has_no_driver() {
    // A prompt target whose id is not present in the queue still yields a
    // scope (output components) but no driver.
    let content = "<!-- agent:queue -->\n- do [#present]\n<!-- /agent:queue -->\n";
    let targets = vec!["do [#absent]".to_string()];
    let scope = derive_turn_scope(content, &targets).expect("scope derived");
    assert!(scope.driver.is_none());
    assert!(scope.write_set.iter().all(|a| a.component != "queue"));
}

fn user_prompt_change(text: &str) -> crate::diff::PromptBearingChange {
    crate::diff::PromptBearingChange {
        kind: crate::diff::PromptBearingChangeKind::PromptTarget,
        text: text.to_string(),
    }
}

fn affectedness(turn_affected: bool) -> agent_doc_core::turn_scope::CycleAffectedness {
    use agent_doc_core::turn_scope::{AffectednessClass, ClassifiedOp};
    agent_doc_core::turn_scope::CycleAffectedness {
        turn_affected,
        classified: vec![ClassifiedOp {
            component: "queue".to_string(),
            node_key: "queue:0:other:0".to_string(),
            op_kind: "move".to_string(),
            actor: agent_doc_core::op_log::OpActor::User,
            class: if turn_affected {
                AffectednessClass::InputAffecting
            } else {
                AffectednessClass::Independent
            },
        }],
    }
}

#[test]
fn user_intent_empty_for_synthetic_queue_continuation() {
    // A pure auto-queue continuation is never user intent, regardless of the
    // affectedness verdict.
    let changes = vec![user_prompt_change("do [#next]")];
    assert!(
        compute_user_intent_prompt_changes(&changes, true, Some(&affectedness(true))).is_empty()
    );
}

#[test]
fn user_intent_drops_turn_independent_edits() {
    // #queue-no-stop-unrelated-edit: a real (non-managed) edit that the
    // classifier scoped as independent of the turn must NOT halt the drain.
    let changes = vec![user_prompt_change("a stray note in the parking lot")];
    let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(false)));
    assert!(
        out.is_empty(),
        "independent edit should not preempt: {out:?}"
    );
}

#[test]
fn user_intent_keeps_turn_affecting_prompt() {
    // A genuine new user prompt edits the in-scope exchange tail, so the
    // classifier reports turn_affected — it must still preempt.
    let changes = vec![user_prompt_change("please also handle the error case")];
    let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(true)));
    assert_eq!(out.len(), 1, "turn-affecting prompt must preempt");
}

#[test]
fn user_intent_filters_managed_state_when_turn_affected() {
    // Even when the turn is affected, managed-component bookkeeping (a backlog
    // item line) stays filtered — it is not a real prompt.
    let changes = vec![crate::diff::PromptBearingChange {
        kind: crate::diff::PromptBearingChangeKind::ContentEdit,
        text: "- [ ] [#newitem] track a follow-up".to_string(),
    }];
    let out = compute_user_intent_prompt_changes(&changes, false, Some(&affectedness(true)));
    assert!(
        out.is_empty(),
        "managed-state edit must not preempt: {out:?}"
    );
}

#[test]
fn user_intent_conservative_without_classifier() {
    // No affectedness classifier (semantic-diff skip): fall back to the
    // managed-state filter only, so a real change still preempts.
    let changes = vec![user_prompt_change("a real prompt with no classifier")];
    let out = compute_user_intent_prompt_changes(&changes, false, None);
    assert_eq!(out.len(), 1, "without classifier, a real change preempts");
}

#[test]
fn extract_target_id_handles_bracket_and_bare_forms() {
    assert_eq!(
        extract_target_id("do [#op-scoped-drift-2]").as_deref(),
        Some("op-scoped-drift-2")
    );
    assert_eq!(extract_target_id("do #fix1").as_deref(), Some("fix1"));
    assert_eq!(extract_target_id("no id here"), None);
}

#[test]
fn build_ops_from_semantic_diff_tags_user_actor_and_session() {
    let before = "<!-- agent:queue -->\n- do [#alpha]\n<!-- /agent:queue -->\n";
    let after = "<!-- agent:queue -->\n- do [#alpha]\n- do [#beta]\n<!-- /agent:queue -->\n";
    let summary = semantic_diff_summary(before, after, &[]).unwrap();
    let ops = build_ops_from_semantic_diff("plan.md", Some("sess-1"), "100", &summary);
    assert!(!ops.is_empty());
    let beta = ops
        .iter()
        .find(|op| op.node_key == "queue:0:beta:0")
        .expect("beta op present");
    assert_eq!(beta.actor, agent_doc_core::op_log::OpActor::User);
    assert_eq!(beta.op_kind, "insert");
    assert_eq!(beta.component, "queue");
    assert_eq!(beta.clock.origin_session.as_deref(), Some("sess-1"));
    // Lamport assignment is owned by the durable store; the builder leaves 0.
    assert_eq!(beta.clock.lamport, 0);
}

#[test]
fn preflight_output_includes_semantic_diff_when_set() {
    let output = PreflightOutput {
        semantic_diff: Some(SemanticDiffSummary {
            schema_version: 1,
            changed_components: vec!["queue".to_string()],
            node_events: vec![SemanticNodeEvent {
                component: "queue".to_string(),
                node_key: "queue:0:task:0".to_string(),
                op: "insert".to_string(),
                item_id: "task".to_string(),
                before_index: None,
                after_index: Some(0),
                previous_node_key: None,
                next_node_key: None,
                before_preview: None,
                after_preview: Some("- do [#task]".to_string()),
            }],
            ..Default::default()
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["semantic_diff"]["schema_version"], 1);
    assert_eq!(parsed["semantic_diff"]["changed_components"][0], "queue");
    assert_eq!(parsed["semantic_diff"]["node_events"][0]["op"], "insert");
}

#[test]
fn preflight_output_omits_semantic_diff_when_none() {
    let output = PreflightOutput::default();
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("semantic_diff").is_none(),
        "semantic_diff should be omitted when absent"
    );
}

#[test]
fn preflight_output_includes_inline_annotations() {
    let output = PreflightOutput {
        inline_annotations: vec![
            "This is wrong, fix it".to_string(),
            "Broaden the gate".to_string(),
        ],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let anns = parsed["inline_annotations"].as_array().unwrap();
    assert_eq!(anns.len(), 2);
    assert_eq!(anns[0], "This is wrong, fix it");
    assert_eq!(anns[1], "Broaden the gate");
}

#[test]
fn preflight_output_omits_inline_annotations_when_empty() {
    let output = PreflightOutput {
        inline_annotations: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("inline_annotations").is_none(),
        "inline_annotations should be omitted when empty"
    );
}

#[test]
fn preflight_output_includes_prompt_bearing_changes() {
    let output = PreflightOutput {
        prompt_bearing_changes: vec![
            crate::diff::PromptBearingChange {
                kind: crate::diff::PromptBearingChangeKind::PromptTarget,
                text: "❯ Why was this missed?".to_string(),
            },
            crate::diff::PromptBearingChange {
                kind: crate::diff::PromptBearingChangeKind::ContentEdit,
                text: "This line should say 503, not 401.".to_string(),
            },
        ],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    let changes = parsed["prompt_bearing_changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    assert_eq!(changes[0]["kind"], "prompt_target");
    assert_eq!(changes[0]["text"], "❯ Why was this missed?");
    assert_eq!(changes[1]["kind"], "content_edit");
}

#[test]
fn preflight_output_omits_prompt_bearing_changes_when_empty() {
    let output = PreflightOutput {
        prompt_bearing_changes: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("prompt_bearing_changes").is_none(),
        "prompt_bearing_changes should be omitted when empty"
    );
}

#[test]
fn preflight_output_includes_session_accretion_when_present() {
    let output = PreflightOutput {
        session_accretion: Some(crate::session_accretion::SessionAccretionReport {
            level: crate::session_accretion::SessionAccretionLevel::Warn,
            exchange_lines: 220,
            response_sections: 9,
            recent_committed_cycles: 7,
            recent_noop_closeouts: 2,
            recent_restart_count: 0,
            recent_session_loss_count: 0,
            startup_miss_active: false,
            clear_threshold: 50,
            reasons: vec!["exchange has grown".to_string()],
            guidance: vec!["Run `agent-doc compact session.md --commit`.".to_string()],
        }),
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["session_accretion"]["level"], "warn");
    assert_eq!(parsed["session_accretion"]["exchange_lines"], 220);
}

#[test]
fn preflight_output_omits_session_accretion_when_absent() {
    let output = PreflightOutput::default();
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("session_accretion").is_none(),
        "session_accretion should be omitted when absent"
    );
}

#[test]
fn preflight_output_slash_commands_from_diff() {
    // /clear is a built-in command — goes to builtin_commands, not slash_commands
    let diff = "--- snapshot\n+++ document\n@@ -1 +1,2 @@\n ctx\n+/clear\n";
    let parsed_cmds = crate::diff::parse_slash_commands_classified(diff);
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: false,
        claims: vec![],
        diff: Some(diff.to_string()),
        no_changes: false,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: parsed_cmds.skill_commands,
        builtin_commands: parsed_cmds.builtin_commands,
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    // /clear is a built-in — appears in builtin_commands, not slash_commands
    assert_eq!(parsed["builtin_commands"][0], "/clear");
    assert!(
        parsed["slash_commands"].is_null()
            || parsed["slash_commands"]
                .as_array()
                .is_none_or(|a| a.is_empty())
    );
}

#[test]
fn preflight_output_no_document_field() {
    // The `document` field was removed — it must not appear in serialized JSON.
    // Having it would send full document content to the agent every cycle,
    // wasting tokens on every invocation.
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: false,
        claims: vec![],
        diff: None,
        no_changes: true,
        linked_changes: vec![],
        baseline_file: None,
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert!(
        parsed.get("document").is_none(),
        "document key must be absent from preflight JSON — it would waste tokens on every cycle"
    );
}

#[test]
fn preflight_output_no_large_content() {
    // Regression: preflight JSON must not embed document content.
    // Any field containing the full file body would be sent to the agent
    // on every cycle, burning tokens proportional to document size.
    let large_content = "x".repeat(10_000);
    let output = PreflightOutput {
        layout_issues: vec![],
        recovered: false,
        committed: false,
        claims: vec![],
        diff: Some(format!("+{large_content}")), // diff can include content
        no_changes: false,
        linked_changes: vec![],
        baseline_file: Some("/tmp/baseline.md".to_string()),
        diff_type: None,
        diff_type_reason: None,
        annotated_diff: None,
        slash_commands: vec![],
        builtin_commands: vec![],
        ..Default::default()
    };
    let json = serde_json::to_string(&output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    // Only `diff` may contain the large content (it's the actual user change).
    // No OTHER field should contain it.
    let diff_str = parsed["diff"].as_str().unwrap_or("");
    for (key, val) in parsed.as_object().unwrap() {
        if key == "diff" {
            continue;
        }
        let val_str = val.to_string();
        assert!(
            !val_str.contains(&large_content),
            "field `{key}` contains large content — this would waste tokens on every preflight cycle"
        );
        assert!(
            val_str.len() < 1_000 || key == "annotated_diff",
            "field `{key}` is suspiciously large ({} bytes) — preflight should not embed document content",
            val_str.len()
        );
    }
    // Diff itself is allowed to contain the content
    assert!(diff_str.contains(&large_content));
}

// --- Fix 5: cross-document sweep ---

#[test]
fn preflight_sweep_commits_other_tracked_docs() {
    use std::fs;
    let dir = setup_project();
    let root = dir.path();

    // Create initial commit so HEAD exists
    let readme = root.join("README.md");
    fs::write(&readme, "# project\n").unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "README.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .output()
        .unwrap();

    // Primary doc (the one preflight runs on)
    let primary = root.join("primary.md");
    let primary_content = "---\nagent_doc_session: primary\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n\n## User\n\n";
    fs::write(&primary, primary_content).unwrap();
    snapshot::save(&primary, primary_content).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "primary.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add primary", "--no-verify"])
        .output()
        .unwrap();

    // Secondary doc (tracked in sessions.json, snapshot newer than file — needs sweep)
    let secondary = root.join("secondary.md");
    let secondary_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
    fs::write(&secondary, secondary_content).unwrap();
    snapshot::save(&secondary, secondary_content).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "secondary.md"])
        .output()
        .unwrap();
    // Backdate the commit so the <5s freshness gate in sweep doesn't skip it.
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add secondary", "--no-verify"])
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .output()
        .unwrap();

    // Touch snapshot to make it newer than the file (simulates agent write without commit)
    let snap_rel = snapshot::path_for(&secondary).unwrap();
    let snap_abs = root.join(&snap_rel);
    let new_snap = format!("{}\n<!-- agent updated -->", secondary_content);
    fs::write(&snap_abs, &new_snap).unwrap();

    // Write sessions.json with secondary tracked
    let sessions_path = root.join(".agent-doc/sessions.json");
    let sessions = serde_json::json!({
        "secondary-session": {
            "pane": "%1",
            "pid": 9999,
            "cwd": root.to_string_lossy(),
            "started": "2026-01-01",
            "file": "secondary.md",
            "window": "@1"
        }
    });
    fs::write(
        &sessions_path,
        serde_json::to_string_pretty(&sessions).unwrap(),
    )
    .unwrap();

    // Run preflight on primary — sweep should commit secondary
    run(&primary).unwrap();

    // Verify secondary was committed by the sweep
    let log = Command::new("git")
        .current_dir(root)
        .args(["log", "--oneline", "-4"])
        .output()
        .unwrap();
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(
        log_str.contains("agent-doc(secondary):"),
        "preflight sweep should have committed secondary.md, got:\n{log_str}"
    );
}

#[test]
fn preflight_sweep_skips_doc_with_unresponded_user_content() {
    use std::fs;
    let dir = setup_project();
    let root = dir.path();

    // Create initial commit so HEAD exists
    let readme = root.join("README.md");
    fs::write(&readme, "# project\n").unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "README.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "initial", "--no-verify"])
        .output()
        .unwrap();

    // Primary doc (the one preflight runs on)
    let primary = root.join("primary.md");
    let primary_content = "---\nagent_doc_session: primary\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n\n## User\n\n";
    fs::write(&primary, primary_content).unwrap();
    snapshot::save(&primary, primary_content).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "primary.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add primary", "--no-verify"])
        .output()
        .unwrap();

    // Secondary doc with agent response in snapshot but user added new content in document
    let secondary = root.join("secondary.md");
    let snap_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
    // Document has user additions not in the snapshot
    let doc_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\nNew question from user\n";
    fs::write(&secondary, doc_content).unwrap();
    snapshot::save(&secondary, snap_content).unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["add", "secondary.md"])
        .output()
        .unwrap();
    Command::new("git")
        .current_dir(root)
        .args(["commit", "-m", "add secondary", "--no-verify"])
        .env("GIT_COMMITTER_DATE", "2026-01-01T00:00:00Z")
        .env("GIT_AUTHOR_DATE", "2026-01-01T00:00:00Z")
        .output()
        .unwrap();

    // Touch snapshot to make it newer than the file
    let snap_rel = snapshot::path_for(&secondary).unwrap();
    let snap_abs = root.join(&snap_rel);
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(&snap_abs, snap_content).unwrap();

    // Write sessions.json with secondary tracked
    let sessions_path = root.join(".agent-doc/sessions.json");
    let sessions = serde_json::json!({
        "secondary-session": {
            "pane": "%1",
            "pid": 9999,
            "cwd": root.to_string_lossy(),
            "started": "2026-01-01",
            "file": "secondary.md",
            "window": "@1"
        }
    });
    fs::write(
        &sessions_path,
        serde_json::to_string_pretty(&sessions).unwrap(),
    )
    .unwrap();

    // Count commits before sweep
    let log_before = Command::new("git")
        .current_dir(root)
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    let count_before = String::from_utf8_lossy(&log_before.stdout).lines().count();

    // Run preflight on primary — sweep should SKIP secondary due to user additions
    run(&primary).unwrap();

    // Verify secondary was NOT committed
    let log_after = Command::new("git")
        .current_dir(root)
        .args(["log", "--oneline"])
        .output()
        .unwrap();
    let log_str = String::from_utf8_lossy(&log_after.stdout);
    assert!(
        !log_str.contains("agent-doc(secondary):"),
        "preflight sweep should NOT have committed secondary.md (has unresponded user content), got:\n{log_str}"
    );
    // Only primary should have been committed (by step 2, not sweep)
    let count_after = log_str.lines().count();
    assert!(
        count_after <= count_before + 1,
        "expected at most one new commit (primary), got {} new commits",
        count_after - count_before
    );
}

#[test]
fn preflight_sweep_skips_foreign_owned_doc() {
    use std::fs;
    let dir = setup_project();
    let root = dir.path();
    initialize_git_head(root);

    let primary_content = "---\nagent_doc_session: primary\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n\n## User\n\n";
    let primary = write_committed_doc(root, "primary.md", primary_content, "add primary", None);

    let secondary_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
    let secondary = write_committed_doc(
        root,
        "secondary.md",
        secondary_content,
        "add secondary",
        Some("2026-01-01T00:00:00Z"),
    );

    let snap_rel = snapshot::path_for(&secondary).unwrap();
    let snap_abs = root.join(&snap_rel);
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(
        &snap_abs,
        format!("{}\n<!-- agent updated -->", secondary_content),
    )
    .unwrap();

    write_sessions_json(
        root,
        &[
            ("primary-session", "%70", &primary, "@1", "2026-01-01"),
            ("secondary-session", "%73", &secondary, "@2", "2026-01-01"),
        ],
    );
    crate::session_actor::project_binding_in(
        root,
        &primary.to_string_lossy(),
        "primary-session",
        "%70",
        "@1",
        "test",
        "primary_owner",
    )
    .unwrap();
    crate::session_actor::project_binding_in(
        root,
        &secondary.to_string_lossy(),
        "secondary-session",
        "%73",
        "@2",
        "test",
        "secondary_owner",
    )
    .unwrap();

    run(&primary).unwrap();

    let log = Command::new("git")
        .current_dir(root)
        .args(["log", "--oneline", "-4"])
        .output()
        .unwrap();
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(
        !log_str.contains("agent-doc(secondary):"),
        "foreign-owned secondary.md must not be sweep-committed, got:\n{log_str}"
    );

    let head_secondary = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:secondary.md"])
        .output()
        .unwrap();
    let head_secondary = String::from_utf8_lossy(&head_secondary.stdout);
    assert!(
        !head_secondary.contains("agent updated"),
        "foreign-owned snapshot drift must stay out of HEAD:\n{head_secondary}"
    );

    let ops_log = fs::read_to_string(root.join(".agent-doc/logs/ops.log")).unwrap();
    assert!(
        ops_log.contains("foreign_owned_sweep_skip")
            && ops_log.contains("owner_pane=%73")
            && ops_log.contains("current_pane=%70"),
        "foreign-owned skip should be logged for audit:\n{ops_log}"
    );
}

#[test]
fn preflight_sweep_commits_same_owner_doc() {
    use std::fs;
    let dir = setup_project();
    let root = dir.path();
    initialize_git_head(root);

    let primary_content = "---\nagent_doc_session: primary\n---\n\n## User\n\nHello\n\n## Assistant\n\nReply\n\n## User\n\n";
    let primary = write_committed_doc(root, "primary.md", primary_content, "add primary", None);

    let secondary_content = "---\nagent_doc_session: secondary\n---\n\n## User\n\nHi\n\n## Assistant\n\nResponse\n\n## User\n\n";
    let secondary = write_committed_doc(
        root,
        "secondary.md",
        secondary_content,
        "add secondary",
        Some("2026-01-01T00:00:00Z"),
    );

    let snap_rel = snapshot::path_for(&secondary).unwrap();
    let snap_abs = root.join(&snap_rel);
    std::thread::sleep(std::time::Duration::from_millis(50));
    fs::write(
        &snap_abs,
        format!("{}\n<!-- agent updated -->", secondary_content),
    )
    .unwrap();

    write_sessions_json(
        root,
        &[
            ("primary-session", "%70", &primary, "@1", "2026-01-01"),
            ("secondary-session", "%70", &secondary, "@1", "2026-01-01"),
        ],
    );
    crate::session_actor::project_binding_in(
        root,
        &primary.to_string_lossy(),
        "primary-session",
        "%70",
        "@1",
        "test",
        "primary_owner",
    )
    .unwrap();
    // Leave the sibling owner in sessions.json so this exercises the sweep
    // fallback projection without seeding an invalid two-document actor
    // alias for pane %70.

    run(&primary).unwrap();

    let log = Command::new("git")
        .current_dir(root)
        .args(["log", "--oneline", "-4"])
        .output()
        .unwrap();
    let log_str = String::from_utf8_lossy(&log.stdout);
    assert!(
        log_str.contains("agent-doc(secondary):"),
        "same-owner secondary.md should still be sweep-committed, got:\n{log_str}"
    );

    let head_secondary = Command::new("git")
        .current_dir(root)
        .args(["show", "HEAD:secondary.md"])
        .output()
        .unwrap();
    let head_secondary = String::from_utf8_lossy(&head_secondary.stdout);
    assert!(
        head_secondary.contains("agent updated"),
        "same-owner snapshot drift should land in HEAD:\n{head_secondary}"
    );
}

// --- #cce5: resolve_agent_model / short_model_name tests ---

#[test]
fn short_model_name_strips_claude_prefix() {
    assert_eq!(short_model_name("claude-sonnet-4-6"), "sonnet-4-6");
    assert_eq!(short_model_name("claude-opus-4"), "opus-4");
    assert_eq!(short_model_name("claude-haiku-4-5"), "haiku-4-5");
}

#[test]
fn short_model_name_returns_as_is_without_prefix() {
    assert_eq!(short_model_name("sonnet-4-6"), "sonnet-4-6");
    assert_eq!(short_model_name("gpt-4o"), "gpt-4o");
    assert_eq!(short_model_name("gpt-5"), "gpt-5");
    assert_eq!(short_model_name("gpt-5.4"), "gpt-5.4");
    assert_eq!(short_model_name("opus-4-6"), "opus-4-6");
    assert_eq!(short_model_name(""), "");
}

#[test]
fn resolve_agent_model_uses_frontmatter_only() {
    // ANTHROPIC_MODEL env var is deliberately ignored — only frontmatter matters.
    let cfg = agent_doc_core::model_tier::ModelConfig::default();
    let result = resolve_agent_model(Some("claude-opus-4"), "claude-code", &cfg);
    assert_eq!(result, Some("opus-4".to_string()));
}

#[test]
fn resolve_agent_model_strips_claude_prefix_from_frontmatter() {
    let cfg = agent_doc_core::model_tier::ModelConfig::default();
    let result = resolve_agent_model(Some("claude-haiku-4-5"), "claude-code", &cfg);
    assert_eq!(result, Some("haiku-4-5".to_string()));
}

#[test]
fn resolve_agent_model_defers_claude_code_opus_alias() {
    // The bare `opus` alias is deferred: agent-doc pins no version, so
    // attribution returns None and the running skill self-stamps its real
    // model identity (always the current opus).
    let cfg = agent_doc_core::model_tier::ModelConfig::default();
    let result = resolve_agent_model(Some("opus"), "claude-code", &cfg);
    assert_eq!(result, None);
}

#[test]
fn resolve_agent_model_stamps_pinned_concrete_opus() {
    // An explicitly pinned concrete opus id still stamps its short name.
    let cfg = agent_doc_core::model_tier::ModelConfig::default();
    let result = resolve_agent_model(Some("claude-opus-4-8"), "claude-code", &cfg);
    assert_eq!(result, Some("opus-4-8".to_string()));
}

#[test]
fn resolve_agent_model_preserves_short_openai_style_name() {
    let cfg = agent_doc_core::model_tier::ModelConfig::default();
    let result = resolve_agent_model(Some("gpt-5"), "codex", &cfg);
    assert_eq!(result, Some("gpt-5".to_string()));
}

#[test]
fn resolve_agent_model_none_when_no_frontmatter() {
    // No frontmatter → None, regardless of env var state.
    let cfg = agent_doc_core::model_tier::ModelConfig::default();
    let result = resolve_agent_model(None, "claude-code", &cfg);
    assert_eq!(result, None);
}

#[test]
fn resolve_pipeline_state_none_without_cycle_or_frontmatter() {
    let dir = setup_project();
    let doc = dir.path().join("doc.md");
    std::fs::write(&doc, "body\n").unwrap();
    assert!(resolve_pipeline_state(&doc).unwrap().is_none());
}

#[test]
fn resolve_pipeline_state_falls_back_to_frontmatter_block() {
    // No cycle-state on disk → read the document `agent_doc_pipeline:` mirror.
    let dir = setup_project();
    let doc = dir.path().join("doc.md");
    std::fs::write(
        &doc,
        "---\nagent_doc_pipeline:\n  run_id: cycle-77\n  step: write_applied\n---\n\nbody\n",
    )
    .unwrap();
    let p = resolve_pipeline_state(&doc)
        .unwrap()
        .expect("frontmatter fallback");
    assert_eq!(p.run_id.as_deref(), Some("cycle-77"));
    assert_eq!(p.step.as_deref(), Some("write_applied"));
}

#[test]
fn resolve_pipeline_state_cycle_state_wins_over_frontmatter() {
    // Cycle-state is authoritative; a stale frontmatter block must not override it.
    let dir = setup_project();
    let doc = dir.path().join("doc.md");
    std::fs::write(
        &doc,
        "---\nagent_doc_pipeline:\n  run_id: stale-mirror\n  step: committed\n---\n\nbody\n",
    )
    .unwrap();
    let state = crate::cycle_state::start_preflight_with_task(
        &doc,
        Some("snap"),
        Some("body"),
        Some("#fmrunid-wire"),
        Some("#fmrunid-wire"),
    )
    .unwrap();

    let p = resolve_pipeline_state(&doc)
        .unwrap()
        .expect("cycle-state present");
    assert_eq!(p.run_id.as_deref(), Some(state.cycle_id.as_str()));
    assert_eq!(p.step.as_deref(), Some("preflight_started"));
    assert_eq!(p.turn_id.as_deref(), Some("#fmrunid-wire"));
    assert_ne!(p.run_id.as_deref(), Some("stale-mirror"));
}
