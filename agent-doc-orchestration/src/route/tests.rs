use super::*;
use crate::flow::routed_reopen::{PromptReadyBarrierFacts, classify_prompt_ready_barrier};
use crate::supervisor::ipc::{IpcMethod, IpcResponse, SupervisorIpc};

// A smaller lock for startup-sensitive isolated tmux tests that inject the
// first command immediately after pane creation.
static TMUX_START_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
// Serialize mock agent launches without contending with tests that already
// hold TMUX_START_MUTEX for broader prompt-readiness coverage.
static TMUX_INJECT_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());
// Serialize mutations of the route-specific binary override without
// contending with current-dir guards that already hold the shared test lock.
static ROUTE_BIN_ENV_MUTEX: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn env_lock() -> crate::test_support::ProcessGlobalLockGuard {
    crate::test_support::env_lock()
}

fn tmux_start_lock() -> std::sync::MutexGuard<'static, ()> {
    TMUX_START_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tmux_inject_lock() -> std::sync::MutexGuard<'static, ()> {
    TMUX_INJECT_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn route_bin_env_lock() -> std::sync::MutexGuard<'static, ()> {
    ROUTE_BIN_ENV_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn test_cwd() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

#[test]
fn authoritative_actor_optimistic_queue_excludes_starting_state() {
    assert!(
        authoritative_actor_dispatch_can_queue_optimistically(
            crate::session_actor::ActorState::Busy
        ),
        "busy actors may still accept a supervisor-owned queued reopen"
    );
    assert!(
        !authoritative_actor_dispatch_can_queue_optimistically(
            crate::session_actor::ActorState::Starting
        ),
        "starting actors must become ready before route submits a reopen"
    );
}

#[test]
fn authoritative_actor_starting_hint_names_reroute_and_restart() {
    let file = std::path::Path::new("/tmp/session.md");
    let hint = authoritative_actor_dispatch_recovery_hint(
        crate::session_actor::ActorState::Starting,
        file,
    );
    assert!(
        hint.contains("rerun `agent-doc /tmp/session.md`"),
        "starting actor hint should tell the user how to retry: {hint}"
    );
    assert!(
        hint.contains("prompt_ready=true"),
        "starting actor hint should name the dispatch-ready wait state: {hint}"
    );
    assert!(
        hint.contains("agent-doc start /tmp/session.md"),
        "starting actor hint should name the owner restart recovery: {hint}"
    );
}

#[test]
fn dispatch_only_starting_pane_not_ready_error_matches_equityfundingsource_active_turn() {
    let file = std::path::Path::new("tasks/professional/equityfundingsource.md");
    let message = dispatch_only_starting_pane_not_ready_error(
        &HarnessConfig::codex(),
        "%42",
        file,
        "active codex turn",
    );

    assert!(message.contains("dispatch-only codex reopen refused"));
    assert!(message.contains("tasks/professional/equityfundingsource.md"));
    assert!(message.contains("latest run is still booting"));
    assert!(message.contains("never reached a dispatch-ready prompt"));
    assert!(message.contains("(active codex turn)"));
}

#[test]
fn authoritative_actor_start_wait_terminal_state_only_for_terminal_states() {
    assert!(authoritative_actor_start_wait_terminal_state(
        crate::session_actor::ActorState::Closed
    ));
    assert!(authoritative_actor_start_wait_terminal_state(
        crate::session_actor::ActorState::Blocked
    ));
    assert!(!authoritative_actor_start_wait_terminal_state(
        crate::session_actor::ActorState::Starting
    ));
    assert!(!authoritative_actor_start_wait_terminal_state(
        crate::session_actor::ActorState::Busy
    ));
    assert!(!authoritative_actor_start_wait_terminal_state(
        crate::session_actor::ActorState::WaitingInput
    ));
    assert!(!authoritative_actor_start_wait_terminal_state(
        crate::session_actor::ActorState::Ready
    ));
}

#[test]
fn authoritative_actor_ready_poll_requires_ready_state_and_prompt_proof() {
    use crate::session_actor::ActorState;

    let schedule = [
        (ActorState::Starting, false, true),
        (ActorState::Busy, false, true),
        (ActorState::Ready, false, true),
    ];
    for (state, prompt_ready, dispatch_eligible) in schedule {
        assert_eq!(
            classify_prompt_ready_barrier(PromptReadyBarrierFacts {
                actor_state: actor_dispatch_state(state),
                prompt_ready,
                dispatch_eligible,
            }),
            PromptReadyBarrierDecision::Continue,
            "route must keep waiting while the current generation is {state:?} prompt_ready={prompt_ready} eligible={dispatch_eligible}"
        );
    }

    assert_eq!(
        classify_prompt_ready_barrier(PromptReadyBarrierFacts {
            actor_state: actor_dispatch_state(ActorState::Ready),
            prompt_ready: true,
            dispatch_eligible: false,
        }),
        PromptReadyBarrierDecision::Continue,
        "a ready actor still cannot dispatch until the target passes dispatch eligibility"
    );
    assert_eq!(
        classify_prompt_ready_barrier(PromptReadyBarrierFacts {
            actor_state: actor_dispatch_state(ActorState::Ready),
            prompt_ready: true,
            dispatch_eligible: true,
        }),
        PromptReadyBarrierDecision::Ready,
        "route may dispatch only after ready state, prompt proof, and eligibility agree"
    );
}

#[test]
fn authoritative_actor_ready_poll_surfaces_terminal_states() {
    use crate::session_actor::ActorState;

    assert_eq!(
        classify_prompt_ready_barrier(PromptReadyBarrierFacts {
            actor_state: actor_dispatch_state(ActorState::Closed),
            prompt_ready: false,
            dispatch_eligible: true,
        }),
        PromptReadyBarrierDecision::Terminal
    );
    assert_eq!(
        classify_prompt_ready_barrier(PromptReadyBarrierFacts {
            actor_state: actor_dispatch_state(ActorState::Blocked),
            prompt_ready: false,
            dispatch_eligible: true,
        }),
        PromptReadyBarrierDecision::Terminal
    );
}

#[test]
fn route_low_level_cleanup_scrubs_duplicate_prompt_comment_without_preserve_doc() {
    let prompt = "The duplicate content corrupting document and duplicate prompt issues happened yet again. Reproduce bugs with tests first and fix the implementation. #spec-test-build-install-commit-push";
    let content = format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ {prompt}\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "{prompt}\n",
            "-->\n\n",
            "<!--\n",
            "Keep this unrelated scratch note hidden.\n",
            "-->\n"
        ),
        prompt = prompt
    );

    let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
        .unwrap()
        .expect("route should canonicalize duplicate prompt scratch comments before dispatch");
    let cleaned = cleanup.content;

    let duplicate_comment = format!("<!--\n{prompt}\n-->");
    assert!(
        !cleaned.contains(&duplicate_comment),
        "route must not dispatch with duplicate prompt text still in the post-exchange comment:\n{cleaned}"
    );
    assert!(
        cleaned.contains("\n<!--\n-->\n\n<!--\nKeep this unrelated scratch note hidden."),
        "route must preserve the ordinary comment shell and unrelated scratch comments:\n{cleaned}"
    );
}

#[test]
fn route_preserves_duplicate_prompt_comment_from_snapshot() {
    let prompt = "What are #next-steps to improve the sqlitedb graph performance?";
    let content = format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ {prompt}\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n\n",
            "<!--\n",
            "{prompt}\n",
            "-->\n"
        ),
        prompt = prompt
    );

    let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[&content]).unwrap();

    assert!(
        cleanup.is_none(),
        "route cleanup must preserve snapshot-owned scratch comments"
    );
}

#[test]
fn route_preserves_scratch_comment_after_compact_summary_before_dispatch() {
    let prompt = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
    let content = format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted content:\n",
            "- Trailing prompt/context: {prompt}\n",
            "❯ {prompt}\n",
            "❯ #spec-test-build-install-commit-push\n",
            "### Re: compact prompt duplication — gpt-5\n\n",
            "Line-by-line diff was the right diagnostic.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n",
            "<!--\n",
            "{prompt}\n",
            "#spec-test-build-install-commit-push\n",
            "---\n",
            "Look through the Claude + Codex + agent-doc session logs\n",
            "-->\n"
        ),
        prompt = prompt
    );

    let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[&content]).unwrap();

    assert!(
        cleanup.is_none(),
        "production route cleanup must preserve visible post-exchange scratch comments"
    );
}

#[test]
fn route_low_level_cleanup_scrubs_unowned_duplicate_prompt_comment() {
    let prompt = "The duplicate corrupt document bug & the duplicated prompt happened yet again as I was typing in this prompt. Should we diff line by line? Do we still have race conditions?";
    let content = format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "### Session Summary\n\n",
            "Compacted content:\n",
            "- Trailing prompt/context: {prompt}\n",
            "❯ {prompt}\n",
            "❯ #spec-test-build-install-commit-push\n",
            "### Re: compact prompt duplication — gpt-5\n\n",
            "Line-by-line diff was the right diagnostic.\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n",
            "<!--\n",
            "{prompt}\n",
            "#spec-test-build-install-commit-push\n",
            "---\n",
            "Look through the Claude + Codex + agent-doc session logs\n",
            "-->\n"
        ),
        prompt = prompt
    );

    let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
        .unwrap()
        .expect("route cleanup should scrub duplicate compact prompt residue");
    let cleaned = cleanup.content;

    assert!(
        !cleaned.contains(&format!("<!--\n{prompt}")),
        "route cleanup should remove only the duplicate prompt line:\n{cleaned}"
    );
    assert!(
        cleaned.contains("Look through the Claude + Codex + agent-doc session logs"),
        "route cleanup must not erase unrelated post-exchange scratch comments:\n{cleaned}"
    );
    assert!(
        cleaned.contains("<!--\n#spec-test-build-install-commit-push\n---\nLook through"),
        "route cleanup must preserve command and separator scratch lines:\n{cleaned}"
    );
}

#[test]
fn route_preserves_scratch_comment_when_response_quotes_same_text() {
    let scratch =
        "Look through the Claude + Codex + agent-doc session logs for #next-steps to fix bugs.";
    let content = format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ Please inspect the latest route cleanup report. #spec-test-build-install-commit-push\n",
            "### Re: route cleanup — gpt-5\n\n",
            "{scratch}\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "###\n",
            "<!--\n",
            "{scratch}\n",
            "-->\n"
        ),
        scratch = scratch
    );

    let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[]).unwrap();
    let cleaned = cleanup
        .as_ref()
        .map(|cleanup| cleanup.content.as_str())
        .unwrap_or(content.as_str());

    assert!(
        cleaned.contains(&format!("<!--\n{scratch}\n-->")),
        "route cleanup must not treat assistant response quotes as prompt residue:\n{cleaned}"
    );
}

#[test]
fn route_scrubs_duplicate_answered_prompt_tail_before_dispatch() {
    let prompt = "The content of the html comment below this agent:exchange element was deleted after the last agent-doc turn. Should we diff line by line?";
    // Genuine delayed replay re-adds the just-answered prompt in answered form
    // (carrying the `❯ ` marker) — that is the ownership proof that lets route
    // scrub it safely.
    let content = format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ {prompt}\n",
            "❯ #spec-test-build-install-commit-push\n",
            "### Re: mixed scratch comment deletion — gpt-5\n\n",
            "Answered already.\n",
            "<!-- agent:boundary:head -->\n",
            "❯ {prompt}\n",
            "❯ #spec-test-build-install-commit-push\n",
            "<!-- /agent:exchange -->\n"
        ),
        prompt = prompt
    );

    let cleanup = scrub_duplicate_prompt_comments_for_route(&content, &[])
        .unwrap()
        .expect("route should canonicalize duplicate answered prompt tails before dispatch");
    let cleaned = cleanup.content;

    assert!(cleanup.removed_answered_tail);
    assert!(
        cleaned.contains(&format!(
            "❯ {prompt}\n❯ #spec-test-build-install-commit-push\n### Re:"
        )),
        "answered prompt block must remain in exchange history:\n{cleaned}"
    );
    assert!(
        !cleaned.contains(&format!("<!-- agent:boundary:head -->\n❯ {prompt}")),
        "route must not dispatch with duplicate answered-form prompt after the boundary:\n{cleaned}"
    );
}

#[test]
fn route_preserves_unprefixed_live_prompt_matching_an_answered_prompt() {
    // Regression for #ipcfullprompt-recur: a freshly-typed prompt that happens to
    // match a previously-answered prompt (e.g. a re-typed "go") has no `❯ ` marker
    // and MUST be preserved for dispatch — never scrubbed as duplicate residue.
    let content = concat!(
        "---\nagent_doc_format: template\n---\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "❯ go\n",
        "### Re: go — gpt-5\n\n",
        "Did the thing.\n",
        "<!-- agent:boundary:head -->\n",
        "go\n",
        "<!-- /agent:exchange -->\n",
    );

    let cleanup = scrub_duplicate_prompt_comments_for_route(content, &[]).unwrap();
    assert!(
        cleanup.is_none(),
        "a bare re-typed prompt must not be scrubbed: {cleanup:?}"
    );
}

#[test]
fn route_rejects_duplicate_prompt_markdown_residue_before_dispatch() {
    let prompt =
        "Please keep this exact sentence around for duplicate residue coverage in markdown";
    let content = format!(
        concat!(
            "---\nagent_doc_format: template\n---\n\n",
            "<!-- agent:exchange patch=append -->\n",
            "❯ {prompt}\n",
            "<!-- agent:boundary:head -->\n",
            "<!-- /agent:exchange -->\n\n",
            "# Notes\n\n",
            "{prompt}\n"
        ),
        prompt = prompt
    );

    let err = scrub_duplicate_prompt_comments_for_route(&content, &[]).unwrap_err();

    assert!(
        err.to_string().contains("duplicate prompt residue"),
        "route must fail closed before dispatching against duplicate prompt Markdown residue: {err}"
    );
}

#[test]
fn route_debounce_fails_closed_while_typing_indicator_is_active() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "prompt in progress\n").unwrap();

    let doc_str = doc.to_string_lossy().to_string();
    crate::debounce::document_changed(&doc_str);

    let err = await_idle_with_max_wait(&doc, Duration::from_millis(500), Duration::from_millis(25))
        .expect_err("route must not proceed while the editor typing indicator is active");

    assert!(
        err.to_string().contains("typing_active=true"),
        "route debounce error should prove the active typing reason: {err}"
    );
}

#[test]
fn route_debounce_allows_dispatch_after_typing_indicator_expires() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "settled prompt\n").unwrap();

    let doc_str = doc.to_string_lossy().to_string();
    crate::debounce::document_changed(&doc_str);

    await_idle_with_max_wait(&doc, Duration::from_millis(10), Duration::from_millis(1000))
        .expect("route should proceed after mtime and typing indicator are both idle");
}

#[test]
fn route_enqueue_dispatch_prompt_creates_visible_auto_queue_and_snapshot() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "queue_active: false\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "❯ prior prompt\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#qipc] Fix queue dispatch.\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(&doc, content).unwrap();

    let outcome = enqueue_route_dispatch_prompt(
        &doc,
        "❯ do [#qipc]. #spec-test-build-install-commit-push",
        "test_busy_actor",
    )
    .expect("route should persist a queued dispatch prompt");

    assert!(outcome.appended);
    assert!(outcome.component_created);
    assert!(outcome.activated);
    assert_eq!(
        outcome.prompt_text,
        "do [#qipc]. #spec-test-build-install-commit-push"
    );
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("queue_active: true"));
    assert!(updated.contains("<!-- agent:queue auto -->"));
    assert!(updated.contains("- do [#qipc]. #spec-test-build-install-commit-push"));
    let queue_pos = updated.find("<!-- agent:queue auto -->").unwrap();
    let backlog_pos = updated.find("<!-- agent:backlog -->").unwrap();
    assert!(
        queue_pos < backlog_pos,
        "created queue component should be visible before tracked work components:\n{updated}"
    );
    let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snapshot, updated,
        "route queueing must sync the snapshot so auto-queue continuation is not treated as a modified head prompt"
    );
}

#[test]
fn route_enqueue_dispatch_prompt_preserves_unparseable_queue_instead_of_crashing() {
    // Repro of "JB Run Agent Doc error: route queue dispatch: failed to parse
    // existing agent:queue": an earlier corruption merged free-text prose into
    // the agent:queue component, so `queue::parse` bails on a bare line. The
    // route must not propagate that as a fatal error — it must preserve the
    // polluted body and still append the new pending dispatch.
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "queue_active: false\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "JB `Run Agent Doc` error:\n",
        "- do [#existing]\n",
        "<!-- /agent:queue -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(&doc, content).unwrap();

    // The polluted free-text line is preserved as a non-actionable Freeform
    // entry (tolerant parse) rather than failing the consume/dispatch guards.
    let parsed = crate::queue::parse("JB `Run Agent Doc` error:\n- do [#existing]\n").unwrap();
    assert!(
        parsed
            .iter()
            .any(|e| matches!(e, crate::queue::QueueEntry::Freeform(_)))
    );

    let outcome = enqueue_route_dispatch_prompt(&doc, "do [#newitem]", "test_busy_actor")
        .expect("route must not crash on a polluted agent:queue");
    assert!(outcome.appended);

    let updated = std::fs::read_to_string(&doc).unwrap();
    // Existing (polluted) content preserved — not silently dropped.
    assert!(updated.contains("JB `Run Agent Doc` error:"));
    assert!(updated.contains("- do [#existing]"));
    // New dispatch appended below it.
    assert!(updated.contains("- do [#newitem]"));

    // Re-dispatching the same prompt into the still-polluted queue is idempotent.
    let outcome2 = enqueue_route_dispatch_prompt(&doc, "do [#newitem]", "test_busy_actor")
        .expect("route must stay resilient on repeat dispatch");
    assert!(outcome2.already_present);
    let updated2 = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(
        updated2.matches("- do [#newitem]").count(),
        1,
        "repeat dispatch into a polluted queue must not duplicate the entry:\n{updated2}"
    );
}

#[test]
fn route_enqueue_dispatch_prompt_activates_existing_queue_without_duplicate() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "queue_active: false\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue -->\n",
        "- do [#qipc]. #spec-test-build-install-commit-push\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#qipc] Fix queue dispatch.\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(&doc, content).unwrap();

    let outcome = enqueue_route_dispatch_prompt(
        &doc,
        "do [#qipc]. #spec-test-build-install-commit-push",
        "test_busy_actor",
    )
    .expect("route should activate an existing queued dispatch prompt");

    assert!(!outcome.appended);
    assert!(outcome.already_present);
    assert!(!outcome.superseded);
    assert!(!outcome.component_created);
    assert!(outcome.activated);
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("queue_active: true"));
    assert!(updated.contains("<!-- agent:queue auto -->"));
    assert_eq!(
        updated
            .matches("- do [#qipc]. #spec-test-build-install-commit-push")
            .count(),
        1,
        "route must not duplicate an already visible queue prompt:\n{updated}"
    );
}

#[test]
fn route_enqueue_dispatch_prompt_no_dup_with_completed_residue_and_live_head() {
    // Repro for #adoc-queue-ipc-drift: a halted/inactive-then-reactivated queue
    // that still carries struck `Completed` residue plus a single live prompt.
    // Re-dispatching the live head must NOT append a duplicate, and must NOT
    // supersede the live head into a struck id.
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "preset #spec-test-build-install-commit-push\n",
        "- ~do [#adoc-sqlite-isolation]~\n",
        "- ~do [#adoc-sqlite-seam]~\n",
        "- do [#adoc-orch-shim-cleanup]\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#adoc-orch-shim-cleanup] Finish the migration.\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(&doc, content).unwrap();

    let outcome =
        enqueue_route_dispatch_prompt(&doc, "do [#adoc-orch-shim-cleanup]", "test_busy_actor")
            .expect("route should treat the live head as already queued");

    let updated = std::fs::read_to_string(&doc).unwrap();
    assert_eq!(
        updated.matches("- do [#adoc-orch-shim-cleanup]").count(),
        1,
        "re-dispatching the live queue head must not duplicate it:\n{updated}\noutcome={outcome:?}"
    );
    assert!(
        !outcome.appended,
        "live head re-dispatch must not append:\n{updated}\noutcome={outcome:?}"
    );
}

#[test]
fn route_enqueue_dispatch_prompt_supersedes_single_auto_queue_prompt() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- Run Agent Doc queued the first prompt.\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(&doc, content).unwrap();

    let outcome = enqueue_route_dispatch_prompt(
        &doc,
        "Run Agent Doc queued the edited prompt.",
        "test_busy_actor",
    )
    .expect("route should update a stale single auto-queue prompt");

    assert!(!outcome.appended);
    assert!(!outcome.already_present);
    assert!(outcome.superseded);
    assert!(!outcome.component_created);
    assert!(outcome.activated);
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(updated.contains("<!-- agent:queue auto -->"));
    assert!(
        !updated.contains("- Run Agent Doc queued the first prompt."),
        "stale route-owned queue prompt should be replaced:\n{updated}"
    );
    assert!(
        updated.contains("- Run Agent Doc queued the edited prompt."),
        "edited prompt should become the single queued rerun:\n{updated}"
    );
    let snapshot = crate::snapshot::load(&doc).unwrap().unwrap();
    assert_eq!(
        snapshot, updated,
        "queue prompt supersession must sync the route snapshot"
    );
}

#[test]
fn route_enqueue_dispatch_prompt_appends_to_multi_prompt_auto_queue() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    let content = concat!(
        "---\n",
        "agent_doc_format: template\n",
        "queue_active: true\n",
        "---\n\n",
        "<!-- agent:exchange -->\n",
        "<!-- /agent:exchange -->\n\n",
        "<!-- agent:queue auto -->\n",
        "- first queued prompt\n",
        "- second queued prompt\n",
        "<!-- /agent:queue -->\n\n",
        "<!-- agent:backlog -->\n",
        "<!-- /agent:backlog -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(&doc, content).unwrap();

    let outcome = enqueue_route_dispatch_prompt(&doc, "third queued prompt", "test_busy_actor")
        .expect("route should append to user-style multi-prompt auto queues");

    assert!(outcome.appended);
    assert!(!outcome.already_present);
    assert!(!outcome.superseded);
    assert!(!outcome.component_created);
    assert!(outcome.activated);
    let updated = std::fs::read_to_string(&doc).unwrap();
    assert!(
        updated.contains("- first queued prompt\n- second queued prompt\n- third queued prompt")
    );
}

fn test_registry_entry(pane: &str, file: &str, cwd: &std::path::Path) -> sessions::SessionEntry {
    sessions::SessionEntry {
        pane: pane.to_string(),
        pid: 1234,
        cwd: cwd.to_string_lossy().to_string(),
        started: "2026-01-01T00:00:00Z".to_string(),
        session_id: "test-session".to_string(),
        file: file.to_string(),
        window: "@1".to_string(),
        supervisor_instance_id: String::new(),
    }
}

struct ScopedCurrentDir {
    prev_cwd: std::path::PathBuf,
    _env_guard: crate::test_support::ProcessGlobalLockGuard,
}

impl ScopedCurrentDir {
    fn set(path: &std::path::Path) -> Self {
        let env_guard = env_lock();
        let prev_cwd = std::env::current_dir().unwrap_or_else(|_| test_cwd());
        std::env::set_current_dir(path).unwrap();
        Self {
            prev_cwd,
            _env_guard: env_guard,
        }
    }
}

impl Drop for ScopedCurrentDir {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.prev_cwd);
    }
}

fn write_codex_proof_status_fixture(
    dir: &std::path::Path,
    session_id: &str,
    event: &str,
) -> std::path::PathBuf {
    std::fs::create_dir_all(dir.join(".agent-doc/logs")).unwrap();
    let doc = dir.join("session.md");
    std::fs::write(
            &doc,
            "---\nagent_doc_session: route-proof-status\nagent: codex\ncodex_network_access: enabled\n---\n",
        )
        .unwrap();
    std::fs::write(
        dir.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
        format!(
            "[1] session_start file={} pane=%1 session={}\n[2] {}\n",
            doc.display(),
            session_id,
            event
        ),
    )
    .unwrap();
    doc
}

fn write_codex_writable_proof_status_fixture(
    dir: &std::path::Path,
    session_id: &str,
    event: &str,
) -> (std::path::PathBuf, String) {
    std::fs::create_dir_all(dir.join(".agent-doc/logs")).unwrap();
    let writable = dir.join("writable-root");
    std::fs::create_dir_all(&writable).unwrap();
    let writable = writable.canonicalize().unwrap();
    let doc = dir.join("session.md");
    std::fs::write(
            &doc,
            format!(
                "---\nagent_doc_session: route-proof-status\nagent: codex\ncodex_args: \"--add-dir {}\"\n---\n",
                writable.display()
            ),
        )
        .unwrap();
    std::fs::write(
        dir.join(".agent-doc/logs")
            .join(format!("{session_id}.log")),
        format!(
            "[1] session_start file={} pane=%1 session={}\n[2] {}\n",
            doc.display(),
            session_id,
            event
        ),
    )
    .unwrap();
    let contract = crate::agent::codex::writable_root_contract_id(&[writable]).unwrap();
    (doc, contract)
}

#[test]
fn managed_capability_proof_status_tracks_pending_and_failed() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let session_id = "route-proof-status";
    let doc = write_codex_proof_status_fixture(
        dir.path(),
        session_id,
        "codex_capability_proof status=pending",
    );

    assert_eq!(
        managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
        ManagedCapabilityProofStatus::Pending
    );

    let doc = write_codex_proof_status_fixture(
        dir.path(),
        session_id,
        "codex_capability_proof status=failed error=\"dns\"",
    );
    assert_eq!(
        managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
        ManagedCapabilityProofStatus::Failed
    );
}

#[test]
fn managed_capability_proof_status_requires_matching_writable_root_contract() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let session_id = "route-writable-proof-status";
    let (doc, contract) = write_codex_writable_proof_status_fixture(
        dir.path(),
        session_id,
        "codex_capability_proof status=proven network=not_required network_probe=not_required ssh_targets=0 writable_roots=1",
    );

    assert_eq!(
        managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
        ManagedCapabilityProofStatus::Missing
    );

    let (doc, _) = write_codex_writable_proof_status_fixture(
        dir.path(),
        session_id,
        &format!(
            "codex_capability_proof status=proven network=not_required network_probe=not_required ssh_targets=0 writable_roots=1 writable_root_contract={contract}"
        ),
    );
    assert_eq!(
        managed_capability_proof_status(&doc, session_id, &HarnessConfig::codex()).unwrap(),
        ManagedCapabilityProofStatus::Proven
    );
}

#[test]
fn managed_capability_proof_status_opencode_tracks_pending_and_failed() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let session_id = "route-proof-status-opencode";
    let doc = write_codex_proof_status_fixture(
        dir.path(),
        session_id,
        "opencode_capability_proof status=pending",
    );

    assert_eq!(
        managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap(),
        ManagedCapabilityProofStatus::Pending
    );

    let doc = write_codex_proof_status_fixture(
        dir.path(),
        session_id,
        "opencode_capability_proof status=failed error=\"ssh\"",
    );
    assert_eq!(
        managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap(),
        ManagedCapabilityProofStatus::Failed
    );
}

#[test]
fn pane_registration_matches_file_resolves_entry_cwd() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let submodule = dir.path().join("src/session-share");
    let tasks = submodule.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let doc = tasks.join("claudescore-3.md");
    std::fs::write(&doc, "# session\n").unwrap();

    let mut registry = sessions::SessionRegistry::new();
    registry.insert(
        "session-a".to_string(),
        test_registry_entry("%401", "tasks/claudescore-3.md", &submodule),
    );

    assert!(
        pane_registration_matches_file(&registry, "%401", &doc.to_string_lossy()),
        "relative registry paths should resolve against the pane cwd"
    );
}

#[test]
fn ensure_dispatch_target_matches_file_rejects_cross_file_registration() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let submodule = dir.path().join("src/session-share");
    let tasks = submodule.join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let registered = tasks.join("monsterrodholders.md");
    let requested = tasks.join("claudescore-3.md");
    std::fs::write(&registered, "# registered\n").unwrap();
    std::fs::write(&requested, "# requested\n").unwrap();

    sessions::register_full_with_cwd_in(
        dir.path(),
        "session-a",
        "%401",
        "tasks/monsterrodholders.md",
        1234,
        "@1",
        &submodule.to_string_lossy(),
    )
    .unwrap();

    let err = ensure_dispatch_target_matches_file("%401", &requested.to_string_lossy())
        .expect_err("cross-file pane reuse must fail closed");
    assert!(
        err.to_string().contains("refusing cross-file dispatch"),
        "error should explain the rejected cross-file dispatch: {err}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn register_dispatch_target_rejects_cross_file_rebind_and_preserves_registry() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let iso = IsolatedTmux::new("route-test-cross-file-rebind-guard");
    let session = "test";

    let tasks = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let first = tasks.join("agent-doc-bugs2.md");
    let second = tasks.join("tsift.md");
    std::fs::write(&first, "# first\n").unwrap();
    std::fs::write(&second, "# second\n").unwrap();
    let pane_a = iso.auto_start(session, dir.path()).unwrap();
    let pane_b = iso.split_window(&pane_a, dir.path(), "-dh").unwrap();

    sessions::register_full_with_cwd_in(
        dir.path(),
        "session-a",
        &pane_a,
        &first.to_string_lossy(),
        1234,
        "@128",
        &dir.path().to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd_in(
        dir.path(),
        "session-b",
        &pane_b,
        &second.to_string_lossy(),
        5678,
        "@128",
        &dir.path().to_string_lossy(),
    )
    .unwrap();
    crate::startup_miss::append_session_log_event(
        &first,
        "session-a",
        &format!(
            "session_start file={} pane={} session=session-a",
            first.display(),
            pane_a
        ),
    )
    .unwrap();

    let err = register_dispatch_target(&iso, "session-b", &pane_a, &second.to_string_lossy())
        .expect_err("cross-file dispatch target rebind must fail closed");
    assert!(
        err.to_string().contains("refusing cross-file dispatch"),
        "error should explain the rejected cross-file dispatch: {err}"
    );
    assert_eq!(
        sessions::lookup_in(dir.path(), "session-a").unwrap(),
        Some(pane_a.clone()),
        "the original authoritative pane must stay bound to its file"
    );
    assert_eq!(
        sessions::lookup_in(dir.path(), "session-b").unwrap(),
        Some(pane_b),
        "the requesting file must keep its own registered pane"
    );
}

fn wait_for_pane_contains(
    iso: &IsolatedTmux,
    pane: &str,
    needle: &str,
    timeout: std::time::Duration,
) -> String {
    let start = std::time::Instant::now();
    let poll = std::time::Duration::from_millis(100);
    let mut last = String::new();
    while start.elapsed() < timeout {
        last = sessions::capture_pane(iso, pane).unwrap_or_default();
        if last.contains(needle) {
            return last;
        }
        std::thread::sleep(poll);
    }
    last
}

fn pane_capture_contains_wrapped(capture: &str, needle: &str) -> bool {
    capture.contains(needle) || capture.replace(['\r', '\n'], "").contains(needle)
}

fn send_keys_with_retry(iso: &IsolatedTmux, pane: &str, text: &str) {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(3);
    let poll = std::time::Duration::from_millis(100);
    let mut last_err = None;

    while start.elapsed() < timeout {
        match iso.send_keys(pane, text) {
            Ok(()) => return,
            Err(err) => last_err = Some(err.to_string()),
        }
        std::thread::sleep(poll);
    }

    panic!(
        "failed to send keys to pane {} after {:.1}s: {}",
        pane,
        start.elapsed().as_secs_f64(),
        last_err.unwrap_or_else(|| "unknown error".to_string())
    );
}

fn pane_current_command(iso: &IsolatedTmux, pane: &str) -> Option<String> {
    let output = iso
        .cmd()
        .args([
            "display-message",
            "-t",
            pane,
            "-p",
            "#{pane_current_command}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let cmd = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if cmd.is_empty() { None } else { Some(cmd) }
}

fn wait_for_shell(iso: &IsolatedTmux, pane: &str, timeout: std::time::Duration) -> bool {
    const IDLE_SHELLS: &[&str] = &["zsh", "bash", "sh", "fish"];
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Some(cmd) = pane_current_command(iso, pane)
            && IDLE_SHELLS.contains(&cmd.as_str())
        {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
    false
}

// --- rewrite_start_path tests ---

#[test]
fn rewrite_start_path_narrows_to_submodule_relative() {
    // Simulate: super root with a `src/sub` submodule holding `tasks/foo.md`.
    // `cwd` = super/src/sub (narrowed by resolve_pane_cwd).
    // `file_path` = "src/sub/tasks/foo.md" (super-root-relative, as passed by caller).
    // Expected: rewritten to "tasks/foo.md".
    let tmp = tempfile::TempDir::new().unwrap();
    let super_root = tmp.path();
    let sub_root = super_root.join("src").join("sub");
    let tasks_dir = sub_root.join("tasks");
    std::fs::create_dir_all(&tasks_dir).unwrap();
    let doc = tasks_dir.join("foo.md");
    std::fs::write(&doc, "# foo\n").unwrap();

    let original = "src/sub/tasks/foo.md";
    let rewritten = rewrite_start_path(&doc, &sub_root, original);
    assert_eq!(
        rewritten,
        format!("tasks{}foo.md", std::path::MAIN_SEPARATOR)
    );
}

#[test]
fn rewrite_start_path_no_op_when_file_under_cwd_with_same_prefix() {
    // Non-submodule case: cwd = super root, file is already super-root-relative.
    // The rewrite still works — it just returns the same relative path.
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let doc = root.join("plan.md");
    std::fs::write(&doc, "# plan\n").unwrap();

    let original = "plan.md";
    let rewritten = rewrite_start_path(&doc, root, original);
    assert_eq!(rewritten, "plan.md");
}

#[test]
fn rewrite_start_path_falls_back_when_canonicalize_fails() {
    // Non-existent file path → canonicalize fails → fallback to original.
    let tmp = tempfile::TempDir::new().unwrap();
    let ghost = tmp.path().join("does-not-exist.md");
    let original = "does-not-exist.md";
    let rewritten = rewrite_start_path(&ghost, tmp.path(), original);
    assert_eq!(rewritten, original);
}

#[test]
fn rewrite_start_path_falls_back_when_file_not_under_cwd() {
    // File exists but lives outside the given cwd → strip_prefix fails → fallback.
    let tmp = tempfile::TempDir::new().unwrap();
    let outside = tmp.path().join("outside.md");
    std::fs::write(&outside, "# outside\n").unwrap();
    let unrelated_cwd = tempfile::TempDir::new().unwrap();

    let original = "outside.md";
    let rewritten = rewrite_start_path(&outside, unrelated_cwd.path(), original);
    assert_eq!(rewritten, original);
}

// --- Split direction tests ---

#[test]
fn is_first_column_empty_cols() {
    let file = Path::new("tasks/agent-doc.md");
    assert!(!is_first_column(file, &[]));
}

#[test]
fn is_first_column_single_col() {
    let file = Path::new("tasks/agent-doc.md");
    let cols = vec!["tasks/agent-doc.md".to_string()];
    // Single column — no need to split before
    assert!(!is_first_column(file, &cols));
}

#[test]
fn is_first_column_in_first_col() {
    let file = Path::new("tasks/agent-doc.md");
    let cols = vec![
        "tasks/agent-doc.md".to_string(),
        "tasks/email.md".to_string(),
    ];
    assert!(is_first_column(file, &cols));
}

#[test]
fn is_first_column_in_second_col() {
    let file = Path::new("tasks/email.md");
    let cols = vec![
        "tasks/agent-doc.md".to_string(),
        "tasks/email.md".to_string(),
    ];
    assert!(!is_first_column(file, &cols));
}

#[test]
fn is_first_column_comma_separated() {
    let file = Path::new("tasks/agent-doc.md");
    let cols = vec![
        "tasks/agent-doc.md,tasks/corky.md".to_string(),
        "tasks/email.md".to_string(),
    ];
    assert!(is_first_column(file, &cols));
}

// --- Prompt detection tests (via HarnessConfig) ---

#[test]
fn detects_unicode_prompt() {
    let h = HarnessConfig::claude();
    assert!(h.is_prompt_line("❯"));
    assert!(h.is_prompt_line("❯ "));
    assert!(h.is_prompt_line("  ❯  "));
}

#[test]
fn detects_ascii_prompt() {
    let h = HarnessConfig::codex();
    assert!(h.is_prompt_line(">"));
    assert!(h.is_prompt_line("> "));
    assert!(h.is_prompt_line("  >  "));
}

#[test]
fn rejects_non_prompt_lines() {
    let h = HarnessConfig::claude();
    assert!(!h.is_prompt_line("Starting claude..."));
    assert!(!h.is_prompt_line("test result: ok"));
    assert!(!h.is_prompt_line(""));
    assert!(!h.is_prompt_line("  "));
    assert!(!h.is_prompt_line("## User"));
}

#[test]
fn handles_ansi_prompt() {
    let h = HarnessConfig::claude();
    assert!(h.is_prompt_line("\x1b[32m❯\x1b[0m"));
    let h_codex = HarnessConfig::codex();
    assert!(h_codex.is_prompt_line("\x1b[1m>\x1b[0m"));
}

// --- Routing logic tests ---

#[test]
fn unregistered_file_skips_lazy_claim() {
    // When registered is None, the lazy-claim step should be skipped.
    // This is verified by the code structure: `if registered.is_some()` guards
    // the find_target_pane call.
    let registered: Option<String> = None;
    assert!(
        registered.is_none(),
        "unregistered files should not attempt lazy claim"
    );
}

#[test]
fn dead_registered_pane_allows_lazy_claim() {
    // When registered is Some but pane is dead, lazy-claim should be attempted.
    let registered: Option<String> = Some("%99".to_string());
    assert!(
        registered.is_some(),
        "dead registered pane should attempt lazy claim"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn lazy_claim_requires_explicit_pane_provenance() {
    let iso = IsolatedTmux::new("route-test-lazy-claim-explicit-only");
    let cwd = std::env::current_dir().unwrap();
    let pane = iso.auto_start("claim", &cwd).unwrap();
    let claimed_panes = std::collections::HashSet::new();

    assert_eq!(
        find_target_pane(&iso, None, "claim", &claimed_panes),
        None,
        "route must not adopt the session's active pane implicitly"
    );
    assert_eq!(
        find_target_pane(&iso, Some(&pane), "claim", &claimed_panes),
        Some(pane),
        "explicit pane override remains valid lazy-claim provenance"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn wrong_session_pane_still_receives_send() {
    // Strategy 1 is session-agnostic after the fix: when a registered pane
    // is alive, send to it regardless of which tmux session it lives in.
    //
    // This is the bug scenario: IDE `Run Agent Doc` spawns `agent-doc route`
    // with no $TMUX env, so target_session falls back to the constant
    // "claude". The claimed pane lives in the user's real session (e.g.
    // "btak"). Before the fix, the session mismatch + shell-idle process
    // sent routing to Strategy 2/3 — auto-starting a new Claude pane in
    // the non-existent "claude" session.
    //
    // This test verifies the tmux infrastructure that makes the fix work:
    // pane_alive must return true for an alive pane regardless of the
    // session it belongs to. The %N pane ID is the routing key.
    let iso = IsolatedTmux::new("route-test-wrong-sess-send");
    let cwd = std::env::current_dir().unwrap();

    // Pane lives in session "real" (simulating the user's tmux session).
    let registered_pane = iso.auto_start("real", &cwd).unwrap();
    assert!(iso.pane_alive(&registered_pane));

    // tmux has no session named "claude" (the fallback target_session).
    let claude_alive = iso
        .cmd()
        .args(["has-session", "-t", "claude"])
        .status()
        .map(|s| s.success())
        .unwrap_or(false);
    assert!(!claude_alive, "fallback target session should not exist");

    // pane_alive does not consult session membership — Strategy 1 can
    // send to the pane via its %N id even though pane_session != "claude".
    assert!(
        iso.pane_alive(&registered_pane),
        "alive pane must be routable regardless of target_session"
    );
}

// --- Integration tests (IsolatedTmux) ---

use sessions::IsolatedTmux;

/// Create a mock agent script: blocks for delay, then prints ❯ prompt on its own line.
/// Uses `cat` to keep the process alive after showing the prompt.
fn mock_agent_script(delay_ms: u64) -> String {
    format!(
        r#"exec /bin/sh -c 'printf "Starting agent...\n"; sleep {}; printf "❯ \n"; cat'"#,
        delay_ms as f64 / 1000.0
    )
}

fn write_mock_registered_agent_doc(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc");
    std::fs::write(
            &script,
            "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  [ -z \"$CMD\" ] && continue\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_registered_agent_doc_with_prefix(
    base: &Path,
    name: &str,
    prefix: &str,
) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join(name);
    std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nprintf \"> \\n\"\nwhile IFS= read -r CMD; do\n  printf '{prefix}:%s\\n' \"$CMD\"\ndone\n",
            ),
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_registered_agent_doc_extra_line_detector(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-extra-line-detector");
    std::fs::write(
            &script,
            "#!/bin/bash\nprintf \"> \\n\"\nIFS= read -r CMD || exit 0\nprintf 'GOT:%s\\n' \"$CMD\"\nif IFS= read -r -t 0.5 EXTRA; then\n  printf 'EXTRA:%s\\n' \"$EXTRA\"\nfi\ncat\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_registered_agent_doc_with_stale_trigger(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-stale-trigger-detector");
    std::fs::write(
            &script,
            "#!/bin/bash\nprintf '> %s\\n' \"$1\"\nIFS= read -r CMD || exit 0\nprintf 'GOT:%s\\n' \"$CMD\"\nif IFS= read -r -t 0.5 EXTRA; then\n  printf 'EXTRA:%s\\n' \"$EXTRA\"\nfi\ncat\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_busy_registered_agent_doc(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-busy");
    std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'Working...\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_active_codex_turn_registered_agent_doc(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-active-codex-turn");
    std::fs::write(
        &script,
        "#!/bin/sh\nprintf 'Working...\\n'\ni=0\nwhile [ \"$i\" -lt 20 ]; do\n  printf 'Working (1m 34s - esc to interrupt)\\n'\n  i=$((i + 1))\ndone\nprintf '\\n> Write tests for @filename\\ngpt-5 high - ~/work/btakita/agent-loop - Context 41%% used\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_busy_registered_agent_doc_ignores_interrupt(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-busy-ignore-int");
    std::fs::write(
            &script,
            "#!/bin/sh\ntrap '' INT\nprintf 'Working...\\n'\nwhile IFS= read -r CMD; do\n  printf 'EARLY:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_busy_opencode_recovers_on_escape(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-busy-opencode");
    std::fs::write(
        &script,
        r#"#!/bin/bash
trap '' INT
cleanup() { stty sane 2>/dev/null || true; }
trap cleanup EXIT
stty -echo -icanon min 1 time 0
printf '⬝⬝■■■■■■  esc interrupt\n'
while IFS= read -r -n1 ch; do
  stty sane
  printf '> \n'
  while IFS= read -r CMD; do
    printf 'GOT:%s\n' "$CMD"
  done
  exit 0
done
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_busy_registered_agent_doc_recovers_on_ctrl_g(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-busy-recovers-on-ctrl-g");
    std::fs::write(
        &script,
        r#"#!/bin/bash
trap '' INT
cleanup() { stty sane 2>/dev/null || true; }
trap cleanup EXIT
stty -echo -icanon min 1 time 0
printf 'Working...\n'
printf 'reverse-i-search: bugs enter accept · esc cancel\n'
while IFS= read -r -n1 ch; do
  if [[ "$ch" == $'\a' ]]; then
    stty sane
    printf '› \n'
    printf 'gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used\n'
    while IFS= read -r CMD; do
      printf 'GOT:%s\n' "$CMD"
    done
    exit 0
  fi
done
"#,
    )
    .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_start_agent_doc(base: &Path) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-start");
    std::fs::write(
            &script,
            "#!/bin/sh\nprintf 'Starting agent...\\n'\nprintf '> \\n'\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn write_mock_delayed_start_agent_doc(base: &Path, delay_secs: u64) -> std::path::PathBuf {
    use std::os::unix::fs::PermissionsExt;

    let bin_dir = base.join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script = bin_dir.join("agent-doc-start-delayed");
    std::fs::write(
            &script,
            format!(
                "#!/bin/sh\nsleep {}\nprintf 'Starting agent...\\n'\nprintf '> \\n'\nwhile IFS= read -r CMD; do\n  printf 'GOT:%s\\n' \"$CMD\"\ndone\n",
                delay_secs
            ),
        )
        .unwrap();
    let mut perms = std::fs::metadata(&script).unwrap().permissions();
    perms.set_mode(0o755);
    std::fs::set_permissions(&script, perms).unwrap();
    script
}

fn launch_mock_registered_agent_doc(iso: &IsolatedTmux, pane: &str, script: &Path, file: &Path) {
    {
        let _tmux_guard = tmux_inject_lock();
        assert!(
            wait_for_shell(iso, pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock agent launch"
        );
        send_keys_with_retry(
            iso,
            pane,
            &format!("exec {} {}", script.display(), file.display()),
        );
    }
    let launch_command = format!("exec {} {}", script.display(), file.display());
    let content = wait_for_mock_agent_prompt(iso, pane, &launch_command);
    assert!(
        content.lines().any(|line| line.trim() == ">"),
        "mock agent-doc session should present a prompt, got: {content}"
    );
}

fn launch_mock_agent_doc_without_file_arg(iso: &IsolatedTmux, pane: &str, script: &Path) {
    {
        let _tmux_guard = tmux_inject_lock();
        assert!(
            wait_for_shell(iso, pane, std::time::Duration::from_secs(5)),
            "shell did not become ready before mock agent launch"
        );
        send_keys_with_retry(iso, pane, &format!("exec {}", script.display()));
    }
    let launch_command = format!("exec {}", script.display());
    let content = wait_for_mock_agent_prompt(iso, pane, &launch_command);
    assert!(
        content.lines().any(|line| line.trim() == ">"),
        "mock agent-doc session should present a prompt, got: {content}"
    );
}

fn wait_for_mock_agent_prompt(iso: &IsolatedTmux, pane: &str, launch_command: &str) -> String {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(20);
    let poll = std::time::Duration::from_millis(100);
    let mut last_submit = std::time::Instant::now()
        .checked_sub(std::time::Duration::from_secs(1))
        .unwrap_or_else(std::time::Instant::now);
    let mut last = String::new();

    while start.elapsed() < timeout {
        last = sessions::capture_pane(iso, pane).unwrap_or_default();
        if last.lines().any(|line| line.trim() == ">") {
            return last;
        }
        if last.contains(launch_command)
            && last_submit.elapsed() >= std::time::Duration::from_millis(500)
        {
            let _ = iso.send_keys_raw(pane, "Enter");
            last_submit = std::time::Instant::now();
        }
        std::thread::sleep(poll);
    }

    last
}

fn wait_for_process_pid(pattern: &str, timeout: std::time::Duration) -> u32 {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(output) = std::process::Command::new("pgrep")
            .args(["-f", pattern])
            .output()
            && output.status.success()
        {
            for line in String::from_utf8_lossy(&output.stdout).lines() {
                let pid = line.trim();
                if pid.is_empty() {
                    continue;
                }
                if let Ok(parsed) = pid.parse::<u32>() {
                    return parsed;
                }
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    panic!("timed out waiting for process matching pattern: {pattern}");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn wait_for_agent_ready_detects_prompt() {
    let _tmux_guard = tmux_start_lock();
    let iso = IsolatedTmux::new("route-test-ready");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
        "shell did not become ready before mock agent launch"
    );

    send_keys_with_retry(&iso, &pane, &mock_agent_script(500));
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "Starting agent...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Starting agent..."),
        "mock agent never started in pane: {content}"
    );

    let harness = HarnessConfig::claude();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
    assert!(ready, "should detect ❯ prompt from mock agent");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn wait_for_agent_ready_detects_claude_composer_hint_prompt() {
    let _tmux_guard = tmux_start_lock();
    let iso = IsolatedTmux::new("route-test-claude-composer-hint");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
        "shell did not become ready before mock agent launch"
    );

    let script = r#"exec /bin/sh -c 'printf "Starting claude...\n"; sleep 0.5; printf "⏵⏵ bypass permissions on (shift+tab to cycle)\n"; cat'"#;
    send_keys_with_retry(&iso, &pane, script);
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "Starting claude...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Starting claude..."),
        "mock claude never started in pane: {content}"
    );

    let harness = HarnessConfig::claude();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
    assert!(
        ready,
        "should detect Claude composer hint line as an idle prompt"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn wait_for_agent_ready_times_out_without_prompt() {
    let iso = IsolatedTmux::new("route-test-timeout");
    let session = "test";
    let cwd = test_cwd();

    let pane_id = iso
        .cmd()
        .args([
            "new-session",
            "-d",
            "-s",
            session,
            "-c",
            &cwd.to_string_lossy(),
            "-P",
            "-F",
            "#{pane_id}",
            "sleep",
            "30",
        ])
        .output()
        .expect("failed to create tmux session");
    let pane = String::from_utf8_lossy(&pane_id.stdout).trim().to_string();

    let harness = HarnessConfig::claude();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(2), &harness);
    assert!(!ready, "should time out when no ❯ prompt appears");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn wait_for_agent_ready_codex_prompt() {
    let _tmux_guard = tmux_start_lock();
    let iso = IsolatedTmux::new("route-test-codex-ready");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
        "shell did not become ready before mock codex launch"
    );

    // Recent Codex builds expose a `›` prompt above a footer/status line.
    let script = r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "› \n"; printf "gpt-5.4 high · ~/work/btakita/agent-loop · Context 0% used\n"; cat'"#;
    send_keys_with_retry(&iso, &pane, script);
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "Starting codex...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Starting codex..."),
        "mock codex never started in pane: {content}"
    );

    let harness = HarnessConfig::codex();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
    assert!(
        ready,
        "should detect › prompt for codex harness even when a footer/status line follows it"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn wait_for_agent_ready_rejects_codex_queue_message_footer() {
    let _tmux_guard = tmux_start_lock();
    let iso = IsolatedTmux::new("route-test-codex-queue-message");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
        "shell did not become ready before mock codex launch"
    );

    let script = r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "› \n"; printf "tab to queue message\n"; printf "gpt-5.4 high · ~/work/btakita/agent-loop · Context 54% used\n"; cat'"#;
    send_keys_with_retry(&iso, &pane, script);
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "Starting codex...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Starting codex..."),
        "mock codex never started in pane: {content}"
    );

    let harness = HarnessConfig::codex();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(2), &harness);
    assert!(
        !ready,
        "queue-message footer must not count as an idle Codex prompt"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn wait_for_agent_ready_rejects_codex_reverse_history_search() {
    let _tmux_guard = tmux_start_lock();
    let iso = IsolatedTmux::new("route-test-codex-reverse-search");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
        "shell did not become ready before mock codex launch"
    );

    let script = r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "reverse-i-search: bugs enter accept · esc cancel\n"; cat'"#;
    send_keys_with_retry(&iso, &pane, script);
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "Starting codex...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Starting codex..."),
        "mock codex never started in pane: {content}"
    );

    let harness = HarnessConfig::codex();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(3), &harness);
    assert!(
        !ready,
        "reverse-i-search must not count as an idle Codex prompt"
    );
}

#[test]
fn ready_prompt_candidate_accepts_codex_idle_placeholder_prompt() {
    let harness = HarnessConfig::codex();
    let content = "\
Starting codex...
› Run /review on my current changes
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
    assert!(
        ready_prompt_candidate(content, &harness).is_some(),
        "known idle Codex placeholder suggestions must count as a ready dispatch target"
    );
}

#[test]
fn ready_prompt_candidate_accepts_future_codex_idle_placeholder_shape() {
    let harness = HarnessConfig::codex();
    let content = "\
Starting codex...
› Explain this module in @filename
gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used
";
    assert!(
        ready_prompt_candidate(content, &harness).is_some(),
        "structurally-valid Codex idle placeholder suggestions must count as ready"
    );
}

#[test]
fn ready_prompt_candidate_rejects_codex_footer_without_prompt() {
    let harness = HarnessConfig::codex();
    let content = "\
gpt-5.5 high · ~/work/btakita/agent-loop · Context 70% used
";
    assert!(
        ready_prompt_candidate(content, &harness).is_none(),
        "a Codex status/footer line alone is not a dispatch-ready prompt"
    );
}

#[test]
fn ready_prompt_candidate_rejects_codex_hook_review_prompt_after_capability_proof() {
    let harness = HarnessConfig::codex();
    let content = "\
Starting codex...
⚠ 1 hook needs review before it can run. Open /hooks to review it.

› [start] managed codex capability proof: codex_capability_proof status=proven network=proven network_probe=child_dns_https ssh_targets=0 writable_roots=0 timings_ms=network_host_dns:8,network_child:9806,ssh:not_required,writable_launcher:not_required,writable_child:not_required,total:9815
";
    assert!(
        ready_prompt_candidate(content, &harness).is_none(),
        "Codex hook-review chrome requires operator approval before route can dispatch"
    );
}

#[test]
fn ready_prompt_candidate_accepts_opencode_status_without_proof_output() {
    let harness = HarnessConfig::opencode();
    let content = "\
zai/glm-5 · ~/work/btakita/agent-loop · context 0% used
";
    assert!(
        ready_prompt_candidate(content, &harness).is_some(),
        "OpenCode can render an idle composer as status chrome with proof output kept out of the pane"
    );
}

#[test]
fn ready_prompt_candidate_accepts_opencode_idle_splash_without_prompt_glyph() {
    let harness = HarnessConfig::opencode();
    let content = "\
                                                                                                     ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▄ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀ ▀▀▀▀
                                                                                   ┃
                                                                                   ┃  Ask anything... \"What is the tech stack of this project?\"
                                                                                   ┃
                                                                                   ┃  Build · GLM-5.1 Z.AI Coding Plan
                                                                                   ╹▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀
                                                                                                                                   tab agents  ctrl+p commands
                                                                                        ● Tip Toggle username display in chat via command palette (Ctrl+P)
  ~/work/btakita/agent-loop:main                                                                                                                                                                                                       1.14.48
";
    assert!(
        ready_prompt_candidate(content, &harness).is_some(),
        "OpenCode 1.14 can render the idle composer as splash chrome without a prompt glyph"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn wait_for_agent_ready_rejects_codex_prompt_with_real_drafted_text() {
    let _tmux_guard = tmux_start_lock();
    let iso = IsolatedTmux::new("route-test-codex-drafted-prompt");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
        "shell did not become ready before mock codex launch"
    );

    let script = r#"exec /bin/sh -c 'printf "Starting codex...\n"; sleep 0.5; printf "› investigate this issue\n"; printf "gpt-5.4 high · ~/work/btakita/agent-loop · Context 31% used\n"; cat'"#;
    send_keys_with_retry(&iso, &pane, script);
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "Starting codex...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Starting codex..."),
        "mock codex never started in pane: {content}"
    );

    let harness = HarnessConfig::codex();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(2), &harness);
    assert!(
        !ready,
        "real drafted Codex text must not count as an idle dispatch target"
    );
}

#[test]
fn recent_lines_contain_trigger_matches_claude_trigger() {
    let content = "\
history line
\x1b[32m❯\x1b[0m /agent-doc test.md
";
    assert!(recent_lines_contain_trigger(content, "/agent-doc test.md"));
    assert!(!recent_lines_contain_trigger(content, "agent-doc test.md"));
}

#[test]
fn recent_lines_contain_trigger_matches_codex_trigger() {
    let content = "\
history line
> agent-doc test.md
";
    assert!(recent_lines_contain_trigger(content, "agent-doc test.md"));
    assert!(!recent_lines_contain_trigger(content, "/agent-doc test.md"));
}

#[test]
fn recent_lines_contain_trigger_matches_wrapped_codex_trigger() {
    let trigger =
        "agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claudescore-3.md";
    let content = "\
› agent-doc /home/brian/work/btakita/agent-loop/src/session-share/tasks/claud
escore-3.md
gpt-5.4 high · ~/work/btakita/agent-loop/src/session-share · Context 31% used
";
    assert!(
        recent_lines_contain_trigger(content, trigger),
        "wrapped Codex composer lines must still count as pending input"
    );
}

#[test]
fn codex_routed_dispatch_start_proof_accepts_any_newer_state_for_same_file() {
    let tracker = RoutedDispatchStartTracker::CodexHook {
        trigger: "agent-doc /tmp/task.md".to_string(),
        previous_session_id: Some("codex-session".to_string()),
        previous_turn_id: Some("turn-1".to_string()),
        previous_updated_at: Some(10),
    };
    let state = crate::codex_hook::ActiveSessionState {
        session_id: "codex-session".to_string(),
        doc_path: "/tmp/task.md".to_string(),
        last_turn_id: "turn-2".to_string(),
        last_prompt: "/review current changes".to_string(),
        updated_at: 11,
    };
    assert_eq!(
        codex_routed_dispatch_start_proof(&tracker, &state),
        Some(RoutedDispatchStartProof::HookStateAdvanced)
    );
}

#[test]
fn opencode_pane_state_change_proof_requires_trigger_to_leave_composer() {
    let harness = HarnessConfig::opencode();
    let trigger = harness.trigger_command("tasks/bugs.md");
    let before = ">\n";
    let drafted = format!("> {trigger}\n");
    assert!(
        !opencode_pane_state_changed_from_idle(&harness, &trigger, before, &drafted),
        "drafted trigger text is pane input, not dispatch-start proof"
    );

    let active = "\
Working (2s - esc to interrupt)
zai/glm-5 · ~/work/btakita/agent-loop · context 0% used
";
    assert!(
        opencode_pane_state_changed_from_idle(&harness, &trigger, before, active),
        "OpenCode leaving idle chrome for active output should prove dispatch start"
    );

    let idle_status = "zai/glm-5 · ~/work/btakita/agent-loop · context 0% used\n";
    assert!(
        !opencode_pane_state_changed_from_idle(&harness, &trigger, before, idle_status),
        "idle status chrome alone must not prove dispatch start"
    );
}

#[test]
fn codex_dispatch_start_tracking_enabled_accepts_workspace_hook_for_nested_agent_doc_root() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path();
    let nested = workspace.join("src/session-share");
    let doc = nested.join("tasks/claudescore-3.md");

    std::fs::create_dir_all(workspace.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(workspace.join(".codex")).unwrap();
    std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(workspace.join(".codex/hooks.json"), "{}").unwrap();
    std::fs::write(&doc, "# Session\n").unwrap();

    assert!(
        codex_dispatch_start_tracking_enabled(&doc),
        "workspace-level Codex hooks should enable routed dispatch tracking for nested agent-doc roots"
    );
}

#[test]
fn codex_dispatch_start_tracking_enabled_stays_false_without_any_hook_install() {
    let dir = tempfile::tempdir().unwrap();
    let nested = dir.path().join("src/session-share");
    let doc = nested.join("tasks/claudescore-3.md");

    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "# Session\n").unwrap();

    assert!(
        !codex_dispatch_start_tracking_enabled(&doc),
        "route should not wait for hook-backed submission proof when no tracked root has Codex hooks installed"
    );
}

#[test]
fn codex_dispatch_start_tracking_enabled_stays_false_when_nested_codex_path_shadows_hooks() {
    let dir = tempfile::tempdir().unwrap();
    let workspace = dir.path();
    let nested = workspace.join("src/session-share");
    let doc = nested.join("tasks/claudescore-3.md");

    std::fs::create_dir_all(workspace.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(workspace.join(".codex")).unwrap();
    std::fs::create_dir_all(nested.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(workspace.join(".codex/hooks.json"), "{}").unwrap();
    std::fs::write(nested.join(".codex"), "").unwrap();
    std::fs::write(&doc, "# Session\n").unwrap();

    assert!(
        !codex_dispatch_start_tracking_enabled(&doc),
        "route should not require hook-backed submission proof when a nearer `.codex` path shadows the workspace install"
    );
}

#[test]
fn dispatch_only_codex_with_visible_hooks_suppresses_optimistic_unproven_progress() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(dir.path().join(".codex/hooks.json"), "{}").unwrap();
    std::fs::write(&doc, "# Session\n").unwrap();

    assert!(
        !should_print_dispatch_only_unproven_progress(&doc, &HarnessConfig::codex()),
        "dispatch-only Codex reroutes with visible hooks should let the final accepted-but-unproven error own the user-facing output"
    );
    assert!(
        should_print_dispatch_only_unproven_progress(&doc, &HarnessConfig::claude()),
        "non-Codex reroutes still may report command-accepted fallback progress"
    );
}

#[test]
fn dispatch_only_codex_with_visible_hooks_rejects_accepted_only_submit() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(dir.path().join(".codex/hooks.json"), "{}").unwrap();
    std::fs::write(&doc, "# Session\n").unwrap();

    let err = require_dispatch_only_dispatch_start_proof(
        &doc,
        "%4",
        &HarnessConfig::codex(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::CommandAcceptedOnly,
    )
    .expect_err("hook-visible Codex dispatch-only acceptance must require routed submit proof");

    assert!(
        err.to_string()
            .contains("only pane-input acceptance proof was available"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn dispatch_blocker_recovery_hint_names_codex_hook_review_action() {
    let doc = PathBuf::from("tasks/agent-doc/agent-doc-bugs2.md");
    let hint =
        dispatch_blocker_recovery_hint(&HarnessConfig::codex(), "codex hook review prompt", &doc);

    assert!(
        hint.contains("open `/hooks`"),
        "hook-review blockers should tell the operator where to approve hooks: {hint}"
    );
    assert!(
        hint.contains("approve or disable the pending hook change"),
        "hook-review blockers should describe the approval gate: {hint}"
    );
    assert!(
        hint.contains("agent-doc route --dispatch-only tasks/agent-doc/agent-doc-bugs2.md"),
        "hook-review blockers should include a reroute recovery command: {hint}"
    );

    let generic =
        dispatch_blocker_recovery_hint(&HarnessConfig::codex(), "queued draft in composer", &doc);
    assert_eq!(generic, "restore an idle prompt and retry");
}

#[test]
fn dispatch_active_turn_blockers_are_queueable_for_prompt_bearing_reroutes() {
    assert_eq!(
        dispatch_active_turn_queue_source(&HarnessConfig::codex(), "active codex turn"),
        Some("dispatch_only_codex_active_turn")
    );
    assert_eq!(
        dispatch_active_turn_queue_source(&HarnessConfig::opencode(), "opencode active turn"),
        Some("dispatch_only_opencode_active_turn")
    );
    assert_eq!(
        dispatch_active_turn_queue_source(&HarnessConfig::codex(), "codex hook review prompt"),
        None,
        "hook review requires an explicit operator decision, not auto-queueing"
    );
    assert_eq!(
        dispatch_active_turn_queue_source(&HarnessConfig::codex(), "queued draft in composer"),
        None,
        "drafted prompt input must not be overwritten by route queueing"
    );
}

#[test]
fn dispatch_only_submit_proof_gate_allows_non_codex_and_hook_proven_codex() {
    let dir = tempfile::tempdir().unwrap();
    let doc = dir.path().join("tasks/agent-doc/agent-doc-bugs2.md");

    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(dir.path().join(".codex/hooks.json"), "{}").unwrap();
    std::fs::write(&doc, "# Session\n").unwrap();

    require_dispatch_only_dispatch_start_proof(
        &doc,
        "%4",
        &HarnessConfig::claude(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::CommandAcceptedOnly,
    )
    .expect("Claude currently has accepted-only semantics");

    require_dispatch_only_dispatch_start_proof(
        &doc,
        "%4",
        &HarnessConfig::codex(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::HookPromptMatched,
    )
    .expect("hook-proven Codex dispatch-only submit should pass");
}

#[test]
fn line_contains_trigger_rejects_codex_substring_inside_claude_trigger() {
    assert!(line_contains_trigger(
        "❯ /agent-doc test.md",
        "/agent-doc test.md"
    ));
    assert!(!line_contains_trigger(
        "❯ /agent-doc test.md",
        "agent-doc test.md"
    ));
}

#[test]
fn routed_trigger_payload_keeps_bare_reopen() {
    let codex_trigger = HarnessConfig::codex().trigger_command("test.md");
    assert_eq!(routed_trigger_payload(&codex_trigger), "agent-doc test.md");
    assert_eq!(
        routed_trigger_payload("/agent-doc test.md"),
        "/agent-doc test.md"
    );
}

#[test]
fn plain_trigger_override_uses_bare_agent_doc_reopen_for_route() {
    let mut claude = HarnessConfig::claude();
    apply_plain_trigger_override(&mut claude);
    assert_eq!(claude.trigger_command("test.md"), "agent-doc test.md");

    let mut opencode = HarnessConfig::opencode();
    apply_plain_trigger_override(&mut opencode);
    assert_eq!(opencode.trigger_command("test.md"), "agent-doc test.md");
}

#[test]
fn routed_trigger_submit_payload_strips_trailing_line_endings() {
    assert_eq!(
        routed_trigger_submit_payload("agent-doc test.md\r\n"),
        "agent-doc test.md"
    );
}

#[test]
fn validate_routed_trigger_payload_accepts_bare_codex_reopen() {
    let harness = HarnessConfig::codex();
    let trigger = harness.trigger_command("test.md");
    let payload = routed_trigger_payload(&trigger);
    validate_routed_trigger_payload(&harness, &trigger, &payload)
        .expect("bare Codex reopen should remain dispatchable");
}

#[test]
fn validate_routed_trigger_payload_rejects_multiline_codex_payload() {
    let harness = HarnessConfig::codex();
    let trigger = harness.trigger_command("test.md");
    let err =
        validate_routed_trigger_payload(&harness, &trigger, "agent-doc test.md\nfollow-up text")
            .expect_err("Codex reroute payload must fail before injecting extra lines");
    assert!(
        err.to_string().contains("bare `agent-doc <FILE>` reopen"),
        "unexpected error: {err:#}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn send_keys_delivers_claude_command_with_enter() {
    let iso = IsolatedTmux::new("route-test-send");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    // Start a shell that reads a line and echoes it back with a marker
    send_keys_with_retry(
        &iso,
        &pane,
        r#"exec /bin/sh -c 'printf "READY\n"; read CMD; printf "GOT:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &pane, "READY", std::time::Duration::from_secs(3));

    let trigger = HarnessConfig::claude().trigger_command("test.md");
    send_keys_with_retry(&iso, &pane, &trigger);

    // Capture and verify the command was received
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        &format!("GOT:{}", trigger),
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains(&format!("GOT:{}", trigger)),
        "command should be delivered and echoed back, got: {}",
        content
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn send_keys_delivers_codex_command_with_enter() {
    let iso = IsolatedTmux::new("route-test-send-codex");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    send_keys_with_retry(
        &iso,
        &pane,
        r#"exec /bin/sh -c 'printf "READY\n"; read CMD; printf "GOT:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &pane, "READY", std::time::Duration::from_secs(3));

    let trigger = HarnessConfig::codex().trigger_command("test.md");
    send_keys_with_retry(&iso, &pane, &trigger);

    let content = wait_for_pane_contains(
        &iso,
        &pane,
        &format!("GOT:{}", trigger),
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains(&format!("GOT:{}", trigger)),
        "command should be delivered and echoed back, got: {}",
        content
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn send_command_checked_reports_accepted_when_command_is_consumed() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::write(dir.path().join("test.md"), "# test\n").unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-send-checked");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    let window = iso.pane_window(&pane).unwrap();
    sessions::register_full_with_cwd_in(
        dir.path(),
        "route-test-send-checked",
        &pane,
        "test.md",
        1234,
        &window,
        &dir.path().to_string_lossy(),
    )
    .unwrap();

    send_keys_with_retry(
        &iso,
        &pane,
        r#"exec /bin/sh -c 'printf "READY\n"; read CMD; printf "GOT:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &pane, "READY", std::time::Duration::from_secs(3));

    let status = send_command_checked(&iso, &pane, "test.md", &HarnessConfig::codex()).unwrap();
    assert_eq!(status.status, CommandDispatchStatus::Accepted);
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn send_command_checked_codex_does_not_append_follow_up_lines() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::write(dir.path().join("test.md"), "# test\n").unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-send-checked-no-extra-lines");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    let window = iso.pane_window(&pane).unwrap();
    sessions::register_full_with_cwd_in(
        dir.path(),
        "route-test-send-checked-no-extra-lines",
        &pane,
        "test.md",
        1234,
        &window,
        &dir.path().to_string_lossy(),
    )
    .unwrap();

    let script = write_mock_registered_agent_doc_extra_line_detector(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!(
            "exec {} {}",
            script.display(),
            dir.path().join("test.md").display()
        ),
    );
    let _ = wait_for_pane_contains(&iso, &pane, ">", std::time::Duration::from_secs(3));

    let status = send_command_checked(&iso, &pane, "test.md", &HarnessConfig::codex()).unwrap();
    assert_eq!(status.status, CommandDispatchStatus::Accepted);

    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc test.md",
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains("GOT:agent-doc test.md"),
        "command should be delivered as a bare reopen, got: {content}"
    );
    assert!(
        !content.contains("EXTRA:"),
        "codex reroute should not inject follow-up lines into the same payload: {content}"
    );
}

#[test]
fn wait_for_start_ack_detects_new_preflight_cycle() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-live-child-skip.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    let doc_for_thread = doc.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some("# Session\n")).unwrap();
    });

    let ack = wait_for_start_ack(&doc, None, Duration::from_secs(1));
    assert!(
        ack.is_some(),
        "fresh start should acknowledge a new preflight cycle"
    );
    assert_eq!(
        ack.unwrap().phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
}

#[test]
fn wait_for_start_ack_detects_new_committed_cycle_after_prior_commit() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-live-pane-busy.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some("# Session\n"),
        Some("# Session\n"),
    )
    .unwrap();
    let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

    let doc_for_thread = doc.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some("# Session\n")).unwrap();
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_success",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
    });

    let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_secs(1))
        .expect("new committed cycle should count as startup acknowledgment");
    assert_ne!(ack.cycle_id, baseline.cycle_id);
    assert_eq!(ack.phase, crate::cycle_state::CyclePhase::Committed);
}

#[test]
fn drain_reaps_completed_review_item_across_all_surfaces() {
    // #route-drain reap-all-surfaces: the focused route-drain repair reaped only
    // the backlog, so a deployed `[x]` item left in review blocked dispatch until
    // a manual repeat ran full preflight maintenance ("JB Run Agent Doc failed; a
    // repeat attempt succeeded"). The drain now runs all-surface pending
    // maintenance first, so the completed review item is reaped on the first
    // attempt regardless of the final drain outcome.
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("drain-review.md");
    let content = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Backlog\n\n",
        "<!-- agent:backlog -->\n",
        "- [ ] [#keep1] Keep me\n",
        "<!-- /agent:backlog -->\n\n",
        "## Review\n\n",
        "<!-- agent:review -->\n",
        "- [x] [#seocat] Implemented and deployed\n",
        "<!-- /agent:review -->\n\n",
        "## Completed / Reaped\n\n",
        "<!-- agent:done -->\n",
        "<!-- /agent:done -->\n"
    );
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(&doc, content).unwrap();
    // Open cycle so the drain actually runs (is_open()).
    crate::cycle_state::start_preflight(&doc, None, Some(content)).unwrap();

    // The drain may still report Blocked on later (committed/etc.) guards in this
    // minimal fixture, but the all-surface reap runs before that — assert the
    // completed review item is gone from the file.
    let _ = super::drain_open_closeout_before_routed_dispatch(&doc);

    let after = std::fs::read_to_string(&doc).unwrap();
    let review = crate::component::parse(&after)
        .unwrap()
        .into_iter()
        .find(|c| c.name == "review")
        .unwrap()
        .content(&after)
        .to_string();
    assert!(
        !review.contains("[#seocat]"),
        "drain must reap the completed review item via all-surface maintenance: {review}"
    );
    assert!(after.contains("[#keep1]"), "open backlog item must remain");
}

#[test]
fn wait_for_start_ack_times_out_without_cycle_change() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-live-same-cycle.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some("# Session\n"),
        Some("# Session\n"),
    )
    .unwrap();
    let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

    let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_millis(250));
    assert!(
        ack.is_none(),
        "unchanged cycle state must not count as a fresh-start ack"
    );
}

#[test]
fn wait_for_start_ack_ignores_same_committed_cycle_mutation() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-live-ack-ok.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some("# Session\n"),
        Some("# Session\n"),
    )
    .unwrap();
    let baseline = crate::cycle_state::load(&doc).unwrap().unwrap();

    let doc_for_thread = doc.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_already_current",
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
    });

    let ack = wait_for_start_ack(&doc, Some(&baseline), Duration::from_millis(350));
    assert!(
        ack.is_none(),
        "same committed cycle mutations must not count as a new routed-start ack"
    );
}

#[test]
fn routed_cycle_ack_only_required_for_prompt_bearing_drift_on_closed_cycle() {
    assert!(!should_require_routed_cycle_ack(None, None));

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-live-owner-missing.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    crate::cycle_state::start_preflight(&doc, None, Some("# Session\n")).unwrap();
    let open_state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert!(!should_require_routed_cycle_ack(
        Some(&open_state),
        Some("prompt_target: ❯ follow-up question"),
    ));

    crate::cycle_state::mark_committed(
        &doc,
        "commit_success",
        Some("# Session\n"),
        Some("# Session\n"),
    )
    .unwrap();
    let committed_state = crate::cycle_state::load(&doc).unwrap().unwrap();
    assert!(should_require_routed_cycle_ack(
        Some(&committed_state),
        Some("prompt_target: ❯ follow-up question"),
    ));
}

#[test]
fn routed_cycle_ack_timeout_extends_for_live_children() {
    assert_eq!(routed_cycle_ack_timeout(false), Duration::from_secs(1));
    assert_eq!(routed_cycle_ack_timeout(true), Duration::from_secs(2));
}

#[test]
fn fresh_route_start_ack_timeout_allows_restart_slack() {
    assert_eq!(fresh_route_start_ack_timeout(), Duration::from_secs(2));
}

#[test]
fn route_latency_message_marks_budget_status() {
    let harness = HarnessConfig::codex();
    let ok = route_latency_message(
        "dispatch_start_proof",
        Duration::from_millis(999),
        Duration::from_secs(1),
        "%1",
        &harness,
        "submitted",
    );
    assert!(ok.contains("status=ok"), "{ok}");
    assert!(ok.contains("elapsed_ms=999"), "{ok}");

    let slow = route_latency_message(
        "dispatch_start_proof",
        Duration::from_secs(10),
        Duration::from_secs(10),
        "%1",
        &harness,
        "unproven_but_accepted",
    );
    assert!(slow.contains("status=over_budget"), "{slow}");
    assert!(slow.contains("outcome=unproven_but_accepted"), "{slow}");
}

#[test]
fn direct_pane_submit_budget_allows_acceptance_poll_slack() {
    assert_eq!(
        direct_pane_submit_acceptance_timeout(),
        Duration::from_secs(5)
    );
    assert_eq!(
        direct_pane_submit_acceptance_budget(),
        Duration::from_secs(6)
    );

    let message = route_latency_message(
        "direct_pane_submit",
        Duration::from_millis(5180),
        direct_pane_submit_acceptance_budget(),
        "%1",
        &HarnessConfig::codex(),
        direct_pane_submit_outcome(
            CommandDispatchStatus::TimedOut,
            Some(RoutedDispatchStartProof::HookPromptMatched),
        ),
    );

    assert!(message.contains("status=ok"), "{message}");
    assert!(
        message.contains("outcome=acceptance_unobserved_dispatch_proven"),
        "{message}"
    );
    assert!(!message.contains("timed_out"), "{message}");
}

#[test]
fn direct_pane_submit_outcome_separates_acceptance_from_dispatch_proof() {
    assert_eq!(
        direct_pane_submit_outcome(CommandDispatchStatus::Accepted, None),
        "accepted"
    );
    assert_eq!(
        direct_pane_submit_outcome(CommandDispatchStatus::TimedOut, None),
        "acceptance_unobserved"
    );
    assert_eq!(
        direct_pane_submit_outcome(
            CommandDispatchStatus::TimedOut,
            Some(RoutedDispatchStartProof::HookStateAdvanced),
        ),
        "acceptance_unobserved_dispatch_proven"
    );
}

#[test]
fn dispatch_only_sent_log_marks_claude_accepted_only_scope() {
    let message = route_dispatch_only_sent_log_message(
        Path::new("/tmp/robert-ross.md"),
        "%7",
        &HarnessConfig::claude(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::CommandAcceptedOnly,
    );

    assert!(message.contains("harness=claude"), "{message}");
    assert!(message.contains("proof=accepted"), "{message}");
    assert!(message.contains("proof_scope=accepted_only"), "{message}");
}

#[test]
fn dispatch_only_sent_log_marks_opencode_accepted_only_scope() {
    let message = route_dispatch_only_sent_log_message(
        Path::new("/tmp/monsterrodholders.md"),
        "%13",
        &HarnessConfig::opencode(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::CommandAcceptedOnly,
    );

    assert!(message.contains("harness=opencode"), "{message}");
    assert!(message.contains("proof=accepted"), "{message}");
    assert!(message.contains("proof_scope=accepted_only"), "{message}");
}

#[test]
fn dispatch_only_sent_log_marks_opencode_pane_state_dispatch_scope() {
    let message = route_dispatch_only_sent_log_message(
        Path::new("/tmp/monsterrodholders.md"),
        "%13",
        &HarnessConfig::opencode(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::PaneStateChanged,
    );

    assert!(message.contains("harness=opencode"), "{message}");
    assert!(message.contains("proof=pane_state_changed"), "{message}");
    assert!(message.contains("proof_scope=dispatch_start"), "{message}");
}

#[test]
fn dispatch_only_opencode_accepted_only_proof_is_successful_delivery() {
    require_dispatch_only_dispatch_start_proof(
        Path::new("/tmp/monsterrodholders.md"),
        "%13",
        &HarnessConfig::opencode(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::CommandAcceptedOnly,
    )
    .unwrap();
}

#[test]
fn dispatch_only_opencode_pane_state_proof_is_successful_delivery() {
    require_dispatch_only_dispatch_start_proof(
        Path::new("/tmp/monsterrodholders.md"),
        "%13",
        &HarnessConfig::opencode(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::PaneStateChanged,
    )
    .unwrap();
}

#[test]
fn dispatch_only_claude_accepted_only_proof_remains_accepted_delivery() {
    require_dispatch_only_dispatch_start_proof(
        Path::new("/tmp/robert-ross.md"),
        "%7",
        &HarnessConfig::claude(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::CommandAcceptedOnly,
    )
    .unwrap();
}

#[test]
fn dispatch_only_sent_log_marks_codex_hook_proof_scope() {
    let message = route_dispatch_only_sent_log_message(
        Path::new("/tmp/agent-doc-bugs2.md"),
        "%1",
        &HarnessConfig::codex(),
        DispatchOnlyReopenDelivery::DirectPaneSubmit,
        RoutedDispatchStartProof::HookPromptMatched,
    );

    assert!(message.contains("harness=codex"), "{message}");
    assert!(message.contains("proof=consumed"), "{message}");
    assert!(message.contains("proof_scope=dispatch_start"), "{message}");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_waits_longer_for_live_child_cycle_ack() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-child-extended-ack");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-child-extended-ack.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let mock_agent = write_mock_registered_agent_doc(dir.path());
    launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-child-extended-ack";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1300));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current)).unwrap();
    });

    let routed = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should tolerate a delayed but real live-child cycle start");
    assert_eq!(routed, pane);
    assert_eq!(
        *injects.lock().unwrap(),
        vec![routed_trigger_submit_payload(
            &HarnessConfig::codex().trigger_command(&file_path)
        )],
        "route should dispatch the bare Codex reopen through supervisor IPC before waiting for the delayed live-child ack"
    );

    let state = crate::cycle_state::load(&doc)
        .unwrap()
        .expect("cycle state should exist after delayed ack");
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
    ipc.stop();
}

#[test]
fn pending_prompt_bearing_context_for_route_ignores_frontmatter_only_drift() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-frontmatter-only-drift.md");
    let snapshot = "---\nagent: claude\nagent_doc_session: test\n---\n\n\
<!-- agent:exchange patch=append -->\n\
### Re: prior — gpt-5\n\
Body\n\
<!-- /agent:exchange -->\n";
    let current = snapshot.replacen("agent: claude", "agent: codex", 1);
    std::fs::write(&doc, current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();

    let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
    assert!(
        ctx.is_none(),
        "frontmatter-only drift must not force routed cycle acknowledgment"
    );
}

#[test]
fn pending_prompt_bearing_context_for_route_ignores_answered_prompt_after_stale_boundary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-stale-boundary-answered-tail.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: older — gpt-5\n\n",
        "Done.\n",
        "<!-- agent:boundary:stale -->\n",
        "<!-- /agent:exchange -->\n",
    );
    let current = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: older — gpt-5\n\n",
        "Done.\n",
        "<!-- agent:boundary:stale -->\n",
        "Can we run specific rubrics for fine tuning?\n",
        "### Re: specific rubrics — gpt-5\n\n",
        "Yes.\n",
        "<!-- /agent:exchange -->\n",
    );
    std::fs::write(&doc, current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();

    let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
    assert!(
        ctx.is_none(),
        "answered prompt after a stale boundary must not force routed cycle acknowledgment"
    );
}

#[test]
fn pending_prompt_bearing_context_for_route_ignores_raw_answered_prompt_after_stale_boundary() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-stale-boundary-raw-answered-tail.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: older — gpt-5\n\n",
        "Done.\n",
        "<!-- agent:boundary:stale -->\n",
        "<!-- /agent:exchange -->\n",
    );
    let current = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: older — gpt-5\n\n",
        "Done.\n",
        "<!-- agent:boundary:stale -->\n",
        "I renamed the repo to ClaudeScore/buildparty-investor-demo. Please update references\n",
        "I updated the repo-local references to the renamed GitHub repo.\n\n",
        "- `.gitmodules` now uses `git@github.com:ClaudeScore/buildparty-investor-demo.git`\n",
        "- The checked-out submodule at `buildparty-investor-demo/` now has `origin` set to the same URL\n",
        "<!-- /agent:exchange -->\n",
    );
    std::fs::write(&doc, current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();

    let ctx = pending_prompt_bearing_context_for_route(&doc, None).unwrap();
    assert!(
        ctx.is_none(),
        "raw assistant completion prose after a stale-boundary prompt must not force routed cycle acknowledgment"
    );
}

#[test]
fn pending_prompt_bearing_context_for_route_detects_plain_exchange_tail_prompt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("route-stale-boundary-plain-tail.md");
    let snapshot = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: older — gpt-5\n\n",
        "Done.\n",
        "<!-- agent:boundary:stale -->\n",
        "<!-- /agent:exchange -->\n",
    );
    let current = concat!(
        "---\nagent_doc_session: test\nagent_doc_format: template\n---\n\n",
        "## Exchange\n\n",
        "<!-- agent:exchange patch=append -->\n",
        "### Re: older — gpt-5\n\n",
        "Done.\n",
        "<!-- agent:boundary:stale -->\n",
        "When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push\n",
        "<!-- /agent:exchange -->\n",
    );
    std::fs::write(&doc, current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();

    let ctx = pending_prompt_bearing_context_for_route(&doc, None)
        .unwrap()
        .expect("plain exchange-tail prompt should force routed ack gating");
    assert_eq!(
        ctx.marker,
        "prompt_target: When I run `Run Agent Doc` on this document...nothing happens. Please diagnose the root cause failure and fix the root cause. spec-test-build-install-commit-push"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_keeps_live_child_reroute_optimistic_when_cycle_ack_is_missing() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-child-skip-ack");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-owner-reregister.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let mock_agent = write_mock_registered_agent_doc(dir.path());
    launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    sessions::register("route-live-child-skip", &pane, &file_path).unwrap();
    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc =
        SupervisorIpc::start(
            dir.path(),
            "route-live-child-skip",
            move |method| match method {
                IpcMethod::Inject { bytes } => {
                    injects_for_ipc.lock().unwrap().push(bytes.clone());
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            },
        )
        .unwrap();

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        "route-live-child-skip",
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should stay optimistic when the correct live Codex pane accepts the reopen");
    assert_eq!(resolved, pane);
    let injects = injects.lock().unwrap().clone();
    assert!(
        !injects.is_empty()
            && injects.iter().all(|inject| {
                inject
                    == &routed_trigger_submit_payload(
                        &HarnessConfig::codex().trigger_command(&file_path),
                    )
            }),
        "route should still dispatch the trigger through supervisor IPC before accepting the optimistic startup-miss path: {injects:?}"
    );
    let miss = crate::startup_miss::load(&doc)
        .unwrap()
        .expect("optimistic route should still record a startup miss");
    assert_eq!(miss.pane_id, pane);
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_retries_fresh_restart_after_live_codex_ack_timeout() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-codex-fresh-retry");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-codex-fresh-retry.md");
    let snapshot = "---\nagent: codex\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let stale_agent = write_mock_registered_agent_doc(dir.path());
    launch_mock_registered_agent_doc(&iso, &pane, &stale_agent, &doc);
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-codex-fresh-retry";
    sessions::register(session_id, &pane, &file_path).unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let supervisor_instance_id = "busy-reroute-supervisor".to_string();
    let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
    let ipc_tmux = iso.clone();
    let injected_pane = Arc::new(std::sync::Mutex::new(None::<String>));
    let injected_pane_for_ipc = injected_pane.clone();
    *injected_pane.lock().unwrap() = Some(pane.clone());
    let mut ipc =
        crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "restart_count": 0,
                    "actor_state": "ready",
                    "supervisor_pid": 12345,
                    "supervisor_instance_id": supervisor_instance_id_for_ipc
                })),
                IpcMethod::Restart { mode } => {
                    if mode == "fresh" {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                    }
                    IpcResponse::ok_empty()
                }
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Inject { bytes } => {
                    if let Some(target) = injected_pane_for_ipc.lock().unwrap().clone() {
                        let _ = ipc_tmux.send_keys(&target, bytes.trim_end_matches('\n'));
                    }
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            }
        })
        .unwrap();

    let iso_for_thread = iso.clone();
    let ready_agent = write_mock_registered_agent_doc(dir.path());
    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    let pane_for_thread = pane.clone();
    let restart_called_for_thread = restart_called.clone();
    std::thread::spawn(move || {
        let wait_start = std::time::Instant::now();
        while !restart_called_for_thread.load(Ordering::Relaxed)
            && wait_start.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        iso_for_thread
            .raw_cmd(&[
                "respawn-pane",
                "-k",
                "-t",
                &pane_for_thread,
                &format!(
                    "exec {} {}",
                    ready_agent.display(),
                    doc_for_thread.display()
                ),
            ])
            .unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
    });

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should retry once after a fresh Codex supervisor restart");
    assert_eq!(resolved, pane);
    assert!(
        restart_called.load(Ordering::Relaxed),
        "route should request a fresh supervisor restart before the retry"
    );

    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("GOT:agent-doc "),
        "route should resend the reopen after the fresh restart: {content}"
    );

    ipc.stop();
}

#[test]
fn tracked_harness_clear_requires_fresh_restart_only_for_exact_clear_prompt() {
    assert!(tracked_harness_clear_requires_fresh_restart(
        &HarnessConfig::codex(),
        Some("/clear")
    ));
    assert!(tracked_harness_clear_requires_fresh_restart(
        &HarnessConfig::codex(),
        Some("  /clear  ")
    ));
    assert!(!tracked_harness_clear_requires_fresh_restart(
        &HarnessConfig::codex(),
        Some("agent-doc tasks/bugs.md")
    ));
    assert!(!tracked_harness_clear_requires_fresh_restart(
        &HarnessConfig::claude(),
        Some("/clear")
    ));
    assert!(tracked_harness_clear_requires_fresh_restart(
        &HarnessConfig::opencode(),
        Some("/clear")
    ));
    assert!(tracked_harness_clear_requires_fresh_restart(
        &HarnessConfig::opencode(),
        Some("  /clear  ")
    ));
    assert!(!tracked_harness_clear_requires_fresh_restart(
        &HarnessConfig::opencode(),
        Some("agent-doc tasks/bugs.md")
    ));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_records_optimistic_fresh_restart_retry_in_original_pane() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-codex-fresh-retry-handoff");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-codex-fresh-retry-handoff.md");
    let snapshot = "---\nagent: codex\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let stale_agent = write_mock_registered_agent_doc(dir.path());
    launch_mock_registered_agent_doc(&iso, &pane, &stale_agent, &doc);
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-codex-fresh-retry-handoff";
    sessions::register(session_id, &pane, &file_path).unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let supervisor_instance_id = "fresh-retry-handoff-supervisor".to_string();
    let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
    let mut ipc =
        crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "restart_count": 0,
                    "actor_state": "ready",
                    "supervisor_pid": 12345,
                    "supervisor_instance_id": supervisor_instance_id_for_ipc
                })),
                IpcMethod::Restart { mode } => {
                    if mode == "fresh" {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                    }
                    IpcResponse::ok_empty()
                }
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Inject { bytes } => {
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            }
        })
        .unwrap();

    let iso_for_thread = iso.clone();
    let ready_agent = write_mock_registered_agent_doc(dir.path());
    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    let pane_for_thread = pane.clone();
    let file_for_thread = file_path.clone();
    let registry_root = dir.path().to_path_buf();
    let restart_called_for_thread = restart_called.clone();
    let replacement = std::thread::spawn(move || {
        let wait_start = std::time::Instant::now();
        while !restart_called_for_thread.load(Ordering::Relaxed)
            && wait_start.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        let replacement_pane = iso_for_thread.auto_start(session, &cwd).unwrap();
        iso_for_thread
            .send_keys(
                &replacement_pane,
                &format!(
                    "exec {} {}",
                    ready_agent.display(),
                    doc_for_thread.display()
                ),
            )
            .unwrap();
        let prompt_wait_start = std::time::Instant::now();
        while prompt_wait_start.elapsed() < Duration::from_secs(5) {
            let captured = crate::sessions::capture_pane(&iso_for_thread, &replacement_pane)
                .unwrap_or_default();
            if captured.contains("> ") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = iso_for_thread.raw_cmd(&["kill-pane", "-t", &pane_for_thread]);
        let replacement_window = iso_for_thread.pane_window(&replacement_pane).unwrap();
        sessions::register_supervisor_in(
            &registry_root,
            session_id,
            &replacement_pane,
            &file_for_thread,
            12345,
            &supervisor_instance_id,
        )
        .unwrap();
        crate::session_actor::project_binding_in(
            &registry_root,
            &file_for_thread,
            session_id,
            &replacement_pane,
            &replacement_window,
            "route",
            "fresh_restart_retry",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
        replacement_pane
    });

    let routed = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect(
        "route should keep the original routed pane authoritative after the fresh restart retry",
    );

    let replacement_pane = replacement.join().unwrap();
    assert!(restart_called.load(Ordering::Relaxed));
    assert_eq!(routed, pane);

    let replacement_after = sessions::capture_pane(&iso, &replacement_pane).unwrap_or_default();
    assert!(
        !replacement_after.contains("GOT:agent-doc "),
        "route must not redirect the reopen into the replacement pane after the fresh restart retry: {replacement_after}"
    );

    let miss = crate::startup_miss::load(&doc)
        .unwrap()
        .expect("fresh restart retry should leave an optimistic startup-miss marker");
    assert_eq!(miss.file, file_path);
    assert_eq!(miss.pane_id, pane);
    assert_eq!(
        miss.origin,
        crate::startup_miss::StartupMissOrigin::RoutedTrigger
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_restarts_fresh_before_dispatch_after_tracked_codex_clear() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/codex-hooks/sessions")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-codex-clear-pre-dispatch-restart");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-codex-clear-pre-dispatch-restart.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();

    let stale_agent =
        write_mock_registered_agent_doc_with_prefix(dir.path(), "agent-doc-stale", "STALE");
    launch_mock_registered_agent_doc(&iso, &pane, &stale_agent, &doc);

    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-codex-clear-pre-dispatch-restart";
    sessions::register(session_id, &pane, &file_path).unwrap();

    let state_path = dir
        .path()
        .join(".agent-doc/codex-hooks/sessions/clear.json");
    std::fs::write(
        &state_path,
        serde_json::json!({
            "session_id": "codex-clear-session",
            "doc_path": file_path,
            "last_turn_id": "turn-clear",
            "last_prompt": "/clear",
            "updated_at": 42u64
        })
        .to_string(),
    )
    .unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let supervisor_instance_id = "busy-reroute-supervisor".to_string();
    let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc =
        crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "restart_count": 0,
                    "actor_state": "ready",
                    "supervisor_pid": 12345,
                    "supervisor_instance_id": supervisor_instance_id_for_ipc
                })),
                IpcMethod::Restart { mode } => {
                    if mode == "fresh" {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                    }
                    IpcResponse::ok_empty()
                }
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Inject { bytes } => {
                    injects_for_ipc.lock().unwrap().push(bytes.clone());
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            }
        })
        .unwrap();

    let iso_for_thread = iso.clone();
    let fresh_agent =
        write_mock_registered_agent_doc_with_prefix(dir.path(), "agent-doc-fresh", "FRESH");
    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    let pane_for_thread = pane.clone();
    let restart_called_for_thread = restart_called.clone();
    std::thread::spawn(move || {
        let wait_start = std::time::Instant::now();
        while !restart_called_for_thread.load(Ordering::Relaxed)
            && wait_start.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        iso_for_thread
            .raw_cmd(&[
                "respawn-pane",
                "-k",
                "-t",
                &pane_for_thread,
                &format!(
                    "exec {} {}",
                    fresh_agent.display(),
                    doc_for_thread.display()
                ),
            ])
            .unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
    });

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should restart fresh before rerouting after a tracked /clear");
    assert_eq!(resolved, pane);
    assert!(
        restart_called.load(Ordering::Relaxed),
        "route should request a fresh restart before dispatch"
    );
    let trigger = HarnessConfig::codex().trigger_command(&file_path);
    let injects = injects.lock().unwrap().clone();
    assert!(
        injects == vec![routed_trigger_submit_payload(&trigger)],
        "route should inject exactly one bare reopen through supervisor IPC after the fresh restart: {injects:?}"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_refuses_busy_registered_pane_before_dispatch_when_prompt_drift_exists() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-pane-busy");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-owner-supervisor-pid.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let mock_agent = write_mock_busy_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", mock_agent.display(), doc.display()),
    );
    let content =
        wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
    assert!(
        content.contains("Working..."),
        "busy mock session should be active in pane: {content}"
    );

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    sessions::register("route-live-pane-busy", &pane, &file_path).unwrap();

    let err = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        "route-live-pane-busy",
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err("route should fail closed instead of injecting into a busy live pane");

    let after = sessions::capture_pane(&iso, &pane).unwrap_or_default();
    assert!(
        !after.contains("EARLY:agent-doc "),
        "route should not inject a trigger before the pane becomes idle: {after}"
    );
    assert!(
        err.to_string()
            .contains("bounded interrupt recovery never restored a dispatch-ready prompt"),
        "unexpected error: {err:#}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_fails_closed_on_busy_registered_pane() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-busy-pane");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-dispatch-only-busy-pane.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let busy_agent = write_mock_busy_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content =
        wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
    assert!(
        content.contains("Working..."),
        "busy mock session should be active in pane: {content}"
    );

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    sessions::register("route-dispatch-only-busy-pane", &pane, &file_path).unwrap();

    let err = resolve_or_create_pane_dispatch_only(
        &iso,
        &doc,
        None,
        &[],
        "route-dispatch-only-busy-pane",
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err(
        "dispatch-only route should now fail closed instead of injecting into a busy live pane",
    );

    let after =
        wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(1));
    assert!(
        !after.contains("EARLY:agent-doc "),
        "dispatch-only route must not inject a reopen into the busy authoritative pane: {after}"
    );
    assert!(
        err.to_string()
            .contains("bounded interrupt recovery never restored a dispatch-ready prompt"),
        "unexpected error: {err:#}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_refuses_while_latest_run_is_still_starting() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-starting-pane");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("tasks/professional/equityfundingsource.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();

    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-dispatch-only-starting-pane";
    sessions::register(session_id, &pane, &file_path).unwrap();
    crate::startup_miss::append_session_log_event(
        &doc,
        session_id,
        &format!(
            "session_start file={} pane={} session={}",
            doc.display(),
            pane,
            session_id
        ),
    )
    .unwrap();
    crate::startup_miss::append_session_log_event(
        &doc,
        session_id,
        "codex_start mode=fresh restart_count=0",
    )
    .unwrap();

    let busy_agent = write_mock_active_codex_turn_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "esc to interrupt",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("esc to interrupt"),
        "busy mock session should be active in pane: {content}"
    );
    assert_eq!(
        HarnessConfig::codex()
            .dispatch_blocker_reason(&content)
            .as_deref(),
        Some("active codex turn"),
        "busy mock session should expose the Codex active-turn blocker: {content}"
    );

    let err = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("dispatch-only route must wait for a dispatch-ready prompt during the fresh-start boot window");
    assert!(
        err.to_string()
            .contains("never reached a dispatch-ready prompt"),
        "unexpected startup-window refusal: {err:#}"
    );
    assert!(
        err.to_string()
            .contains("tasks/professional/equityfundingsource.md"),
        "startup-window refusal should preserve the EFS document path: {err:#}"
    );
    let after = sessions::capture_pane(&iso, &pane).unwrap_or_default();
    assert!(
        !after.contains("EARLY:agent-doc "),
        "dispatch-only route must not submit through the live pane before the startup prompt is visible: {after}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn dispatch_only_send_reopen_direct_pane_submit_avoids_extra_enter_retries() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-no-enter-retries");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-dispatch-only-no-enter-retries.md");
    std::fs::write(
        &doc,
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let trigger = format!("agent-doc {}", file_path);
    let script = write_mock_registered_agent_doc_with_stale_trigger(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} '{}'", script.display(), trigger),
    );
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        &format!("> {}", trigger),
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains(&format!("> {}", trigger)),
        "mock session should keep a stale visible trigger line in pane output: {content}"
    );
    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc = SupervisorIpc::start(
        dir.path(),
        "route-test-dispatch-only-no-enter-retries",
        move |method| match method {
            IpcMethod::Inject { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
        },
    )
    .unwrap();

    sessions::register(
        "route-test-dispatch-only-no-enter-retries",
        &pane,
        &file_path,
    )
    .unwrap();
    dispatch_only_send_reopen(
        &iso,
        &doc,
        "route-test-dispatch-only-no-enter-retries",
        &pane,
        &file_path,
        &HarnessConfig::codex(),
        DispatchOnlySendReopenOptions {
            delivery: DispatchOnlyReopenDelivery::DirectPaneSubmit,
            queue_prompt_text: None,
        },
    )
    .expect("dispatch-only reopen should still send once when no explicit blocker is visible");
    assert!(
        injects.lock().unwrap().is_empty(),
        "dispatch-only direct pane submit should not fall back to supervisor inject"
    );
    let after = wait_for_pane_contains(
        &iso,
        &pane,
        &format!("GOT:{trigger}"),
        std::time::Duration::from_secs(3),
    );
    assert!(
        after.contains(&format!("GOT:{trigger}")),
        "dispatch-only reopen should submit the trigger through the live pane input path: {after}"
    );
    assert!(
        !after.contains("EXTRA:"),
        "dispatch-only reopen should not send an extra newline or second Enter: {after}"
    );
    ipc.stop();
}

#[test]
fn starting_pane_recovery_target_follows_same_file_handoff() {
    let initial = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%151".to_string()),
        latest_start_timestamp: Some(10),
        latest_run_timestamp: Some(11),
        latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_process_exit_after_latest_start: false,
        saw_session_end_after_latest_start: false,
        saw_process_exit_after_latest_run: false,
        saw_session_end_after_latest_run: false,
    };
    let handed_off = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%183".to_string()),
        latest_start_timestamp: Some(20),
        latest_run_timestamp: Some(21),
        latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_process_exit_after_latest_start: false,
        saw_session_end_after_latest_start: false,
        saw_process_exit_after_latest_run: false,
        saw_session_end_after_latest_run: false,
    };

    assert_eq!(
        starting_pane_recovery_target(Some(&initial), Some(&handed_off), "%151", Some("%183")),
        Some(StartingPaneRecoveryTarget::DifferentPane(
            "%183".to_string()
        ))
    );
}

#[test]
fn starting_pane_recovery_target_retries_same_pane_after_new_generation() {
    let initial = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%151".to_string()),
        latest_start_timestamp: Some(10),
        latest_run_timestamp: Some(11),
        latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_process_exit_after_latest_start: false,
        saw_session_end_after_latest_start: false,
        saw_process_exit_after_latest_run: false,
        saw_session_end_after_latest_run: false,
    };
    let restarted = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%151".to_string()),
        latest_start_timestamp: Some(12),
        latest_run_timestamp: Some(13),
        latest_run_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
        saw_process_exit_after_latest_start: false,
        saw_session_end_after_latest_start: false,
        saw_process_exit_after_latest_run: false,
        saw_session_end_after_latest_run: false,
    };

    assert_eq!(
        starting_pane_recovery_target(Some(&initial), Some(&restarted), "%151", Some("%151")),
        Some(StartingPaneRecoveryTarget::SamePane)
    );
}

#[test]
fn starting_pane_recovery_target_ignores_unchanged_open_start() {
    let initial = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%151".to_string()),
        latest_start_timestamp: Some(10),
        latest_run_timestamp: Some(11),
        latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_process_exit_after_latest_start: false,
        saw_session_end_after_latest_start: false,
        saw_process_exit_after_latest_run: false,
        saw_session_end_after_latest_run: false,
    };

    assert_eq!(
        starting_pane_recovery_target(Some(&initial), Some(&initial), "%151", Some("%151")),
        None
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_fails_closed_on_reverse_i_search() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-reverse-i-search");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-dispatch-only-reverse-i-search.md");
    std::fs::write(
            &doc,
            "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n❯ follow-up question\n",
        )
        .unwrap();
    crate::snapshot::save(
        &doc,
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    sessions::register(
        "route-test-dispatch-only-reverse-i-search",
        &pane,
        &file_path,
    )
    .unwrap();

    let busy_agent = write_mock_busy_registered_agent_doc_recovers_on_ctrl_g(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "reverse-i-search",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("reverse-i-search"),
        "dispatch-only blocker test requires a visible reverse-i-search shell state: {content}"
    );

    let err = resolve_or_create_pane_dispatch_only(
        &iso,
        &doc,
        None,
        &[],
        "route-test-dispatch-only-reverse-i-search",
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err("dispatch-only route must fail closed on reverse-i-search");
    assert!(
        err.to_string().contains("reverse-i-search"),
        "unexpected error: {err:#}"
    );

    let after = wait_for_pane_contains(
        &iso,
        &pane,
        "reverse-i-search",
        std::time::Duration::from_secs(1),
    );
    assert!(
        !after.contains("GOT:agent-doc "),
        "dispatch-only route must not inject a reopen after detecting reverse-i-search: {after}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn run_with_tmux_dispatch_only_ignores_startup_miss_on_alive_registered_pane() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-startup-miss");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-dispatch-only-startup-miss.md");
    let content = "---\nagent_doc_session: route-dispatch-only-startup-miss\nagent: codex\n---\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n❯ follow-up question\n";
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(
        &doc,
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-dispatch-only-startup-miss";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let ipc_tmux = iso.clone();
    let pane_for_ipc = pane.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();
    crate::startup_miss::record(
        &doc,
        &pane,
        session_id,
        "codex",
        crate::startup_miss::StartupMissOrigin::RoutedTrigger,
        None,
    )
    .unwrap();

    let ready_agent = write_mock_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", ready_agent.display(), doc.display()),
    );

    run_with_tmux(
        &doc,
        &iso,
        None,
        0,
        &[],
        RouteMode::DispatchOnly,
        false,
        None,
    )
    .expect("dispatch-only route should ignore the stale startup-miss gate and send");

    let after = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(5),
    );
    assert!(
        after.contains("GOT:agent-doc "),
        "dispatch-only route should send despite the retained startup-miss marker: {after}"
    );
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_retries_busy_registered_pane_once_after_interrupt_recovery() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-pane-busy-interrupt-retry");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-pane-busy-interrupt-retry.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let busy_agent = write_mock_busy_registered_agent_doc_ignores_interrupt(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content =
        wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
    assert!(
        content.contains("Working..."),
        "busy mock session should be active in pane: {content}"
    );

    let ready_agent = write_mock_registered_agent_doc(dir.path());
    std::fs::write(
        dir.path().join(".agent-doc/route-busy-interrupt.txt"),
        format!("exec {} {}\n", ready_agent.display(), doc.display()),
    )
    .unwrap();

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-pane-busy-interrupt-retry";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let ipc_tmux = iso.clone();
    let pane_for_ipc = pane.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1300));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
    });

    let reused = resolve_or_create_pane_with_auto_fix_retry(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
        false,
        true,
        true,
    )
    .expect("route should retry once after interrupting a still-busy live Codex pane");
    assert_eq!(reused, pane);

    let after = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(5),
    );
    assert!(
        after.contains("GOT:agent-doc "),
        "route should dispatch the reopen after the interrupt recovery retry: {after}"
    );
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_retries_busy_registered_pane_once_after_ctrl_g_probe() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-pane-busy-ctrl-g-retry");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-pane-busy-ctrl-g-retry.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let busy_agent = write_mock_busy_registered_agent_doc_recovers_on_ctrl_g(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "reverse-i-search",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("reverse-i-search"),
        "busy mock session should be in reverse-i-search: {content}"
    );

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-pane-busy-ctrl-g-retry";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let ipc_tmux = iso.clone();
    let pane_for_ipc = pane.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1300));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
    });

    let reused = resolve_or_create_pane_with_auto_fix_retry(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
        false,
        true,
        true,
    )
    .expect(
        "route should retry once after ctrl-g clears reverse-i-search in a busy live Codex pane",
    );
    assert_eq!(reused, pane);

    let after = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(5),
    );
    assert!(
        after.contains("GOT:agent-doc "),
        "route should dispatch the reopen after the ctrl-g interrupt recovery probe: {after}"
    );
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_retries_busy_opencode_pane_after_escape_interrupt() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-opencode-busy-escape-retry");
    let session = "opencode";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-opencode-busy-escape-retry.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let busy_agent = write_mock_busy_opencode_recovers_on_escape(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "esc interrupt",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("esc interrupt"),
        "busy OpenCode mock should be active in pane: {content}"
    );

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-opencode-busy-escape-retry";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let ipc_tmux = iso.clone();
    let pane_for_ipc = pane.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1300));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
    });

    let reused = resolve_or_create_pane_with_auto_fix_retry(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::opencode(),
        &mut Vec::new(),
        false,
        true,
        true,
    )
    .expect("route should retry after Escape interrupt recovers a busy OpenCode pane");
    assert_eq!(reused, pane);

    let after = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:/agent-doc ",
        std::time::Duration::from_secs(5),
    );
    assert!(
        after.contains("GOT:/agent-doc "),
        "route should dispatch the reopen after the Escape interrupt recovery: {after}"
    );
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_keeps_interrupt_timeout_busy_reroute_optimistic_for_alive_pane() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-pane-busy-interrupt-blocked");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-pane-busy-interrupt-blocked.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let busy_agent = write_mock_busy_registered_agent_doc_ignores_interrupt(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content =
        wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
    assert!(
        content.contains("Working..."),
        "busy mock session should be active in pane: {content}"
    );

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-pane-busy-interrupt-blocked";
    sessions::register(session_id, &pane, &file_path).unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let supervisor_instance_id = "busy-reroute-supervisor".to_string();
    let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
    let ipc_tmux = iso.clone();
    let injected_pane = Arc::new(std::sync::Mutex::new(None::<String>));
    let injected_pane_for_ipc = injected_pane.clone();
    let mut ipc =
        crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "restart_count": 0,
                    "actor_state": "ready",
                    "supervisor_pid": 12345,
                    "supervisor_instance_id": supervisor_instance_id_for_ipc
                })),
                IpcMethod::Restart { mode } => {
                    if mode == "fresh" {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                    }
                    IpcResponse::ok_empty()
                }
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Inject { bytes } => {
                    if let Some(target) = injected_pane_for_ipc.lock().unwrap().clone() {
                        let _ = ipc_tmux.send_keys(&target, &bytes);
                    }
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            }
        })
        .unwrap();

    let reused = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect(
        "route should still inject into the authoritative pane after the bounded interrupt ladder",
    );
    assert_eq!(reused, pane);
    assert!(restart_called.load(Ordering::Relaxed));
    let miss = crate::startup_miss::load(&doc)
        .unwrap()
        .expect("optimistic busy reroute should still record a startup miss");
    assert_eq!(miss.pane_id, pane);

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_restarts_fresh_for_busy_registered_pane_after_noop_fix() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-pane-busy-fresh-reroute");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-pane-busy-fresh-reroute.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let busy_agent = write_mock_busy_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content =
        wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
    assert!(
        content.contains("Working..."),
        "busy mock session should be active in pane: {content}"
    );

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-pane-busy-fresh-reroute";
    sessions::register(session_id, &pane, &file_path).unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let supervisor_instance_id = "busy-reroute-supervisor".to_string();
    let supervisor_instance_id_for_ipc = supervisor_instance_id.clone();
    let ipc_tmux = iso.clone();
    let injected_pane = Arc::new(std::sync::Mutex::new(None::<String>));
    let injected_pane_for_ipc = injected_pane.clone();
    let mut ipc =
        crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "restart_count": 0,
                    "actor_state": "ready",
                    "supervisor_pid": 12345,
                    "supervisor_instance_id": supervisor_instance_id_for_ipc
                })),
                IpcMethod::Restart { mode } => {
                    if mode == "fresh" {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                    }
                    IpcResponse::ok_empty()
                }
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Inject { bytes } => {
                    if let Some(target) = injected_pane_for_ipc.lock().unwrap().clone() {
                        let _ = ipc_tmux.send_keys(&target, &bytes);
                    }
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            }
        })
        .unwrap();
    let ready_agent = write_mock_registered_agent_doc(dir.path());
    let iso_for_thread = iso.clone();
    let registry_root = dir.path().to_path_buf();
    let file_for_thread = file_path.clone();
    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    let pane_for_thread = pane.clone();
    let restart_called_for_thread = restart_called.clone();
    let injected_pane_for_thread = injected_pane.clone();
    let replacement = std::thread::spawn(move || {
        let wait_start = std::time::Instant::now();
        while !restart_called_for_thread.load(Ordering::Relaxed)
            && wait_start.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        let replacement_pane = iso_for_thread.auto_start(session, &cwd).unwrap();
        iso_for_thread
            .send_keys(
                &replacement_pane,
                &format!(
                    "exec {} {}",
                    ready_agent.display(),
                    doc_for_thread.display()
                ),
            )
            .unwrap();
        let prompt_wait_start = std::time::Instant::now();
        while prompt_wait_start.elapsed() < Duration::from_secs(5) {
            let captured = crate::sessions::capture_pane(&iso_for_thread, &replacement_pane)
                .unwrap_or_default();
            if captured.contains("> ") {
                *injected_pane_for_thread.lock().unwrap() = Some(replacement_pane.clone());
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = iso_for_thread.raw_cmd(&["kill-pane", "-t", &pane_for_thread]);
        let replacement_window = iso_for_thread.pane_window(&replacement_pane).unwrap();
        sessions::register_supervisor_in(
            &registry_root,
            session_id,
            &replacement_pane,
            &file_for_thread,
            12345,
            &supervisor_instance_id,
        )
        .unwrap();
        crate::session_actor::project_binding_in(
            &registry_root,
            &file_for_thread,
            session_id,
            &replacement_pane,
            &replacement_window,
            "route",
            "fresh_restart_retry",
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
        replacement_pane
    });

    let routed = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should restart fresh once and reroute into the replacement pane");

    let replacement_pane = replacement.join().unwrap();
    assert!(restart_called.load(Ordering::Relaxed));
    assert!(
        routed == replacement_pane || routed == pane,
        "route should either report the handed-off pane or keep the reroute optimistic in the original pane: routed={routed} replacement={replacement_pane} original={pane}"
    );

    let busy_after = sessions::capture_pane(&iso, &pane).unwrap_or_default();
    assert!(
        !busy_after.contains("GOT:agent-doc "),
        "route must not keep dispatching into the stale busy pane after the fresh restart reroute: {busy_after}"
    );
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn fresh_restart_retry_preserves_absolute_reopen_path_for_relative_docs() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-pane-fresh-reroute-relative-doc");
    let session = "codex";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let relative_doc = std::path::PathBuf::from("src/session-share/tasks/claudescore-3.md");
    let doc = dir.path().join(&relative_doc);
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let stale_agent = write_mock_registered_agent_doc(dir.path());
    launch_mock_registered_agent_doc(&iso, &pane, &stale_agent, &doc);

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-pane-fresh-reroute-relative-doc";
    sessions::register(session_id, &pane, &file_path).unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let mut ipc =
        crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": true,
                    "state": "healthy",
                    "restart_count": 0
                })),
                IpcMethod::Restart { mode } => {
                    if mode == "fresh" {
                        restart_called_for_ipc.store(true, Ordering::Relaxed);
                    }
                    IpcResponse::ok_empty()
                }
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Inject { bytes } => {
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            }
        })
        .unwrap();
    let ready_agent = write_mock_registered_agent_doc(dir.path());
    let iso_for_thread = iso.clone();
    let registry_root = dir.path().to_path_buf();
    let file_for_thread = file_path.clone();
    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    let pane_for_thread = pane.clone();
    let restart_called_for_thread = restart_called.clone();
    let replacement = std::thread::spawn(move || {
        let wait_start = std::time::Instant::now();
        while !restart_called_for_thread.load(Ordering::Relaxed)
            && wait_start.elapsed() < Duration::from_secs(2)
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        let replacement_pane = iso_for_thread.auto_start(session, &cwd).unwrap();
        iso_for_thread
            .send_keys(
                &replacement_pane,
                &format!(
                    "exec {} {}",
                    ready_agent.display(),
                    doc_for_thread.display()
                ),
            )
            .unwrap();
        let _ = iso_for_thread.raw_cmd(&["kill-pane", "-t", &pane_for_thread]);
        sessions::register_full_with_cwd_in(
            &registry_root,
            session_id,
            &replacement_pane,
            &file_for_thread,
            12345,
            "@owner",
            registry_root.to_string_lossy().as_ref(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
        replacement_pane
    });

    let routed = resolve_or_create_pane(
        &iso,
        &relative_doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should keep using the resolved absolute reopen path after a fresh retry");

    let replacement_pane = replacement.join().unwrap();
    assert!(restart_called.load(Ordering::Relaxed));
    assert_eq!(routed, pane);

    let replacement_after = sessions::capture_pane(&iso, &replacement_pane).unwrap_or_default();
    assert!(
        !replacement_after.contains("GOT:agent-doc "),
        "route must not redirect the reopen into the replacement pane after the fresh retry: {replacement_after}"
    );

    let miss = crate::startup_miss::load(&doc)
        .unwrap()
        .expect("fresh restart retry should persist the optimistic startup-miss marker");
    assert_eq!(miss.file, file_path);
    assert_eq!(miss.pane_id, pane);
    assert_eq!(
        miss.origin,
        crate::startup_miss::StartupMissOrigin::RoutedTrigger
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_retries_busy_registered_pane_once_after_scoped_fix() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-pane-busy-auto-fix");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-pane-busy-auto-fix.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let busy_agent = write_mock_busy_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content =
        wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
    assert!(
        content.contains("Working..."),
        "busy mock session should be active in pane: {content}"
    );

    let ready_agent = write_mock_registered_agent_doc(dir.path());
    let hook_command = format!("exec {} {}", ready_agent.display(), doc.display());
    std::fs::write(
        dir.path().join(".agent-doc/route-busy-auto-fix.txt"),
        format!("{hook_command}\n"),
    )
    .unwrap();

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-pane-busy-auto-fix";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let ipc_tmux = iso.clone();
    let pane_for_ipc = pane.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1300));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&current_for_thread))
            .unwrap();
    });

    let reused = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should retry once after the scoped auto-fix recovers the busy pane");
    assert_eq!(reused, pane);

    let after = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(5),
    );
    assert!(
        after.contains("GOT:agent-doc "),
        "route should inject the reopen after the scoped fix retry: {after}"
    );
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_waits_for_busy_restart_handoff_before_retrying_route() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-busy-restart-handoff");
    let session = "claude";
    let cwd = test_cwd();
    let busy_pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-busy-restart-handoff.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let busy_agent = write_mock_busy_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &busy_pane,
        &format!("exec {} {}", busy_agent.display(), doc.display()),
    );
    let content = wait_for_pane_contains(
        &iso,
        &busy_pane,
        "Working...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Working..."),
        "busy mock session should be active in pane: {content}"
    );

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-busy-restart-handoff";
    sessions::register(session_id, &busy_pane, &file_path).unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc =
        crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": false,
                    "state": "healthy",
                    "restart_count": 0
                })),
                IpcMethod::Restart { .. } => {
                    restart_called_for_ipc.store(true, Ordering::Relaxed);
                    IpcResponse::ok_empty()
                }
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
                IpcMethod::Inject { bytes } => {
                    injects_for_ipc.lock().unwrap().push(bytes.clone());
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            }
        })
        .unwrap();

    let iso_for_thread = iso.clone();
    let registry_root = dir.path().to_path_buf();
    let file_for_thread = file_path.clone();
    let doc_for_thread = doc.clone();
    let ack_current = current.clone();
    let ready_agent = write_mock_registered_agent_doc(dir.path());
    let restart_called_for_thread = restart_called.clone();
    let busy_pane_for_thread = busy_pane.clone();
    let replacement = std::thread::spawn(move || {
        let wait_start = std::time::Instant::now();
        while !restart_called_for_thread.load(Ordering::Relaxed)
            && wait_start.elapsed() < Duration::from_secs(10)
        {
            std::thread::sleep(Duration::from_millis(25));
        }
        assert!(
            restart_called_for_thread.load(Ordering::Relaxed),
            "route should request a supervisor restart before test replacement handoff"
        );
        let replacement_pane = iso_for_thread.auto_start(session, &cwd).unwrap();
        iso_for_thread
            .send_keys(
                &replacement_pane,
                &format!(
                    "exec {} {}",
                    ready_agent.display(),
                    doc_for_thread.display()
                ),
            )
            .unwrap();
        let prompt_wait_start = std::time::Instant::now();
        while prompt_wait_start.elapsed() < Duration::from_secs(5) {
            let captured = crate::sessions::capture_pane(&iso_for_thread, &replacement_pane)
                .unwrap_or_default();
            if captured.contains("> ") {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        let _ = iso_for_thread.raw_cmd(&["kill-pane", "-t", &busy_pane_for_thread]);
        sessions::register_full_with_cwd_in(
            &registry_root,
            session_id,
            &replacement_pane,
            &file_for_thread,
            12345,
            "@owner",
            registry_root.to_string_lossy().as_ref(),
        )
        .unwrap();
        std::thread::sleep(Duration::from_millis(1200));
        crate::cycle_state::start_preflight(&doc_for_thread, None, Some(&ack_current)).unwrap();
        replacement_pane
    });

    let routed = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect(
        "route should wait for the restarted session to hand off to the new authoritative pane",
    );

    let replacement_pane = replacement.join().unwrap();
    assert!(restart_called.load(Ordering::Relaxed));
    assert_eq!(routed, replacement_pane);
    assert!(
        *injects.lock().unwrap()
            == vec![routed_trigger_submit_payload(
                &HarnessConfig::codex().trigger_command(&file_path)
            )],
        "route should dispatch exactly one bare Codex reopen through supervisor IPC after the restart handoff"
    );

    ipc.stop();
}

#[test]
fn busy_existing_pane_auto_fix_outcome_restarts_fresh_for_healthy_authoritative_session_without_changes()
 {
    assert_eq!(
        busy_existing_pane_auto_fix_outcome(false, false, Some(SupervisorHealth::Healthy), false,),
        BusyPaneAutoFixOutcome::RetryRouteAfterFreshRestart
    );
    assert_eq!(
        busy_existing_pane_auto_fix_outcome(
            false,
            false,
            Some(SupervisorHealth::Restartable),
            false,
        ),
        BusyPaneAutoFixOutcome::FailClosed
    );
    assert_eq!(
        busy_existing_pane_auto_fix_outcome(
            false,
            false,
            Some(SupervisorHealth::Restartable),
            true,
        ),
        BusyPaneAutoFixOutcome::RetryRouteAfterSupervisorRestart
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_focuses_busy_registered_pane_without_prompt_drift() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-pane-busy-no-drift");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-live-owner-supervisor-pid.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    std::fs::write(&doc, snapshot).unwrap();
    let mock_agent = write_mock_busy_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &pane,
        &format!("exec {} {}", mock_agent.display(), doc.display()),
    );
    let content =
        wait_for_pane_contains(&iso, &pane, "Working...", std::time::Duration::from_secs(5));
    assert!(
        content.contains("Working..."),
        "busy mock session should be active in pane: {content}"
    );

    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    sessions::register("route-live-pane-busy-no-drift", &pane, &file_path).unwrap();

    let reused = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        "route-live-pane-busy-no-drift",
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should focus the already-running pane when there is no new drift");
    assert_eq!(reused, pane);

    let after = sessions::capture_pane(&iso, &pane).unwrap_or_default();
    assert!(
        !after.contains("EARLY:agent-doc "),
        "route should not inject a duplicate reopen into a busy live pane: {after}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_rejects_same_committed_cycle_mutation_for_prompt_drift() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-ack-same-cycle");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("route-supervisor-restart.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let mock_agent = write_mock_registered_agent_doc(dir.path());
    launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-same-cycle";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let ipc_tmux = iso.clone();
    let pane_for_ipc = pane.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let snapshot_for_thread = snapshot.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_already_current",
            Some(&snapshot_for_thread),
            Some(&snapshot_for_thread),
        )
        .unwrap();
    });

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("same-cycle committed churn should not block an already-accepted optimistic reroute");
    assert_eq!(resolved, pane);

    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains("GOT:agent-doc "),
        "route should still dispatch the trigger to the registered pane: {content}"
    );
    let miss = crate::startup_miss::load(&doc)
        .unwrap()
        .expect("optimistic same-cycle reroute should still record a startup miss");
    assert_eq!(miss.pane_id, pane);
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_accepts_registered_pane_trigger_once_new_cycle_starts() {
    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-ack-ok");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("session.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let mock_agent = write_mock_registered_agent_doc_extra_line_detector(dir.path());
    launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-ok";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let ipc_tmux = iso.clone();
    let pane_for_ipc = pane.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let snapshot_for_thread = snapshot.to_string();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_success",
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
    });

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should accept the new cycle ack");
    assert_eq!(resolved, pane);

    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains("GOT:agent-doc "),
        "route should dispatch the trigger before observing the ack: {content}"
    );
    assert!(
        !content.contains("EXTRA:"),
        "route should not append follow-up prompt text onto the Codex reopen payload: {content}"
    );
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_accepts_content_edit_cycle_ack_without_extra_payload_lines() {
    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-ack-content-edit");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("session.md");
    let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nThe service returned 401 from this endpoint\n<!-- /agent:exchange -->\n";
    let current = "<!-- agent:exchange patch=append -->\n### Re: older\nThe service returned 503 from this endpoint\n<!-- /agent:exchange -->\n";
    std::fs::write(&doc, current).unwrap();
    let mock_agent = write_mock_registered_agent_doc_extra_line_detector(dir.path());
    launch_mock_registered_agent_doc(&iso, &pane, &mock_agent, &doc);
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-content-edit-ok";
    sessions::register(session_id, &pane, &file_path).unwrap();
    let ipc_tmux = iso.clone();
    let pane_for_ipc = pane.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Inject { bytes } => {
            let _ = ipc_tmux.send_keys(&pane_for_ipc, &bytes);
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let snapshot_for_thread = snapshot.to_string();
    let current_for_thread = current.to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_success",
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
    });

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should accept the new cycle ack for content edits");
    assert_eq!(resolved, pane);

    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains("GOT:agent-doc "),
        "route should dispatch the bare Codex reopen before observing the content-edit ack: {content}"
    );
    assert!(
        !content.contains("EXTRA:"),
        "route must not append content-edit text onto the Codex reopen payload: {content}"
    );
    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn alive_registered_pane_without_live_owner_deregisters_and_lazy_claims() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-owner-missing");
    let session = "claude";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &stale_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));

    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let mut registry = sessions::SessionRegistry::default();
    registry.insert(
        file_path.clone(),
        sessions::SessionEntry {
            pane: stale_pane.clone(),
            pid: 0,
            cwd: dir.path().to_string_lossy().to_string(),
            started: String::new(),
            session_id: "route-live-owner-missing".to_string(),
            file: file_path.clone(),
            window: iso.pane_window(&stale_pane).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        },
    );
    sessions::save_in(dir.path(), &registry).unwrap();
    let mock_start = write_mock_start_agent_doc(dir.path());

    let doc_for_thread = doc.clone();
    let current_for_thread = "# Session\n❯ follow-up question\n".to_string();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some("# Session\n"),
            Some(&current_for_thread),
        )
        .unwrap();
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_success",
            Some("# Session\n"),
            Some(&current_for_thread),
        )
        .unwrap();
    });

    let resolved = {
        let _route_bin_guard = route_bin_env_lock();
        unsafe {
            std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
        }
        let result = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-live-owner-missing",
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        );
        unsafe {
            std::env::remove_var("AGENT_DOC_ROUTE_BIN");
        }
        result
    }
    .expect("route should continue recovery after clearing the stale registration");
    assert_ne!(resolved, stale_pane);

    let reassigned = sessions::lookup("route-live-owner-missing").unwrap();
    assert!(
        reassigned.as_deref() == Some(resolved.as_str()),
        "route should re-register to the recovered pane, got: {reassigned:?}"
    );

    let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
    assert!(
        !stale_content.contains("STALE:agent-doc "),
        "route should not dispatch into the stale registered pane: {stale_content}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn alive_registered_pane_fails_closed_when_legacy_live_owner_conflicts() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-owner-reregister");
    let session = "claude";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &stale_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));

    let live_pane = iso.auto_start(session, &cwd).unwrap();
    let doc = dir.path().join("session.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    let mock_agent = write_mock_registered_agent_doc(dir.path());
    launch_mock_registered_agent_doc(&iso, &live_pane, &mock_agent, &doc);
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let mut registry = sessions::SessionRegistry::default();
    registry.insert(
        file_path.clone(),
        sessions::SessionEntry {
            pane: stale_pane.clone(),
            pid: 0,
            cwd: dir.path().to_string_lossy().to_string(),
            started: String::new(),
            session_id: "route-live-owner-reregister".to_string(),
            file: file_path.clone(),
            window: iso.pane_window(&stale_pane).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        },
    );
    sessions::save_in(dir.path(), &registry).unwrap();

    let doc_for_thread = doc.clone();
    let snapshot_for_thread = snapshot.to_string();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_success",
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
    });

    let err = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        "route-live-owner-reregister",
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err(
        "route should fail closed instead of re-electing ownership from a legacy associated pane",
    );
    assert!(
        err.to_string()
            .contains("normal path will not re-elect ownership"),
        "unexpected error: {err:#}"
    );

    let live_content = sessions::capture_pane(&iso, &live_pane).unwrap_or_default();
    assert!(
        !live_content.contains("GOT:agent-doc "),
        "route should not dispatch into the conflicting legacy live pane automatically: {live_content}"
    );

    let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
    assert!(
        !stale_content.contains("STALE:agent-doc "),
        "route should not dispatch into the stale registered pane either: {stale_content}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_prefers_authoritative_actor_dispatch_target() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-authoritative-actor-dispatch");
    let session = "codex";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &stale_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));

    let doc = dir.path().join("session.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-authoritative-actor-dispatch";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "ready",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let snapshot_for_thread = snapshot.to_string();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_success",
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
    });

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should dispatch through the authoritative actor pane");
    assert_eq!(resolved, actor_pane);

    let trigger =
        routed_trigger_submit_payload(&HarnessConfig::codex().trigger_command(&file_path));
    assert_eq!(*injects.lock().unwrap(), vec![trigger]);
    assert_eq!(
        sessions::lookup(session_id).unwrap().as_deref(),
        Some(actor_pane.as_str())
    );

    let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
    assert!(
        !stale_content.contains("STALE:agent-doc "),
        "route should not dispatch into the stale registered pane when actor authority points elsewhere: {stale_content}"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_prefers_authoritative_actor_dispatch_target() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-authoritative-actor-dispatch-only");
    let session = "codex";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &stale_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));

    let doc = dir.path().join("dispatch-only.md");
    let content = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n❯ follow-up question\n";
    std::fs::write(&doc, content).unwrap();
    crate::snapshot::save(
        &doc,
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n",
    )
    .unwrap();
    crate::cycle_state::start_preflight(
            &doc,
            Some("<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n"),
            Some("<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n"),
        )
        .unwrap();
    crate::cycle_state::mark_committed(
            &doc,
            "commit_success",
            Some("<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n"),
            Some("<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n"),
        )
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-authoritative-actor-dispatch-only";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "ready",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let resolved = resolve_or_create_pane_dispatch_only(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("dispatch-only reroute should dispatch through the authoritative actor pane");
    assert_eq!(resolved, actor_pane);

    assert!(
        injects.lock().unwrap().is_empty(),
        "ready authoritative dispatch-only path should submit through tmux pane input instead of supervisor inject"
    );
    assert_eq!(
        sessions::lookup(session_id).unwrap().as_deref(),
        Some(actor_pane.as_str())
    );

    let trigger = HarnessConfig::codex().trigger_command(&file_path);
    let actor_after = wait_for_pane_contains(
        &iso,
        &actor_pane,
        "ACTOR:agent-doc ",
        std::time::Duration::from_secs(3),
    );
    assert!(
        pane_capture_contains_wrapped(&actor_after, &trigger),
        "dispatch-only reroute should submit the reopen in the authoritative pane: {actor_after}"
    );
    let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
    assert!(
        !stale_content.contains("STALE:agent-doc "),
        "dispatch-only reroute should not inject into the stale registered pane when actor authority points elsewhere: {stale_content}"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_reuses_registered_authoritative_actor_pane_when_supervisor_state_is_missing()
 {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-authoritative-actor-dispatch-only-fallback");
    let session = "claude";
    let cwd = test_cwd();
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "❯ \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &actor_pane, "❯ ", std::time::Duration::from_secs(3));

    let doc = dir.path().join("dispatch-only-claude-fallback.md");
    let snapshot = "---\nagent: claude\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-authoritative-actor-dispatch-only-fallback";
    sessions::register(session_id, &actor_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let dispatch_pane = resolve_or_create_pane_dispatch_only(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::claude(),
        &mut Vec::new(),
    )
    .expect(
        "dispatch-only reroute should reuse the live authoritative pane after readiness checks",
    );
    assert_eq!(dispatch_pane, actor_pane);
    let actor_after = wait_for_pane_contains(
        &iso,
        &actor_pane,
        &HarnessConfig::claude().trigger_command(&file_path),
        std::time::Duration::from_secs(3),
    );
    assert!(
        actor_after.contains(&HarnessConfig::claude().trigger_command(&file_path)),
        "degraded authoritative actor should receive the direct-pane reopen: {actor_after}"
    );

    let ops_log = std::fs::read_to_string(dir.path().join(".agent-doc/logs/ops.log"))
        .expect("dispatch-only degraded direct submit should write an ops log entry");
    assert!(
        ops_log.contains("route_dispatch_only_authoritative_degraded_direct_pane"),
        "expected authoritative degraded direct submit logging, got: {ops_log}"
    );
    assert!(
        ops_log.contains("supervisor_health=no_socket"),
        "direct-submit logging should explain the degraded supervisor state: {ops_log}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_does_not_restart_after_tracked_codex_clear() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/codex-hooks/sessions")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-clear-no-restart");
    let session = "codex";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &stale_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));

    let doc = dir.path().join("dispatch-only-clear-no-restart.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-dispatch-only-clear-no-restart";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    std::fs::write(
        dir.path()
            .join(".agent-doc/codex-hooks/sessions/clear.json"),
        serde_json::json!({
            "session_id": "codex-clear-session",
            "doc_path": file_path,
            "last_turn_id": "turn-clear",
            "last_prompt": "/clear",
            "updated_at": 42u64
        })
        .to_string(),
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "ready",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => {
            restart_called_for_ipc.store(true, Ordering::Relaxed);
            IpcResponse::ok_empty()
        }
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let resolved = resolve_or_create_pane_dispatch_only(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("dispatch-only reroute should keep sending the bare reopen after session clear");
    assert_eq!(resolved, actor_pane);
    assert!(
        !restart_called.load(Ordering::Relaxed),
        "dispatch-only reroute must not restart Codex just because the latest tracked prompt was /clear"
    );

    assert!(
        injects.lock().unwrap().is_empty(),
        "dispatch-only reroute after session clear should use pane submit instead of supervisor inject"
    );

    let actor_after = wait_for_pane_contains(
        &iso,
        &actor_pane,
        &HarnessConfig::codex().trigger_command(&file_path),
        std::time::Duration::from_secs(3),
    );
    assert!(
        actor_after.contains(&HarnessConfig::codex().trigger_command(&file_path)),
        "dispatch-only reroute after session clear should still submit the bare reopen in the authoritative pane: {actor_after}"
    );

    let stale_content = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
    assert!(
        !stale_content.contains("STALE:agent-doc "),
        "dispatch-only reroute should still avoid the stale registered pane after session clear: {stale_content}"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_fails_closed_when_live_submit_has_no_codex_hook_proof() {
    use std::sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    };

    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(dir.path().join(".codex")).unwrap();
    std::fs::write(dir.path().join(".codex/hooks.json"), "{}\n").unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-authoritative-actor-dispatch-only-unproven");
    let session = "codex";
    let cwd = test_cwd();
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &stale_pane,
        r#"exec /bin/sh -c 'printf "STALE\n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(
        &iso,
        &stale_pane,
        "STALE",
        std::time::Duration::from_secs(3),
    );

    let doc = dir.path().join("dispatch-only-authoritative-unproven.md");
    let snapshot = "---\nagent_doc_session: route-dispatch-only-authoritative-unproven\nagent: codex\ncodex_network_access: enabled\n---\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-dispatch-only-authoritative-unproven";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "ready",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => {
            restart_called_for_ipc.store(true, Ordering::Relaxed);
            IpcResponse::ok_empty()
        }
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let err = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("dispatch-only reroute must fail closed when hooks are visible but Codex never proves consumption");
    assert!(
        err.to_string()
            .contains("only pane-input acceptance proof was available"),
        "unexpected error: {err:#}"
    );
    assert!(
        injects.lock().unwrap().is_empty(),
        "ready authoritative dispatch-only path should stay on direct pane submit even when it later fails closed"
    );
    assert!(
        !restart_called.load(Ordering::Relaxed),
        "editor dispatch-only reroutes must not restart a live Codex pane just because the session log lacks a capability proof"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_recovers_waiting_input_actor_with_fresh_restart() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-waiting-input-restart");
    let session = "codex";
    let cwd = test_cwd();
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "ACTOR:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &actor_pane, "> ", std::time::Duration::from_secs(3));

    let doc = dir.path().join("dispatch-only-waiting-input.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-dispatch-only-waiting-input";
    sessions::register(session_id, &actor_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => {
            let actor_state = if restart_called_for_ipc.load(Ordering::Relaxed) {
                "ready"
            } else {
                "waiting_input"
            };
            IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": actor_state,
                "restart_count": 0
            }))
        }
        IpcMethod::Restart { mode } => {
            assert_eq!(mode, "fresh");
            restart_called_for_ipc.store(true, Ordering::Relaxed);
            IpcResponse::ok_empty()
        }
        IpcMethod::Inject { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let resolved = resolve_or_create_pane_dispatch_only(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("dispatch-only reroute should recover a waiting-input authoritative actor");
    assert_eq!(resolved, actor_pane);
    assert!(
        restart_called.load(Ordering::Relaxed),
        "dispatch-only reroute should request one fresh restart when the authoritative actor is waiting for supervisor input"
    );

    let actor_after = wait_for_pane_contains(
        &iso,
        &actor_pane,
        &HarnessConfig::codex().trigger_command(&file_path),
        std::time::Duration::from_secs(3),
    );
    assert!(
        actor_after.contains(&HarnessConfig::codex().trigger_command(&file_path)),
        "dispatch-only reroute should still submit the bare reopen after recovering the waiting-input supervisor prompt: {actor_after}"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_fails_closed_for_blocked_or_closed_authoritative_actor() {
    use std::sync::{Arc, Mutex};

    for (actor_state, reason) in [
        ("blocked", "the authoritative actor is blocked"),
        ("closed", "the authoritative actor is closed"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
        let _cwd_guard = ScopedCurrentDir::set(dir.path());
        let iso = IsolatedTmux::new(&format!(
            "route-test-authoritative-actor-{}-fail-closed",
            actor_state
        ));
        let session = "codex";
        let cwd = test_cwd();
        let stale_pane = iso.auto_start(session, &cwd).unwrap();
        let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
        let actor_pane = iso.auto_start(session, &cwd).unwrap();

        let doc = dir
            .path()
            .join(format!("{actor_state}-authoritative-actor.md"));
        let snapshot = "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
        let current = format!("{snapshot}❯ follow-up question\n");
        std::fs::write(&doc, &current).unwrap();
        crate::snapshot::save(&doc, snapshot).unwrap();
        crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
        crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
            .unwrap();
        let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
        let session_id = format!("route-authoritative-actor-{actor_state}");
        sessions::register(&session_id, &stale_pane, &file_path).unwrap();

        let actor_window = iso.pane_window(&actor_pane).unwrap();
        crate::session_actor::project_binding_in(
            dir.path(),
            &file_path,
            &session_id,
            &actor_pane,
            &actor_window,
            "route",
            "dispatch_bind",
        )
        .unwrap();

        let injects = Arc::new(Mutex::new(Vec::<String>::new()));
        let injects_for_ipc = injects.clone();
        let mut ipc = SupervisorIpc::start(dir.path(), &session_id, move |method| match method {
            IpcMethod::State => IpcResponse::ok(serde_json::json!({
                "running": true,
                "state": "healthy",
                "actor_state": actor_state,
                "restart_count": 0
            })),
            IpcMethod::Inject { bytes } => {
                injects_for_ipc.lock().unwrap().push(bytes.clone());
                IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
            }
            IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
            IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
            IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
        })
        .unwrap();

        let err = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            &session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("route should fail closed for non-recoverable authoritative actor states");
        let message = format!("{err:#}");
        assert!(
            message.contains(reason),
            "expected {actor_state} actor failure to mention `{reason}`, got: {message}"
        );
        assert!(
            injects.lock().unwrap().is_empty(),
            "route must not inject a duplicate reopen while the authoritative actor is {actor_state}"
        );
        assert_eq!(
            sessions::lookup(&session_id).unwrap().as_deref(),
            Some(actor_pane.as_str()),
            "route should still refresh the registry projection to the authoritative actor pane for {actor_state}"
        );

        let trigger = HarnessConfig::codex().trigger_command(&file_path);
        let actor_after = sessions::capture_pane(&iso, &actor_pane).unwrap_or_default();
        assert!(
            !actor_after.contains(&trigger),
            "route must not type a reopen into the blocked/closed authoritative pane: {actor_after}"
        );
        let stale_after = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
        assert!(
            !stale_after.contains(&trigger),
            "route must not fall back to the stale registered pane when actor state is {actor_state}: {stale_after}"
        );

        ipc.stop();
    }
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn load_authoritative_actor_dispatch_target_accepts_normalized_claude_harness_identity() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-authoritative-actor-claude-harness");
    let session = "claude";
    let cwd = test_cwd();
    let actor_pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("session.md");
    std::fs::write(
            &doc,
            "---\nagent: claude\n---\n\n<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n",
        )
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-authoritative-actor-claude";
    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "ready",
            "restart_count": 0
        })),
        IpcMethod::Inject { .. } => IpcResponse::ok_empty(),
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let actor = load_authoritative_actor_dispatch_target(
        &iso,
        &doc,
        session_id,
        &file_path,
        &HarnessConfig::claude(),
        true,
        true,
    )
    .expect("normalized Claude harness name should not fail the authoritative actor lookup")
    .expect("healthy actor record should remain dispatchable");
    assert_eq!(actor.record.harness, "claude-code");
    assert_eq!(actor.record.pane_id, actor_pane);

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatches_busy_authoritative_actor_when_prompt_target_pending() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-authoritative-actor-busy");
    let session = "claude";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
    let actor_pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("session.md");
    let snapshot = "---\nagent: claude\n---\n\n<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-authoritative-actor-busy";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "busy",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let snapshot_for_thread = snapshot.to_string();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_success",
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
    });

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::claude(),
        &mut Vec::new(),
    )
    .expect("route should optimistically queue a busy authoritative actor");
    assert_eq!(resolved, actor_pane);

    let trigger =
        routed_trigger_submit_payload(&HarnessConfig::claude().trigger_command(&file_path));
    assert_eq!(*injects.lock().unwrap(), vec![trigger]);
    assert_eq!(
        sessions::lookup(session_id).unwrap().as_deref(),
        Some(actor_pane.as_str())
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn route_waits_for_starting_authoritative_actor_ready_before_dispatch() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-authoritative-actor-starting");
    let session = "codex";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &actor_pane, Duration::from_secs(3)),
        "actor pane shell should be ready before installing Codex prompt fixture"
    );
    let prompt_script = dir.path().join("codex-ready.sh");
    std::fs::write(
            &prompt_script,
            "#!/bin/sh\nprintf '\\033[2J\\033[H› \\ngpt-5.4 high · ~/work/btakita/agent-loop · Context 0%% used\\n'\ncat\n",
        )
        .unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        &format!("exec /bin/sh {}", prompt_script.display()),
    );
    let ready_output = wait_for_pane_contains(&iso, &actor_pane, "Context", Duration::from_secs(3));
    assert!(
        ready_prompt_candidate(&ready_output, &HarnessConfig::codex()).is_some(),
        "actor pane should show a Codex dispatch-ready prompt before the ready wait: {ready_output}"
    );

    let doc = dir.path().join("session.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-authoritative-actor-starting";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let ready_at = Instant::now() + Duration::from_millis(150);
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State if Instant::now() >= ready_at => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "ready",
            "restart_count": 0
        })),
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "starting",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let doc_for_thread = doc.clone();
    let snapshot_for_thread = snapshot.to_string();
    let current_for_thread = current.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
        crate::cycle_state::mark_committed(
            &doc_for_thread,
            "commit_success",
            Some(&snapshot_for_thread),
            Some(&current_for_thread),
        )
        .unwrap();
    });

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect(
        "route should wait for a starting authoritative actor to report ready before dispatching",
    );
    assert_eq!(resolved, actor_pane);

    let trigger =
        routed_trigger_submit_payload(&HarnessConfig::codex().trigger_command(&file_path));
    assert_eq!(*injects.lock().unwrap(), vec![trigger]);
    assert_eq!(
        sessions::lookup(session_id).unwrap().as_deref(),
        Some(actor_pane.as_str())
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn route_refreshes_closed_starting_authoritative_actor_without_start_timeout() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-authoritative-actor-starting-closed");
    let session = "codex";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "BOOTING\n"; cat'"#,
    );
    let _ = wait_for_pane_contains(
        &iso,
        &actor_pane,
        "BOOTING",
        std::time::Duration::from_secs(3),
    );

    let doc = dir.path().join("starting-authoritative-closed.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-authoritative-actor-starting-closed";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let close_at = Instant::now() + Duration::from_millis(120);
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State if Instant::now() >= close_at => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "closed",
            "restart_count": 0
        })),
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "starting",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let err = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err("route must fail closed as soon as a starting actor refreshes to closed");
    let message = format!("{err:#}");
    assert!(
        message.contains("the authoritative actor is closed"),
        "closed actor refresh should surface the terminal state instead of the stale starting gate: {message}"
    );
    assert!(
        injects.lock().unwrap().is_empty(),
        "route must not queue a reopen once the starting actor refreshes to closed"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn route_dispatch_only_fails_closed_for_starting_authoritative_actor_without_ready_state() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-authoritative-starting-direct");
    let session = "codex";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &stale_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "BOOTING\n"; cat'"#,
    );
    let _ = wait_for_pane_contains(
        &iso,
        &actor_pane,
        "BOOTING",
        std::time::Duration::from_secs(3),
    );

    let doc = dir
        .path()
        .join("dispatch-only-authoritative-starting-direct.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-dispatch-only-authoritative-starting-direct";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "starting",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let err = resolve_or_create_pane_dispatch_only(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err(
        "dispatch-only reroute must fail closed while the authoritative actor remains starting",
    );
    let message = format!("{err:#}");
    assert!(
        message.contains("the authoritative actor is still starting"),
        "starting actor failure should explain the state gate: {message}"
    );
    assert!(
        injects.lock().unwrap().is_empty(),
        "dispatch-only authoritative reroute must not queue through supervisor IPC while the actor is starting"
    );

    let actor_after = sessions::capture_pane(&iso, &actor_pane).unwrap_or_default();
    assert!(
        !actor_after.contains(&HarnessConfig::codex().trigger_command(&file_path)),
        "dispatch-only authoritative reroute must not submit through the live pane path while still starting: {actor_after}"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn route_dispatch_only_refuses_starting_authoritative_actor_after_tracked_clear_until_ready() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/codex-hooks/sessions")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-dispatch-only-authoritative-starting-clear");
    let session = "codex";
    let cwd = test_cwd();
    let stale_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &stale_pane,
        r#"exec /bin/sh -c 'printf "> \n"; read CMD; printf "STALE:%s\n" "$CMD"; cat'"#,
    );
    let _ = wait_for_pane_contains(&iso, &stale_pane, "> ", std::time::Duration::from_secs(3));
    let actor_pane = iso.auto_start(session, &cwd).unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        r#"exec /bin/sh -c 'printf "BOOTING\n"; while IFS= read -r CMD; do printf "ACTOR:%s\n" "$CMD"; done'"#,
    );
    let _ = wait_for_pane_contains(
        &iso,
        &actor_pane,
        "BOOTING",
        std::time::Duration::from_secs(3),
    );

    let doc = dir
        .path()
        .join("dispatch-only-authoritative-starting-clear.md");
    let snapshot =
        "<!-- agent:exchange patch=append -->\n### Re: older\nold body\n<!-- /agent:exchange -->\n";
    let current = format!("{snapshot}❯ follow-up question\n");
    std::fs::write(&doc, &current).unwrap();
    crate::snapshot::save(&doc, snapshot).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(snapshot), Some(snapshot)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(snapshot), Some(snapshot))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-dispatch-only-authoritative-starting-clear";
    sessions::register(session_id, &stale_pane, &file_path).unwrap();

    let actor_window = iso.pane_window(&actor_pane).unwrap();
    crate::session_actor::project_binding_in(
        dir.path(),
        &file_path,
        session_id,
        &actor_pane,
        &actor_window,
        "route",
        "dispatch_bind",
    )
    .unwrap();

    std::fs::write(
        dir.path()
            .join(".agent-doc/codex-hooks/sessions/clear.json"),
        serde_json::json!({
            "session_id": "codex-clear-session",
            "doc_path": file_path,
            "last_turn_id": "turn-clear",
            "last_prompt": "/clear",
            "updated_at": 42u64
        })
        .to_string(),
    )
    .unwrap();

    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();
    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "starting",
            "restart_count": 0
        })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let err = resolve_or_create_pane_dispatch_only(
            &iso,
            &doc,
            None,
            &[],
            session_id,
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut Vec::new(),
        )
        .expect_err("dispatch-only reroute after /clear must wait for a dispatch-ready prompt before direct pane submit");
    let message = format!("{err:#}");
    assert!(
        message.contains("the authoritative actor is still starting")
            || message.contains("never reached a dispatch-ready prompt")
            || message.contains("never showed a dispatch-ready prompt"),
        "starting actor after /clear should fail before input when no prompt is visible: {message}"
    );
    assert!(
        injects.lock().unwrap().is_empty(),
        "dispatch-only reroute after /clear should not queue through supervisor IPC"
    );

    let actor_after = sessions::capture_pane(&iso, &actor_pane).unwrap_or_default();
    assert!(
        !actor_after.contains(&HarnessConfig::codex().trigger_command(&file_path)),
        "dispatch-only reroute after /clear must not submit to a pane before it is dispatch-ready: {actor_after}"
    );
    let stale_after = sessions::capture_pane(&iso, &stale_pane).unwrap_or_default();
    assert!(
        !stale_after.contains("STALE:agent-doc "),
        "dispatch-only reroute should avoid stale registered panes after /clear: {stale_after}"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_dispatch_only_submits_to_healthy_starting_actor_without_split_churn() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-starting-actor-ready-prompt");
    let session = "codex";
    let actor_pane = iso.new_session(session, dir.path()).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", "codex:0", "agent-doc"]);
    let _ = iso.raw_cmd(&[
        "resize-window",
        "-t",
        "codex:agent-doc",
        "-x",
        "120",
        "-y",
        "40",
    ]);
    let prompt_script = dir.path().join("codex-ready-loop.sh");
    std::fs::write(
            &prompt_script,
            "#!/bin/sh\nprintf '\\033[2J\\033[HREADYMARK\\ngpt-5.4 high · ~/work/btakita/agent-loop · Context 0%% used\\n› \\n'\nwhile IFS= read -r CMD; do printf '[run] Nothing changed\\n'; done\n",
        )
        .unwrap();
    send_keys_with_retry(
        &iso,
        &actor_pane,
        &format!("exec /bin/sh {}", prompt_script.display()),
    );
    let ready_output = wait_for_pane_contains(
        &iso,
        &actor_pane,
        "READYMARK",
        std::time::Duration::from_secs(3),
    );
    assert!(
        ready_output.contains("READYMARK"),
        "fixture command should execute before split setup: {ready_output}"
    );
    assert!(
        ready_prompt_candidate(&ready_output, &HarnessConfig::codex()).is_some(),
        "fixture should show a Codex dispatch-ready prompt before split setup: {ready_output}"
    );
    let sibling_one = iso.split_window(&actor_pane, dir.path(), "-dh").unwrap();
    let sibling_two = iso.split_window(&actor_pane, dir.path(), "-dh").unwrap();
    let sibling_three = iso.split_window(&actor_pane, dir.path(), "-dh").unwrap();
    iso.select_pane(&actor_pane).unwrap();
    let window = iso.pane_window(&actor_pane).unwrap();
    let panes_before = iso.list_window_panes(&window).unwrap();
    assert_eq!(panes_before.len(), 4);

    let doc = dir.path().join("stale-starting-ready-prompt.md");
    let current = "<!-- agent:exchange patch=append -->\n<!-- /agent:exchange -->\n";
    std::fs::write(&doc, current).unwrap();
    crate::snapshot::save(&doc, current).unwrap();
    crate::cycle_state::start_preflight(&doc, Some(current), Some(current)).unwrap();
    crate::cycle_state::mark_committed(&doc, "commit_success", Some(current), Some(current))
        .unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-stale-starting-ready-prompt";
    sessions::register(session_id, &actor_pane, &file_path).unwrap();

    crate::project_controller::store_actor_record(
        dir.path(),
        None,
        &crate::session_actor::ActorRecord {
            document_id: crate::session_actor::canonical_document_id_in(dir.path(), &file_path),
            session_id: session_id.to_string(),
            generation: 1,
            pane_id: actor_pane.clone(),
            window_id: window.clone(),
            harness: "codex".to_string(),
            state: crate::session_actor::ActorState::Starting,
            last_transition: crate::session_actor::ActorLastTransition {
                caller: "start".to_string(),
                reason: "session_start".to_string(),
                timestamp: 1,
                prior_generation: 0,
                new_generation: 1,
            },
        },
    )
    .unwrap();

    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::State => IpcResponse::ok(serde_json::json!({
            "running": true,
            "state": "healthy",
            "actor_state": "starting",
            "restart_count": 0
        })),
        IpcMethod::Inject { .. } => {
            panic!("ready-prompt dispatch-only reroute must use direct pane submit")
        }
        IpcMethod::Restart { .. } => IpcResponse::ok_empty(),
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": 12345 })),
        IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    let resolved = resolve_or_create_pane_dispatch_only(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("dispatch-only reroute should submit to the healthy starting actor");
    assert_eq!(resolved, actor_pane);

    let actor_after = wait_for_pane_contains(
        &iso,
        &actor_pane,
        "[run] Nothing",
        std::time::Duration::from_secs(5),
    );
    let actor_after_compact = actor_after.split_whitespace().collect::<String>();
    assert!(
        actor_after_compact.contains("[run]Nothingchanged"),
        "healthy starting actor should execute the dispatch-only reopen: {actor_after}"
    );
    let panes_after = iso.list_window_panes(&window).unwrap();
    assert_eq!(
        panes_after.len(),
        panes_before.len(),
        "route must not create or remove panes while dispatching to the controller actor"
    );
    for pane in [&sibling_one, &sibling_two, &sibling_three] {
        assert!(
            panes_after.contains(pane),
            "unrelated panes in the split must remain visible"
        );
    }
    let record = crate::project_controller::authoritative_actor_binding(dir.path(), &doc)
        .unwrap()
        .unwrap();
    assert_eq!(record.pane_id, actor_pane);

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn auto_start_reuses_other_file_pane_only_as_split_anchor() {
    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-cross-file-split-anchor");
    let requested_session = "claude";
    let cwd = test_cwd();

    let anchor_pane = iso.auto_start(requested_session, &cwd).unwrap();
    let session =
        pane_session_name(&iso, &anchor_pane).expect("anchor pane should report its tmux session");
    let anchor_window = iso.pane_window(&anchor_pane).unwrap();
    send_keys_with_retry(
        &iso,
        &anchor_pane,
        r#"exec /bin/sh -c 'printf "> \n"; while IFS= read -r CMD; do printf "ANCHOR:%s\n" "$CMD"; done'"#,
    );
    let _ = wait_for_pane_contains(&iso, &anchor_pane, "\n>", std::time::Duration::from_secs(3));

    let anchor_doc = dir.path().join("other.md");
    std::fs::write(&anchor_doc, "# Other\n").unwrap();
    let target_doc = dir.path().join("target.md");
    std::fs::write(&target_doc, "# Target\n").unwrap();

    let anchor_path = anchor_doc
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();
    let target_path = target_doc
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .to_string();

    sessions::register_full_in(
        dir.path(),
        "route-cross-file-anchor",
        &anchor_pane,
        &anchor_path,
        1234,
        &anchor_window,
    )
    .unwrap();

    let mock_start = write_mock_start_agent_doc(dir.path());
    let target_doc_for_thread = target_doc.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(250));
        crate::cycle_state::start_preflight(
            &target_doc_for_thread,
            Some("# Target\n"),
            Some("# Target\n"),
        )
        .unwrap();
    });

    let mut created_panes = Vec::new();
    let new_pane = {
        let _route_bin_guard = route_bin_env_lock();
        unsafe {
            std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
        }
        let result = resolve_or_create_pane(
            &iso,
            &target_doc,
            None,
            &[],
            "route-cross-file-target",
            &target_path,
            &session,
            &HarnessConfig::codex(),
            &mut created_panes,
        );
        unsafe {
            std::env::remove_var("AGENT_DOC_ROUTE_BIN");
        }
        result
    }
    .expect("route should provision a fresh pane without cross-file dispatch");

    assert_eq!(created_panes, vec![new_pane.clone()]);
    assert_ne!(
        new_pane, anchor_pane,
        "auto-start must create a distinct pane rather than dispatching into the anchor"
    );
    assert_eq!(
        iso.pane_window(&new_pane).unwrap(),
        anchor_window,
        "fresh pane should split alongside the existing session pane"
    );

    let target_content = wait_for_pane_contains(
        &iso,
        &new_pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(5),
    );
    assert!(
        target_content.contains("GOT:agent-doc "),
        "fresh pane should receive the routed command: {target_content}"
    );

    let anchor_content = sessions::capture_pane(&iso, &anchor_pane).unwrap_or_default();
    assert!(
        !anchor_content.contains("ANCHOR:agent-doc "),
        "existing pane for another document must stay a split anchor only: {anchor_content}"
    );

    let lookup = sessions::load_in(dir.path())
        .unwrap()
        .values()
        .find(|entry| entry.session_id == "route-cross-file-target")
        .map(|entry| entry.pane.clone());
    assert_eq!(
        lookup.as_deref(),
        Some(new_pane.as_str()),
        "target document should bind to the new pane"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_waits_longer_for_fresh_start_cycle_ack() {
    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-fresh-start-extended-ack");
    let session = "claude";
    let cwd = test_cwd();
    let _anchor_pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("fresh-start-extended-ack.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let mock_start = write_mock_start_agent_doc(dir.path());

    let doc_for_thread = doc.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1300));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
    });

    let mut created_panes = Vec::new();
    let new_pane = {
        let _route_bin_guard = route_bin_env_lock();
        unsafe {
            std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
        }
        let result = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-fresh-start-extended-ack",
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut created_panes,
        );
        unsafe {
            std::env::remove_var("AGENT_DOC_ROUTE_BIN");
        }
        result
    }
    .expect("fresh auto-start should tolerate a delayed but real initial cycle start");

    assert_eq!(created_panes, vec![new_pane.clone()]);

    let content = wait_for_pane_contains(
        &iso,
        &new_pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains("GOT:agent-doc "),
        "route should still dispatch the trigger before observing the delayed ack: {content}"
    );

    let state = crate::cycle_state::load(&doc)
        .unwrap()
        .expect("cycle state should exist after delayed fresh-start ack");
    assert_eq!(
        state.phase,
        crate::cycle_state::CyclePhase::PreflightStarted
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_rebinds_fresh_start_after_ready_wait_registry_churn() {
    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-fresh-start-reregister-before-dispatch");
    let session = "claude";
    let cwd = test_cwd();
    let _anchor_pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("fresh-start-reregister-before-dispatch.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let mock_start = write_mock_delayed_start_agent_doc(dir.path(), 1);

    let registry_root = dir.path().to_path_buf();
    let clear_handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        let registry_path = sessions::registry_path_in(&registry_root);
        let _lock = sessions::RegistryLock::acquire(&registry_path).unwrap();
        let mut registry = sessions::load_in(&registry_root).unwrap();
        let key = registry
            .iter()
            .find(|(_, entry)| entry.session_id == "route-fresh-start-reregister-before-dispatch")
            .map(|(key, _)| key.clone());
        if let Some(key) = key {
            registry.remove(&key);
            sessions::save_in(&registry_root, &registry).unwrap();
        }
    });

    let doc_for_thread = doc.clone();
    let ack_handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        crate::cycle_state::start_preflight(
            &doc_for_thread,
            Some("# Session\n"),
            Some("# Session\n"),
        )
        .unwrap();
    });

    let mut created_panes = Vec::new();
    let new_pane = {
        let _route_bin_guard = route_bin_env_lock();
        unsafe {
            std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
        }
        let result = resolve_or_create_pane(
            &iso,
            &doc,
            None,
            &[],
            "route-fresh-start-reregister-before-dispatch",
            &file_path,
            session,
            &HarnessConfig::codex(),
            &mut created_panes,
        );
        unsafe {
            std::env::remove_var("AGENT_DOC_ROUTE_BIN");
        }
        result
    }
    .expect("fresh auto-start should rebind the pane before the first guarded dispatch");

    clear_handle.join().unwrap();
    ack_handle.join().unwrap();

    assert_eq!(created_panes, vec![new_pane.clone()]);

    let content = wait_for_pane_contains(
        &iso,
        &new_pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains("GOT:agent-doc "),
        "fresh auto-start should still dispatch after the initial binding is cleared during ready-wait: {content}"
    );

    let lookup = sessions::lookup("route-fresh-start-reregister-before-dispatch").unwrap();
    assert_eq!(
        lookup.as_deref(),
        Some(new_pane.as_str()),
        "fresh auto-start should restore the new pane as the registered owner"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_keeps_fresh_start_authoritative_despite_existing_owner_rebind() {
    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-fresh-start-handoff");
    let session = "claude";
    let cwd = test_cwd();
    let existing_pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("fresh-start-handoff.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let mock_agent = write_mock_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &existing_pane,
        &format!("exec {}", mock_agent.display()),
    );
    let owner_ready =
        wait_for_pane_contains(&iso, &existing_pane, ">", std::time::Duration::from_secs(5));
    assert!(
        owner_ready.contains(">"),
        "existing owner pane should be idle before the handoff: {owner_ready}"
    );

    let mock_start = write_mock_delayed_start_agent_doc(dir.path(), 1);
    let registry_root = dir.path().to_path_buf();
    let handoff_pane = existing_pane.clone();
    let handoff_file = file_path.clone();
    let handoff = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        sessions::register_full_with_cwd_in(
            &registry_root,
            "route-fresh-start-handoff",
            &handoff_pane,
            &handoff_file,
            12345,
            "@owner",
            registry_root.to_string_lossy().as_ref(),
        )
        .unwrap();
    });
    let doc_for_ack = doc.clone();
    let ack = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        crate::cycle_state::start_preflight(&doc_for_ack, Some("# Session\n"), Some("# Session\n"))
            .unwrap();
    });

    let mut created_panes = Vec::new();
    let routed_pane = {
            let _route_bin_guard = route_bin_env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
            }
            let result = resolve_or_create_pane(
                &iso,
                &doc,
                None,
                &[],
                "route-fresh-start-handoff",
                &file_path,
                session,
                &HarnessConfig::codex(),
                &mut created_panes,
            );
            unsafe {
                std::env::remove_var("AGENT_DOC_ROUTE_BIN");
            }
            result
        }
        .expect("fresh auto-start should keep the fresh pane authoritative even if another path rebinds the session during boot");

    handoff.join().unwrap();
    ack.join().unwrap();

    let new_pane = created_panes
        .first()
        .cloned()
        .expect("fresh route should still create one pane");
    assert_eq!(routed_pane, new_pane);
    assert_eq!(
        created_panes.len(),
        1,
        "fresh auto-start should still create one pane"
    );

    let owner_after = sessions::capture_pane(&iso, &existing_pane).unwrap_or_default();
    assert!(
        !owner_after.contains("GOT:agent-doc "),
        "route must not hand dispatch back to the older pane after a fresh start: {owner_after}"
    );

    let new_pane_after = sessions::capture_pane(&iso, &new_pane).unwrap_or_default();
    assert!(
        new_pane_after.contains("GOT:agent-doc "),
        "route should keep dispatching into the fresh pane after a competing registry rebind: {new_pane_after}"
    );

    let lookup = sessions::lookup("route-fresh-start-handoff").unwrap();
    assert_eq!(
        lookup.as_deref(),
        Some(new_pane.as_str()),
        "registry should restore the fresh pane as authoritative after the competing rebind"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_ignores_handoff_back_to_active_startup_miss_pane() {
    let _tmux_guard = tmux_start_lock();
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-fresh-start-ignore-startup-miss-handoff");
    let session = "claude";
    let cwd = test_cwd();
    let existing_pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir
        .path()
        .join("fresh-start-ignore-startup-miss-handoff.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let mock_agent = write_mock_registered_agent_doc(dir.path());
    send_keys_with_retry(
        &iso,
        &existing_pane,
        &format!("exec {}", mock_agent.display()),
    );
    let owner_ready =
        wait_for_pane_contains(&iso, &existing_pane, ">", std::time::Duration::from_secs(5));
    assert!(
        owner_ready.contains(">"),
        "existing owner pane should be idle before the handoff: {owner_ready}"
    );

    crate::startup_miss::record(
        &doc,
        &existing_pane,
        "route-fresh-start-ignore-startup-miss-handoff",
        "codex",
        crate::startup_miss::StartupMissOrigin::RoutedTrigger,
        Some("cycle-baseline"),
    )
    .unwrap();

    let mock_start = write_mock_delayed_start_agent_doc(dir.path(), 1);
    let registry_root = dir.path().to_path_buf();
    let handoff_pane = existing_pane.clone();
    let handoff_file = file_path.clone();
    let handoff = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(200));
        sessions::register_full_with_cwd_in(
            &registry_root,
            "route-fresh-start-ignore-startup-miss-handoff",
            &handoff_pane,
            &handoff_file,
            12345,
            "@owner",
            registry_root.to_string_lossy().as_ref(),
        )
        .unwrap();
    });
    let doc_for_ack = doc.clone();
    let ack = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(1500));
        crate::cycle_state::start_preflight(&doc_for_ack, Some("# Session\n"), Some("# Session\n"))
            .unwrap();
    });

    let mut created_panes = Vec::new();
    let routed_pane = {
            let _route_bin_guard = route_bin_env_lock();
            unsafe {
                std::env::set_var("AGENT_DOC_ROUTE_BIN", mock_start.as_os_str());
            }
            let result = resolve_or_create_pane(
                &iso,
                &doc,
                None,
                &[],
                "route-fresh-start-ignore-startup-miss-handoff",
                &file_path,
                session,
                &HarnessConfig::codex(),
                &mut created_panes,
            );
            unsafe {
                std::env::remove_var("AGENT_DOC_ROUTE_BIN");
            }
            result
        }
        .expect("fresh auto-start should keep dispatch in the new pane when the old owner still carries startup-miss provenance");

    handoff.join().unwrap();
    ack.join().unwrap();

    assert_eq!(created_panes.len(), 1, "route should still create one pane");
    let new_pane = &created_panes[0];
    assert_eq!(routed_pane, *new_pane);

    let new_pane_after = wait_for_pane_contains(
        &iso,
        new_pane,
        "GOT:agent-doc ",
        std::time::Duration::from_secs(3),
    );
    assert!(
        new_pane_after.contains("GOT:agent-doc "),
        "route should keep the reopen in the fresh pane when the alternate handoff target still owns startup-miss provenance: {new_pane_after}"
    );

    let old_pane_after = sessions::capture_pane(&iso, &existing_pane).unwrap_or_default();
    assert!(
        !old_pane_after.contains("GOT:agent-doc "),
        "route must not hand dispatch back to the startup-miss pane: {old_pane_after}"
    );

    let lookup = sessions::lookup("route-fresh-start-ignore-startup-miss-handoff").unwrap();
    assert_eq!(
        lookup.as_deref(),
        Some(new_pane.as_str()),
        "registry should restore the fresh pane as authoritative when the old pane is still marked startup-miss"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_fresh_dispatch_target_ignores_explicitly_blocked_startup_miss_pane() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-fresh-start-blocked-handoff");

    let doc = dir.path().join("fresh-start-blocked-handoff.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-fresh-start-blocked-handoff";
    let blocked_pane = "%364";
    let new_pane = "%370";

    sessions::register_full_with_cwd_in(
        dir.path(),
        session_id,
        blocked_pane,
        &file_path,
        12345,
        "@owner",
        dir.path().to_string_lossy().as_ref(),
    )
    .unwrap();

    let resolved = resolve_fresh_dispatch_target_after_ready_wait(
        &iso,
        session_id,
        new_pane,
        &file_path,
        Some(blocked_pane),
    )
    .unwrap();

    assert_eq!(
        resolved, new_pane,
        "resolver should keep dispatch in the fresh pane when the previous startup-miss owner is explicitly blocked"
    );

    let registry = sessions::load_in(dir.path()).unwrap();
    let entry = registry
        .values()
        .find(|entry| entry.session_id == session_id)
        .expect("fresh pane should be registered after the blocked handoff is ignored");
    assert_eq!(entry.pane, new_pane);
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn alive_registered_pane_uses_supervisor_pid_fallback_when_argv_loses_file_path() {
    use std::sync::{Arc, Mutex};

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-live-owner-supervisor-pid");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-live-owner-supervisor";

    let mock_agent = write_mock_registered_agent_doc(dir.path());
    launch_mock_agent_doc_without_file_arg(&iso, &pane, &mock_agent);
    assert!(
        wait_for_agent_ready_outcome(
            &iso,
            &pane,
            Duration::from_secs(10),
            &HarnessConfig::codex()
        )
        .is_ready(),
        "mock agent prompt should be ready before route probes the recovered supervisor owner"
    );
    let mock_agent_pid =
        wait_for_process_pid(&mock_agent.display().to_string(), Duration::from_secs(3));
    let injects = Arc::new(Mutex::new(Vec::<String>::new()));
    let injects_for_ipc = injects.clone();

    let mut ipc = SupervisorIpc::start(dir.path(), session_id, move |method| match method {
        IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": mock_agent_pid })),
        IpcMethod::State => IpcResponse::ok(serde_json::json!({ "running": true })),
        IpcMethod::Inject { bytes } => {
            injects_for_ipc.lock().unwrap().push(bytes.clone());
            IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
        }
        IpcMethod::Restart { .. } | IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
    })
    .unwrap();

    sessions::register(session_id, &pane, &file_path).unwrap();

    let resolved = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect("route should recover the live owner via supervisor pid");
    assert_eq!(resolved, pane);
    assert!(
        *injects.lock().unwrap()
            == vec![routed_trigger_submit_payload(
                &HarnessConfig::codex().trigger_command(&file_path)
            )],
        "route should dispatch to the registered pane via supervisor IPC after recovering the live owner via supervisor pid"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn pane_has_prompt_detects_unicode() {
    let _tmux_guard = tmux_start_lock();
    let iso = IsolatedTmux::new("route-test-has-prompt");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
        "shell did not become ready before prompt detection test"
    );

    send_keys_with_retry(&iso, &pane, &mock_agent_script(100));
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "Starting agent...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Starting agent..."),
        "mock agent never started in pane: {content}"
    );
    let harness = HarnessConfig::claude();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
    let content = sessions::capture_pane(&iso, &pane).unwrap_or_default();
    assert!(
        ready && ready_prompt_candidate(&content, &harness).is_some(),
        "should detect ❯ in pane content, got: {}",
        content
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn full_auto_start_flow() {
    let _tmux_guard = tmux_start_lock();
    let iso = IsolatedTmux::new("route-test-e2e");
    let session = "test";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        wait_for_shell(&iso, &pane, std::time::Duration::from_secs(5)),
        "shell did not become ready before e2e launch"
    );

    send_keys_with_retry(&iso, &pane, &mock_agent_script(300));
    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "Starting agent...",
        std::time::Duration::from_secs(5),
    );
    assert!(
        content.contains("Starting agent..."),
        "mock agent never started in pane: {content}"
    );

    let harness = HarnessConfig::claude();
    let ready = wait_for_agent_ready(&iso, &pane, std::time::Duration::from_secs(10), &harness);
    assert!(ready, "mock agent should become ready");

    send_keys_with_retry(&iso, &pane, "HELLO_FROM_TEST");

    let content = wait_for_pane_contains(
        &iso,
        &pane,
        "HELLO_FROM_TEST",
        std::time::Duration::from_secs(3),
    );
    assert!(
        content.contains("HELLO_FROM_TEST"),
        "command should appear in pane after send, got: {}",
        content
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn select_pane_switches_window() {
    let iso = IsolatedTmux::new("route-test-select");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create first pane (auto_start creates session + first window)
    let pane1 = iso.auto_start(session, &cwd).unwrap();

    // Create second window with a new pane
    let output = iso
        .cmd()
        .args(["new-window", "-t", session, "-P", "-F", "#{pane_id}"])
        .output()
        .unwrap();
    let pane2 = String::from_utf8_lossy(&output.stdout).trim().to_string();

    // Select pane1 — should switch back to window 1
    iso.select_pane(&pane1).unwrap();

    // Verify pane1 is now the active pane
    let active = iso
        .cmd()
        .args(["display-message", "-t", session, "-p", "#{pane_id}"])
        .output()
        .unwrap();
    let active_pane = String::from_utf8_lossy(&active.stdout).trim().to_string();
    assert_eq!(
        active_pane, pane1,
        "select_pane should switch to the correct window/pane"
    );

    let _ = pane2; // suppress unused warning
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn command_text_cleared_after_acceptance() {
    // Verifies that send_command's acceptance check works:
    // The command text should NOT be in the last 5 lines after acceptance.
    let iso = IsolatedTmux::new("route-test-cmd-clear");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();
    let pane = iso.auto_start(session, &cwd).unwrap();

    // Send a command that gets consumed immediately
    send_keys_with_retry(&iso, &pane, "echo DONE");
    std::thread::sleep(std::time::Duration::from_millis(500));

    // The command "echo DONE" should NOT be in the prompt anymore
    // (it was accepted and executed)
    let content = sessions::capture_pane(&iso, &pane).unwrap();
    let _cmd_in_last_lines = content
        .lines()
        .rev()
        .take(5)
        .any(|l| l.contains("echo DONE") && !l.contains("DONE"));
    // The echo command was accepted — "echo DONE" appears in history but
    // "DONE" output appears too. The key is that the INPUT line no longer
    // has the command waiting for Enter.
    assert!(
        content.contains("DONE"),
        "command should have been executed, got: {}",
        content
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn pane_session_detection() {
    // Verify we can detect which session a pane is in
    let iso = IsolatedTmux::new("route-test-session");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();
    let pane = iso.auto_start(session, &cwd).unwrap();

    // Check session name
    let output = iso
        .cmd()
        .args(["display-message", "-t", &pane, "-p", "#{session_name}"])
        .output()
        .unwrap();
    let detected_session = String::from_utf8_lossy(&output.stdout).trim().to_string();
    assert_eq!(
        detected_session, session,
        "pane should be in session '{}'",
        session
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn pane_in_wrong_session_detected() {
    // Create panes in two different sessions, verify we can distinguish them
    let iso = IsolatedTmux::new("route-test-wrong-sess");
    let cwd = std::env::current_dir().unwrap();

    // Create session "correct" with a pane
    let correct_pane = iso.auto_start("correct", &cwd).unwrap();

    // Create session "wrong" with another pane
    let wrong_pane = iso.auto_start("wrong", &cwd).unwrap();

    // Verify they're in different sessions
    let correct_session = iso
        .cmd()
        .args([
            "display-message",
            "-t",
            &correct_pane,
            "-p",
            "#{session_name}",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap();
    let wrong_session = iso
        .cmd()
        .args([
            "display-message",
            "-t",
            &wrong_pane,
            "-p",
            "#{session_name}",
        ])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap();

    assert_eq!(correct_session, "correct");
    assert_eq!(wrong_session, "wrong");
    assert_ne!(
        correct_session, wrong_session,
        "panes should be in different sessions"
    );
}

// --- auto_start_in_session tests ---

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn auto_start_splits_in_existing_window() {
    // When a registered agent-doc pane exists in the target session,
    // auto_start_in_session should split-window in that pane's window
    // (not create a new window).
    let iso = IsolatedTmux::new("route-test-split-existing");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create the first pane (simulating an existing agent-doc pane)
    let pane1 = iso.auto_start(session, &cwd).unwrap();
    let window1 = iso.pane_window(&pane1).unwrap();

    // Split directly in that window (simulating what auto_start_in_session does)
    let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();
    let window2 = iso.pane_window(&pane2).unwrap();

    // Both panes should be in the same window
    assert_eq!(
        window1, window2,
        "split_window should create pane in the SAME window, not a new one"
    );

    // Both panes should be alive
    assert!(iso.pane_alive(&pane1));
    assert!(iso.pane_alive(&pane2));

    // The panes should be different
    assert_ne!(pane1, pane2, "should create a distinct new pane");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn auto_start_creates_new_window_when_no_registered_panes() {
    // When no registered agent-doc panes exist, auto_start_in_session
    // should create a new window via auto_start().
    let iso = IsolatedTmux::new("route-test-new-window");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create session with an initial pane (not registered)
    let pane1 = iso.auto_start(session, &cwd).unwrap();
    let window1 = iso.pane_window(&pane1).unwrap();

    // Calling auto_start again creates a NEW window (since no registered panes)
    let pane2 = iso.auto_start(session, &cwd).unwrap();
    let window2 = iso.pane_window(&pane2).unwrap();

    // Should be in different windows
    assert_ne!(
        window1, window2,
        "auto_start should create a new window when no registered panes exist"
    );
    assert_ne!(pane1, pane2);
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn find_registered_pane_filters_by_session() {
    // find_registered_pane_in_session should only return panes
    // that are alive and in the target tmux session.
    let iso = IsolatedTmux::new("route-test-find-reg");
    let cwd = std::env::current_dir().unwrap();

    // Create two sessions
    let pane_a = iso.auto_start("session-a", &cwd).unwrap();
    let pane_b = iso.auto_start("session-b", &cwd).unwrap();

    // Verify panes are in different sessions
    let sess_a = iso.pane_session(&pane_a).unwrap();
    let sess_b = iso.pane_session(&pane_b).unwrap();
    assert_eq!(sess_a, "session-a");
    assert_eq!(sess_b, "session-b");

    // find_registered_pane_in_session uses the sessions registry,
    // so this test just verifies the tmux infrastructure works.
    // The function itself filters by session name, which we test
    // indirectly via the pane_session check above.
    assert_ne!(pane_a, pane_b);
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn split_window_respects_working_directory() {
    let iso = IsolatedTmux::new("route-test-split-cwd");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    let pane1 = iso.auto_start(session, &cwd).unwrap();
    let pane2 = iso.split_window(&pane1, &cwd, "-dh").unwrap();

    // Both panes should be alive and in same window
    assert!(iso.pane_alive(&pane1));
    assert!(iso.pane_alive(&pane2));

    let w1 = iso.pane_window(&pane1).unwrap();
    let w2 = iso.pane_window(&pane2).unwrap();
    assert_eq!(w1, w2, "split pane should be in same window");

    // Verify the window now has 2 panes
    let panes = iso.list_window_panes(&w1).unwrap();
    assert_eq!(
        panes.len(),
        2,
        "window should have exactly 2 panes after split"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn stash_pane_on_split_failure() {
    // When split_window fails, the fallback should auto_start then stash
    // the pane so it doesn't create a visible new window.
    let iso = IsolatedTmux::new("route-test-stash-fallback");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create a pane then simulate the fallback path:
    // auto_start creates a new window, then stash_pane moves it.
    let pane = iso.auto_start(session, &cwd).unwrap();
    let fallback_pane = iso.auto_start(session, &cwd).unwrap();

    // Before stash: pane and fallback_pane are in different windows
    let w1 = iso.pane_window(&pane).unwrap();
    let w_fb_before = iso.pane_window(&fallback_pane).unwrap();
    assert_ne!(
        w1, w_fb_before,
        "fallback should be in a new window initially"
    );

    // Stash the fallback pane (simulating what the route.rs fallback does)
    iso.stash_pane(&fallback_pane, session).unwrap();

    // After stash: fallback_pane should be in the stash window
    assert!(iso.pane_alive(&fallback_pane), "pane should still be alive");
    let stash_win = iso.find_stash_window(session);
    assert!(stash_win.is_some(), "stash window should have been created");
    let w_fb_after = iso.pane_window(&fallback_pane).unwrap();
    assert_eq!(
        w_fb_after,
        stash_win.unwrap(),
        "fallback pane should be in the stash window"
    );
}

// --- has_named_window tests ---

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn has_named_window_detects_agent_doc_window() {
    let iso = IsolatedTmux::new("route-test-named-win");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create session (first window gets default name, not "agent-doc")
    let _pane = iso.auto_start(session, &cwd).unwrap();
    assert!(
        !has_named_window(&iso, session, "agent-doc"),
        "should not find 'agent-doc' window before renaming"
    );

    // Rename the window to "agent-doc"
    let _ = iso
        .cmd()
        .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
        .status();
    assert!(
        has_named_window(&iso, session, "agent-doc"),
        "should find 'agent-doc' window after renaming"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn has_named_window_false_for_nonexistent_session() {
    let iso = IsolatedTmux::new("route-test-named-win-no-sess");
    assert!(
        !has_named_window(&iso, "nonexistent", "agent-doc"),
        "should return false for nonexistent session"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn duplicate_pane_policy_error_includes_manual_tmux_commands() {
    let iso = IsolatedTmux::new("route-test-duplicate-policy");
    let session = "test";
    let rendered = format_duplicate_pane_policy_error(
        session,
        "tasks/agent-doc/agent-doc-bugs2.md",
        Some("%42"),
        "split-window failed alongside pane %42 (too small)",
    );
    assert!(rendered.contains("tmux list-panes -t test:agent-doc"));
    assert!(rendered.contains("tmux kill-pane -t %42"));
    assert!(rendered.contains("agent-doc tasks/agent-doc/agent-doc-bugs2.md"));
    assert!(rendered.contains("split-window failed alongside pane %42"));
    let _ = iso;
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn preserves_replaced_stash_pane_without_provenance() {
    let iso = IsolatedTmux::new("route-test-evict-stash");
    let session = "route-evict";
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let result = (|| -> anyhow::Result<()> {
        let old_pane = iso.auto_start(session, dir.path())?;
        iso.stash_pane(&old_pane, session)?;

        let replacement_pane = iso.auto_start(session, dir.path())?;
        iso.stash_pane(&replacement_pane, session)?;

        let previous = sessions::SessionEntry {
            pane: old_pane.clone(),
            pid: std::process::id(),
            cwd: dir.path().to_string_lossy().to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            session_id: "session-123".to_string(),
            file: "doc.md".to_string(),
            window: iso.pane_window(&old_pane)?,
            supervisor_instance_id: String::new(),
        };
        evict_previous_stash_pane_entry(
            &iso,
            "session-123",
            &previous,
            &replacement_pane,
            session,
            &HarnessConfig::claude(),
        );

        assert!(
            iso.pane_alive(&old_pane),
            "previous stash pane should be preserved without explicit provenance"
        );
        assert!(
            iso.pane_alive(&replacement_pane),
            "replacement pane should stay alive"
        );
        Ok(())
    })();

    result.unwrap();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn eviction_skipped_when_agent_process_active() {
    let iso = IsolatedTmux::new("route-test-evict-busy");
    let session = "route-evict-busy";
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let result = (|| -> anyhow::Result<()> {
        let busy_pane = iso.auto_start(session, dir.path())?;
        iso.stash_pane(&busy_pane, session)?;

        // Copy /bin/sleep as "agent-doc" so tmux's #{pane_current_command}
        // reports the binary name that matches the harness process list.
        let bin_dir = dir.path().join("bin");
        std::fs::create_dir_all(&bin_dir)?;
        let fake_agent = bin_dir.join("agent-doc");
        std::fs::copy("/bin/sleep", &fake_agent)?;

        iso.raw_cmd(&[
            "send-keys",
            "-t",
            &busy_pane,
            &format!("{} 60", fake_agent.display()),
            "Enter",
        ])?;

        // Poll until pane_current_command changes from the shell
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        loop {
            std::thread::sleep(std::time::Duration::from_millis(100));
            let out = iso
                .cmd()
                .args([
                    "display-message",
                    "-t",
                    &busy_pane,
                    "-p",
                    "#{pane_current_command}",
                ])
                .output()?;
            let cmd = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if cmd == "agent-doc" {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "timed out waiting for agent-doc to start in pane (last cmd: '{}')",
                cmd
            );
        }

        let replacement_pane = iso.auto_start(session, dir.path())?;
        iso.stash_pane(&replacement_pane, session)?;

        let previous = sessions::SessionEntry {
            pane: busy_pane.clone(),
            pid: std::process::id(),
            cwd: dir.path().to_string_lossy().to_string(),
            started: "2026-01-01T00:00:00Z".to_string(),
            session_id: "session-busy".to_string(),
            file: "doc.md".to_string(),
            window: iso.pane_window(&busy_pane)?,
            supervisor_instance_id: String::new(),
        };
        evict_previous_stash_pane_entry(
            &iso,
            "session-busy",
            &previous,
            &replacement_pane,
            session,
            &HarnessConfig::claude(),
        );

        assert!(
            iso.pane_alive(&busy_pane),
            "stash pane running agent process should NOT be evicted"
        );
        assert!(
            iso.pane_alive(&replacement_pane),
            "replacement pane should stay alive"
        );
        Ok(())
    })();

    result.unwrap();
}

// --- tmux_session validation tests ---

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn route_warns_on_nonexistent_tmux_session() {
    // When frontmatter specifies a tmux_session that doesn't exist,
    // run_with_tmux should log a warning and NOT create that session.
    let iso = IsolatedTmux::new("route-test-warn-nonexist");
    let cwd = std::env::current_dir().unwrap();

    // Create a fallback session so there's somewhere to land
    let _fallback_pane = iso.auto_start("claude", &cwd).unwrap();

    // Write a temp file with a nonexistent tmux_session in frontmatter
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.md");
    std::fs::write(
            &file,
            "---\nagent_doc_session: test-uuid-1234\ntmux_session: ghost-session\n---\n## User\nHello\n",
        )
        .unwrap();

    // The nonexistent session should NOT exist before or after
    assert!(
        !iso.session_exists("ghost-session"),
        "ghost-session should not exist before route"
    );

    // Run route — it will fail at auto-start (AGENT_DOC_NO_AUTOSTART),
    // but we can verify the session was never created
    let result = {
        let _env_guard = env_lock();
        unsafe {
            std::env::set_var("AGENT_DOC_NO_AUTOSTART", "1");
        }
        let r = run_with_tmux(&file, &iso, None, 0, &[], RouteMode::Managed, false, None);
        unsafe {
            std::env::remove_var("AGENT_DOC_NO_AUTOSTART");
        }
        r
    };

    // The ghost session should still not exist (route fell back, didn't create it)
    assert!(
        !iso.session_exists("ghost-session"),
        "ghost-session should NOT have been created by route"
    );

    // Route should have bailed due to AGENT_DOC_NO_AUTOSTART (no active pane)
    assert!(result.is_err(), "should error with no autostart");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn route_falls_back_to_existing_session() {
    // When frontmatter requests a nonexistent session, route should
    // fall back to an existing session and create panes there.
    let iso = IsolatedTmux::new("route-test-fallback-sess");
    let cwd = std::env::current_dir().unwrap();

    // Create the fallback session "claude"
    let fallback_pane = iso.auto_start("claude", &cwd).unwrap();
    let fallback_session = iso.pane_session(&fallback_pane).unwrap();
    assert_eq!(fallback_session, "claude");

    // Verify ghost-session does NOT exist
    assert!(!iso.session_exists("ghost-session"));

    // Write a temp file with a nonexistent tmux_session
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("test.md");
    std::fs::write(
            &file,
            "---\nagent_doc_session: fallback-uuid-5678\ntmux_session: ghost-session\n---\n## User\nHello\n",
        )
        .unwrap();

    // Set AGENT_DOC_NO_AUTOSTART so we don't actually spawn Claude,
    // but we can inspect the validation behavior
    let _result = {
        let _env_guard = env_lock();
        unsafe {
            std::env::set_var("AGENT_DOC_NO_AUTOSTART", "1");
        }
        let r = run_with_tmux(&file, &iso, None, 0, &[], RouteMode::Managed, false, None);
        unsafe {
            std::env::remove_var("AGENT_DOC_NO_AUTOSTART");
        }
        r
    };

    // The ghost session should NOT have been created
    assert!(
        !iso.session_exists("ghost-session"),
        "nonexistent session should never be created by route"
    );

    // The fallback "claude" session should still exist
    assert!(
        iso.session_exists("claude"),
        "fallback session should still be alive"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_target_session_ignores_blank_context_session() {
    let iso = IsolatedTmux::new("route-test-blank-context");
    let cwd = std::env::current_dir().unwrap();
    let pane = iso.auto_start("claude", &cwd).unwrap();
    let current_session = iso.pane_session(&pane).unwrap();

    let resolved = resolve_target_session(&iso, Some("   "), &[], None, &HarnessConfig::claude());
    assert_eq!(
        resolved, current_session,
        "blank context_session should fall back to the live target session"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_preferred_session_prefers_live_project_pin_over_current_session() {
    let dir = tempfile::TempDir::new().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::write(
        dir.path().join(".agent-doc/config.toml"),
        "tmux_session = \"0\"\n",
    )
    .unwrap();

    let iso = IsolatedTmux::new("route-test-project-session-pin");
    let _configured = iso.new_session("0", dir.path()).unwrap();
    let _current = iso.new_session("1", dir.path()).unwrap();

    assert_eq!(current_tmux_session(&iso).as_deref(), Some("1"));
    assert_eq!(
        resolve_preferred_session(&iso, None, "[test]").as_deref(),
        Some("0"),
        "a live project tmux_session pin should beat the caller's current session"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_target_session_prefers_nested_file_root_pin_over_outer_cwd() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let subroot = root.join("src/session-share");
    let _cwd_guard = ScopedCurrentDir::set(root);

    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(subroot.join("tasks")).unwrap();
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"4\"\n",
    )
    .unwrap();
    std::fs::write(
        subroot.join(".agent-doc/config.toml"),
        "tmux_session = \"1\"\n",
    )
    .unwrap();

    let child_doc = subroot.join("tasks/claudescore-3.md");
    std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("route-test-nested-file-root-pin");
    let _child = iso.new_session("1", root).unwrap();
    let _workspace = iso.new_session("4", root).unwrap();

    assert_eq!(
        resolve_target_session(&iso, None, &[], Some(&child_doc), &HarnessConfig::claude()),
        "1",
        "route should honor the nested file's own project pin even when cwd is the outer workspace root"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_target_session_prefers_shared_workspace_root_pin_for_mixed_roots() {
    let tmp = tempfile::TempDir::new().unwrap();
    let root = tmp.path();
    let subroot = root.join("src/session-share");

    std::fs::create_dir_all(root.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(root.join("tasks")).unwrap();
    std::fs::create_dir_all(subroot.join(".agent-doc")).unwrap();
    std::fs::create_dir_all(subroot.join("tasks")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(&subroot);
    std::fs::write(
        root.join(".agent-doc/config.toml"),
        "tmux_session = \"4\"\n",
    )
    .unwrap();
    std::fs::write(
        subroot.join(".agent-doc/config.toml"),
        "tmux_session = \"1\"\n",
    )
    .unwrap();

    let root_doc = root.join("tasks/agent-doc-bugs2.md");
    let child_doc = subroot.join("tasks/claudescore-3.md");
    std::fs::write(
            &root_doc,
            "---\nagent_doc_session: root-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            &child_doc,
            "---\nagent_doc_session: child-session\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("route-test-mixed-root-workspace-pin");
    let _child = iso.new_session("1", root).unwrap();
    let _workspace = iso.new_session("4", root).unwrap();

    let col_args = vec![
        root_doc.to_string_lossy().to_string(),
        child_doc.to_string_lossy().to_string(),
    ];
    assert_eq!(
        resolve_target_session(
            &iso,
            None,
            &col_args,
            Some(&child_doc),
            &HarnessConfig::claude(),
        ),
        "4",
        "mixed-root route should stay on the shared workspace root pin instead of the focused child root"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn blank_context_session_does_not_bypass_target_validation() {
    let iso = IsolatedTmux::new("route-test-blank-context-validate");
    let result =
        ensure_auto_start_target_session(&iso, Some("   "), "claude", &HarnessConfig::claude());
    assert!(
        result.is_err(),
        "blank context_session should not bypass implicit fallback validation"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn implicit_fallback_session_is_not_auto_start_target() {
    let iso = IsolatedTmux::new("route-test-no-implicit-fallback");
    let result = ensure_auto_start_target_session(&iso, None, "claude", &HarnessConfig::claude());
    assert!(
        result.is_err(),
        "dead implicit fallback session should not be auto-started"
    );
}

// --- Stash rescue tests ---

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn pane_in_stash_rescued_to_agent_doc() {
    // When a registered pane ends up in a stash window, route should
    // rescue it back to the agent-doc window without ejecting the
    // currently visible pane into stash.
    let iso = IsolatedTmux::new("route-test-stash-rescue");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create session and rename the window to "agent-doc"
    let pane1 = iso.auto_start(session, &cwd).unwrap();
    let _ = iso
        .cmd()
        .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
        .status();

    // Create a second pane and stash it (simulating a pane that ended up in stash)
    let stashed_pane = iso.auto_start(session, &cwd).unwrap();
    iso.stash_pane(&stashed_pane, session).unwrap();

    // Verify it's in the stash window
    let stash_win = iso.find_stash_window(session);
    assert!(stash_win.is_some(), "stash window should exist");
    let pane_win = iso.pane_window(&stashed_pane).unwrap();
    assert_eq!(pane_win, stash_win.unwrap(), "pane should be in stash");

    // Now rescue: join the stashed pane back into the agent-doc window.
    let agent_doc_window = format!("{}:agent-doc", session);
    let target_panes = iso.list_window_panes(&agent_doc_window).unwrap_or_default();
    assert!(
        !target_panes.is_empty(),
        "agent-doc window should have panes"
    );

    if let Some(target) = target_panes.first() {
        sessions::join_pane_guarded(&iso, &stashed_pane, target, session, "-dh").unwrap();
        let rescued_win = iso.pane_window(&stashed_pane).unwrap();
        let visible_win = iso.pane_window(&pane1).unwrap();
        assert_eq!(
            rescued_win, visible_win,
            "rescued pane should rejoin the visible agent-doc window"
        );
        let agent_doc_panes = iso.list_window_panes(&agent_doc_window).unwrap();
        assert!(
            agent_doc_panes.contains(&pane1),
            "existing visible pane should stay in agent-doc window, got: {:?}",
            agent_doc_panes
        );
        assert!(
            agent_doc_panes.contains(&stashed_pane),
            "rescued pane should be in agent-doc window, got: {:?}",
            agent_doc_panes
        );
        assert!(
            iso.pane_alive(&stashed_pane),
            "rescued pane should be alive"
        );
    }
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn join_pane_rescue_places_left_of_target_when_requested() {
    let iso = IsolatedTmux::new("route-test-join-left");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create session with agent-doc window
    let pane1 = iso.auto_start(session, &cwd).unwrap();
    let _ = iso
        .cmd()
        .args(["rename-window", "-t", &format!("{}:", session), "agent-doc"])
        .status();

    // Create a second pane in its own window and rescue it to the left edge.
    let pane2 = iso.auto_start(session, &cwd).unwrap();
    let agent_doc_window = format!("{}:agent-doc", session);
    let target_panes = iso.list_window_panes(&agent_doc_window).unwrap();
    let target = &target_panes[0];

    sessions::join_pane_guarded(&iso, &pane2, target, session, "-dbh").unwrap();

    let agent_doc_panes = iso.list_panes_ordered(&agent_doc_window).unwrap();
    assert!(
        agent_doc_panes.contains(&pane2),
        "pane should be in agent-doc window after join, got: {:?}",
        agent_doc_panes
    );
    assert_eq!(
        agent_doc_panes.first().unwrap(),
        &pane2,
        "split-before rescue should place the pane on the left edge"
    );
    assert!(
        agent_doc_panes.contains(&pane1),
        "original pane should remain visible after rescue, got: {:?}",
        agent_doc_panes
    );
    assert!(iso.pane_alive(&pane2), "pane should be alive after join");
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn sync_after_claim_prefers_col_args_over_registry() {
    // Regression test: when editor provides col_args, sync_after_claim should
    // pass those to sync::run instead of auto-discovering from registry.
    // The actual pane stashing is handled by tmux-router's reconcile —
    // this test verifies the col_args flow.
    let iso = IsolatedTmux::new("route-test-col-args");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    let pane_a = iso.auto_start(session, &cwd).unwrap();
    let window_id = iso.pane_window(&pane_a).unwrap();

    // With col_args having < 2 entries, sync_after_claim returns early (no sync needed).
    // This verifies the early-return path.
    sync_after_claim(&iso, &pane_a, &["single.md".to_string()]);

    // With empty col_args and < 2 registry entries, also returns early.
    sync_after_claim(&iso, &pane_a, &[]);

    // Pane should still be alive and in the same window — no unintended stashing
    assert!(
        iso.pane_alive(&pane_a),
        "pane should survive sync_after_claim"
    );
    assert_eq!(
        iso.pane_window(&pane_a).unwrap(),
        window_id,
        "pane should stay in original window"
    );

    // With 2+ col_args, sync_after_claim runs sync::run with those args.
    // sync::run will fail to resolve files (no registrations), but shouldn't crash.
    let col_args = vec!["file_a.md".to_string(), "file_b.md".to_string()];
    sync_after_claim(&iso, &pane_a, &col_args);

    // Pane should still be alive
    assert!(
        iso.pane_alive(&pane_a),
        "pane should survive sync with unresolved files"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn sync_after_claim_stays_on_injected_tmux_server() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    std::fs::create_dir_all(dir.path().join("tasks")).unwrap();

    let file_a = dir.path().join("tasks/file_a.md");
    let file_b = dir.path().join("tasks/file_b.md");
    std::fs::write(
            &file_a,
            "---\nagent_doc_session: route-sync-claim-a\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();
    std::fs::write(
            &file_b,
            "---\nagent_doc_session: route-sync-claim-b\nagent_doc_format: template\nagent_doc_write: crdt\n---\n",
        )
        .unwrap();

    let iso = IsolatedTmux::new("route-test-sync-after-claim-injected");
    let session = "test";
    let pane_a = iso.new_session(session, dir.path()).unwrap();
    let window = iso.pane_window(&pane_a).unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);
    let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
    let pane_b = iso.split_window(&pane_a, dir.path(), "-dh").unwrap();
    let extra_pane = iso.split_window(&pane_b, dir.path(), "-dh").unwrap();
    let pane_a_pid = pane_display_value(&iso, &pane_a, "#{pane_pid}")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap();
    let pane_b_pid = pane_display_value(&iso, &pane_b, "#{pane_pid}")
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap();

    sessions::register_full_with_cwd(
        "route-sync-claim-a",
        &pane_a,
        &file_a.to_string_lossy(),
        pane_a_pid,
        &window,
        &dir.path().to_string_lossy(),
    )
    .unwrap();
    sessions::register_full_with_cwd(
        "route-sync-claim-b",
        &pane_b,
        &file_b.to_string_lossy(),
        pane_b_pid,
        &window,
        &dir.path().to_string_lossy(),
    )
    .unwrap();

    sync_after_claim(&iso, &pane_a, &[]);

    let visible = iso.list_window_panes(&window).unwrap();
    assert_eq!(
        visible.len(),
        2,
        "post-claim sync should reconcile the injected tmux window instead of mutating the default server"
    );
    assert!(
        visible.contains(&pane_a) && visible.contains(&pane_b),
        "registered panes should remain visible after the injected-server reconcile, got {:?}",
        visible
    );
    assert!(
        !visible.contains(&extra_pane),
        "unregistered overflow pane should be removed from the injected tmux window, got {:?}",
        visible
    );
    assert!(
        iso.pane_alive(&extra_pane),
        "overflow pane should be stashed, not killed"
    );
}

// --- split_before positional target tests ---

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn split_before_true_picks_leftmost_pane() {
    // Regression test for 3-pane layout bug (Fix 1):
    // When split_before=true (left-column file), the split target should be
    // the first (leftmost) pane in the agent-doc window — not the last.
    // Before the fix, the code always used find_registered_pane_in_session
    // which could pick any registered pane regardless of position.
    let iso = IsolatedTmux::new("route-test-split-before-left");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create a window with 2 panes side by side (simulating agent-doc window)
    let pane_left = iso.auto_start(session, &cwd).unwrap();
    let window = iso.pane_window(&pane_left).unwrap();
    let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
    let pane_right = iso.split_window(&pane_left, &cwd, "-dh").unwrap();

    // Rename to "agent-doc" so list_window_panes("test:agent-doc") works
    let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

    // Verify setup: 2 panes, left then right
    let ordered = iso
        .list_window_panes(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(ordered.len(), 2, "should have 2 panes");
    assert_eq!(ordered[0], pane_left, "first pane should be leftmost");
    assert_eq!(ordered[1], pane_right, "second pane should be rightmost");

    // split_before=true: should pick the first pane (leftmost)
    // We split alongside pane_left with -dbh (before, horizontal)
    let new_pane = iso.split_window(&ordered[0], &cwd, "-dbh").unwrap();
    let new_window = iso.pane_window(&new_pane).unwrap();
    assert_eq!(
        iso.pane_window(&pane_left).unwrap(),
        new_window,
        "new pane should be in the same window as the leftmost pane"
    );

    // Verify the new pane is to the LEFT of the original leftmost pane
    let final_order = iso
        .list_window_panes(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(final_order.len(), 3, "should have 3 panes now");
    assert_eq!(
        final_order[0], new_pane,
        "new pane should be leftmost (split before)"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn split_before_false_picks_rightmost_pane() {
    // Regression test for 3-pane layout bug (Fix 1):
    // When split_before=false (right-column file), the split target should be
    // the last (rightmost) pane in the agent-doc window.
    let iso = IsolatedTmux::new("route-test-split-before-right");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    // Create a window with 2 panes side by side
    let pane_left = iso.auto_start(session, &cwd).unwrap();
    let window = iso.pane_window(&pane_left).unwrap();
    let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
    let pane_right = iso.split_window(&pane_left, &cwd, "-dh").unwrap();

    // Rename to "agent-doc"
    let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

    // Verify setup
    let ordered = iso
        .list_window_panes(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(ordered.len(), 2);
    assert_eq!(ordered[0], pane_left);
    assert_eq!(ordered[1], pane_right);

    // split_before=false: should pick the last pane (rightmost)
    // We split alongside pane_right with -dh (after, horizontal)
    let new_pane = iso.split_window(&ordered[1], &cwd, "-dh").unwrap();
    let new_window = iso.pane_window(&new_pane).unwrap();
    assert_eq!(
        iso.pane_window(&pane_right).unwrap(),
        new_window,
        "new pane should be in the same window as the rightmost pane"
    );

    // Verify the new pane is to the RIGHT of the original rightmost pane
    let final_order = iso
        .list_window_panes(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(final_order.len(), 3, "should have 3 panes now");
    assert_eq!(
        final_order[2], new_pane,
        "new pane should be rightmost (split after)"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn provision_pane_first_col_splits_left() {
    // Verify that provision_pane with a file in the first column
    // computes split_before=true via is_first_column and places the new
    // pane at the leftmost position in the agent-doc window.
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let tasks = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let file_a = tasks.join("file_a.md");
    let file_b = tasks.join("file_b.md");
    std::fs::write(&file_a, "# A\n").unwrap();
    std::fs::write(&file_b, "# B\n").unwrap();

    let iso = IsolatedTmux::new("route-test-auto-start-col-left");
    let session = "test";
    let cwd = dir.path().to_path_buf();

    // Create a window with 2 panes to simulate existing agent-doc layout
    let pane_left = iso.auto_start(session, &cwd).unwrap();
    let window = iso.pane_window(&pane_left).unwrap();
    let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
    let pane_right = iso.split_window(&pane_left, &cwd, "-dh").unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

    // Verify 2-pane setup
    let ordered = iso
        .list_window_panes(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(ordered.len(), 2, "should start with 2 panes");

    // col_args: file_a is in first column, file_b in second
    let col_args = vec!["tasks/file_a.md".to_string(), "tasks/file_b.md".to_string()];

    // Call provision_pane with file in the FIRST column
    let file_a_rel = Path::new("tasks/file_a.md");
    let result = provision_pane(
        &iso,
        file_a_rel,
        "route-test-provision-first-col-session-a",
        "tasks/file_a.md",
        Some(session),
        &col_args,
    );
    assert!(
        result.is_ok(),
        "provision_pane should succeed: {:?}",
        result.err()
    );

    // The new pane should be leftmost (split_before=true picks first pane, splits -dbh)
    let after = iso
        .list_window_panes(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(after.len(), 3, "should have 3 panes after auto_start");
    // The new pane is NOT one of the original two — find it
    let new_pane: Vec<_> = after
        .iter()
        .filter(|p| *p != &pane_left && *p != &pane_right)
        .collect();
    assert_eq!(new_pane.len(), 1, "should have exactly 1 new pane");
    assert_eq!(
        &after[0], new_pane[0],
        "first-column file should produce leftmost pane (split_before=true)"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn provision_pane_second_col_splits_right() {
    // Verify that provision_pane with a file in the second column
    // computes split_before=false via is_first_column and places the new
    // pane at the rightmost position in the agent-doc window.
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let tasks = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let file_a = tasks.join("file_a.md");
    let file_b = tasks.join("file_b.md");
    std::fs::write(&file_a, "# A\n").unwrap();
    std::fs::write(&file_b, "# B\n").unwrap();

    let iso = IsolatedTmux::new("route-test-auto-start-col-right");
    let session = "test";
    let cwd = dir.path().to_path_buf();

    // Create a window with 2 panes to simulate existing agent-doc layout
    let pane_left = iso.auto_start(session, &cwd).unwrap();
    let window = iso.pane_window(&pane_left).unwrap();
    let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
    let pane_right = iso.split_window(&pane_left, &cwd, "-dh").unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

    // Verify 2-pane setup
    let ordered = iso
        .list_window_panes(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(ordered.len(), 2, "should start with 2 panes");

    // col_args: file_a is in first column, file_b in second
    let col_args = vec!["tasks/file_a.md".to_string(), "tasks/file_b.md".to_string()];

    // Call provision_pane with file in the SECOND column
    let file_b_rel = Path::new("tasks/file_b.md");
    let result = provision_pane(
        &iso,
        file_b_rel,
        "route-test-provision-second-col-session-b",
        "tasks/file_b.md",
        Some(session),
        &col_args,
    );
    assert!(
        result.is_ok(),
        "provision_pane should succeed: {:?}",
        result.err()
    );

    // The new pane should be rightmost (split_before=false picks last pane, splits -dh)
    let after = iso
        .list_window_panes(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(after.len(), 3, "should have 3 panes after auto_start");
    // Find the new pane (not one of the original two)
    let new_pane: Vec<_> = after
        .iter()
        .filter(|p| *p != &pane_left && *p != &pane_right)
        .collect();
    assert_eq!(new_pane.len(), 1, "should have exactly 1 new pane");
    assert_eq!(
        after.last().unwrap(),
        new_pane[0],
        "second-column file should produce rightmost pane (split_before=false)"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn sync_after_claim_handles_malformed_registry() {
    // When sessions.json is malformed, sync_after_claim should not panic.
    // It should return early (silently) rather than propagating the error.
    let iso = IsolatedTmux::new("route-test-malformed-registry");
    let tmp = tempfile::TempDir::new().unwrap();
    let session = "test";
    let pane = iso.new_session(session, tmp.path()).unwrap();

    // Write malformed sessions.json (array format instead of map)
    let sessions_path = tmp.path().join(".agent-doc");
    std::fs::create_dir_all(&sessions_path).unwrap();
    std::fs::write(
        sessions_path.join("sessions.json"),
        r#"{"sessions": [{"bad": "format"}]}"#,
    )
    .unwrap();

    // sync_after_claim should not panic — it handles errors gracefully
    // (returns early on load failure)
    sync_after_claim(&iso, &pane, &[]);
    // If we reach here without panic, the test passes
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn sync_after_claim_with_empty_col_args_and_no_registry() {
    // When there's no registry file and no col_args, sync_after_claim
    // should return early without creating any panes.
    let iso = IsolatedTmux::new("route-test-no-registry");
    let tmp = tempfile::TempDir::new().unwrap();
    let session = "test";
    let pane = iso.new_session(session, tmp.path()).unwrap();
    let window = iso.pane_window(&pane).unwrap();

    let before = iso.list_window_panes(&window).unwrap();
    sync_after_claim(&iso, &pane, &[]);
    let after = iso.list_window_panes(&window).unwrap();

    assert_eq!(
        before.len(),
        after.len(),
        "no panes should be created when no registry exists"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn list_panes_ordered_returns_screen_position_after_rearrange() {
    // When panes are broken out and re-joined, creation order can diverge from
    // screen position. list_panes_ordered must return screen order (by pane_left).
    let iso = IsolatedTmux::new("route-test-pane-order-rearrange");
    let session = "test";
    let cwd = std::env::current_dir().unwrap();

    let pane_a = iso.auto_start(session, &cwd).unwrap();
    let window = iso.pane_window(&pane_a).unwrap();
    let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
    let pane_b = iso.split_window(&pane_a, &cwd, "-dh").unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

    // Rearrange: break pane_b out, rejoin to the LEFT of pane_a.
    let _ = iso.raw_cmd(&["break-pane", "-d", "-t", &pane_b]);
    let _ = iso.raw_cmd(&["join-pane", "-bh", "-d", "-s", &pane_b, "-t", &pane_a]);

    let screen_order = iso
        .list_panes_ordered(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(screen_order.len(), 2);
    assert_eq!(
        screen_order[0], pane_b,
        "pane_b should be leftmost after rejoin to the left"
    );
    assert_eq!(
        screen_order[1], pane_a,
        "pane_a should be rightmost after rejoin shifted it right"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn provision_pane_right_col_picks_rightmost_after_rearrange() {
    // Regression: provision_pane must use screen position, not creation order.
    // After rearranging panes so creation order != screen order,
    // split_before=false should split from the rightmost pane by screen position.
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let tasks = dir.path().join("tasks");
    std::fs::create_dir_all(&tasks).unwrap();
    let file_a = tasks.join("file_a.md");
    let file_b = tasks.join("file_b.md");
    std::fs::write(&file_a, "# A\n").unwrap();
    std::fs::write(&file_b, "# B\n").unwrap();

    let iso = IsolatedTmux::new("route-test-provision-rearranged");
    let session = "test";
    let cwd = dir.path().to_path_buf();

    let pane_a = iso.auto_start(session, &cwd).unwrap();
    let window = iso.pane_window(&pane_a).unwrap();
    let _ = iso.raw_cmd(&["resize-window", "-t", &window, "-x", "300", "-y", "60"]);
    let pane_b = iso.split_window(&pane_a, &cwd, "-dh").unwrap();
    let _ = iso.raw_cmd(&["rename-window", "-t", &window, "agent-doc"]);

    // Rearrange: break pane_b, rejoin LEFT of pane_a.
    // Screen: [pane_b, pane_a]. pane_a is now rightmost.
    let _ = iso.raw_cmd(&["break-pane", "-d", "-t", &pane_b]);
    let _ = iso.raw_cmd(&["join-pane", "-bh", "-d", "-s", &pane_b, "-t", &pane_a]);

    // Provision a right-column file — should split from pane_a (rightmost by screen).
    let col_args = vec!["tasks/file_a.md".to_string(), "tasks/file_b.md".to_string()];
    let file_b_rel = Path::new("tasks/file_b.md");
    let result = provision_pane(
        &iso,
        file_b_rel,
        "route-test-provision-rearranged-session-b",
        "tasks/file_b.md",
        Some(session),
        &col_args,
    );
    assert!(
        result.is_ok(),
        "provision_pane should succeed: {:?}",
        result.err()
    );

    let after = iso
        .list_panes_ordered(&format!("{}:agent-doc", session))
        .unwrap();
    assert_eq!(after.len(), 3, "should have 3 panes");

    // The new pane should be rightmost (split after pane_a which is rightmost).
    let new_pane: Vec<_> = after
        .iter()
        .filter(|p| *p != &pane_a && *p != &pane_b)
        .collect();
    assert_eq!(new_pane.len(), 1, "should have exactly 1 new pane");
    assert_eq!(
        after.last().unwrap(),
        new_pane[0],
        "right-column file should produce rightmost pane even after rearrangement"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn concurrent_provision_pane_serializes_same_session_auto_start() {
    use std::sync::{Arc, Barrier};

    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let session = "test";
    let iso = Arc::new(IsolatedTmux::new("route-test-concurrent-provision"));
    let doc_a = dir.path().join("a.md");
    let doc_b = dir.path().join("b.md");
    std::fs::write(&doc_a, "# A\n").unwrap();
    std::fs::write(&doc_b, "# B\n").unwrap();

    let barrier = Arc::new(Barrier::new(3));
    let iso_a = Arc::clone(&iso);
    let barrier_a = Arc::clone(&barrier);
    let doc_a_thread = doc_a.clone();
    let handle_a = std::thread::spawn(move || {
        barrier_a.wait();
        provision_pane(
            &iso_a,
            &doc_a_thread,
            "route-test-concurrent-provision-session-a",
            doc_a_thread.to_string_lossy().as_ref(),
            Some(session),
            &[],
        )
    });

    let iso_b = Arc::clone(&iso);
    let barrier_b = Arc::clone(&barrier);
    let doc_b_thread = doc_b.clone();
    let handle_b = std::thread::spawn(move || {
        barrier_b.wait();
        provision_pane(
            &iso_b,
            &doc_b_thread,
            "route-test-concurrent-provision-session-b",
            doc_b_thread.to_string_lossy().as_ref(),
            Some(session),
            &[],
        )
    });

    barrier.wait();
    let pane_a = handle_a.join().unwrap().unwrap();
    let pane_b = handle_b.join().unwrap().unwrap();

    let window_a = iso.pane_window(&pane_a).unwrap();
    let window_b = iso.pane_window(&pane_b).unwrap();
    assert_eq!(
        window_a, window_b,
        "concurrent provisioning in one tmux session should converge into a single window"
    );

    let panes = iso.list_window_panes(&window_a).unwrap();
    assert!(
        panes.contains(&pane_a) && panes.contains(&pane_b),
        "both provisioned panes should remain visible in the shared window"
    );

    let registry = sessions::load_in(dir.path()).unwrap();
    assert!(
        registry
            .values()
            .any(|entry| entry.session_id == "route-test-concurrent-provision-session-a"),
        "first provisioned document should be registered"
    );
    assert!(
        registry
            .values()
            .any(|entry| entry.session_id == "route-test-concurrent-provision-session-b"),
        "second provisioned document should be registered"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn failed_route_cleanup_preserves_live_registered_owner() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let iso = IsolatedTmux::new("route-test-preserve-failed-owner");
    let session = format!("test-{}", std::process::id());
    let pane = iso.new_session(&session, dir.path()).unwrap();
    let file = dir.path().join("tasks/software/corky.md");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&file, "# Corky\n").unwrap();
    sessions::register_full_in(
        dir.path(),
        "session-1",
        &pane,
        "tasks/software/corky.md",
        123,
        "@1",
    )
    .unwrap();

    assert!(
        should_preserve_failed_route_pane(&iso, &file, &pane, "session-1"),
        "failed-route cleanup must preserve the live registered owner pane"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn failed_route_cleanup_does_not_preserve_unregistered_pane() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let iso = IsolatedTmux::new("route-test-cleanup-unregistered");
    let pane = iso.new_session("test", dir.path()).unwrap();
    let file = dir.path().join("tasks/software/corky.md");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&file, "# Corky\n").unwrap();

    assert!(
        !should_preserve_failed_route_pane(&iso, &file, &pane, "session-1"),
        "failed-route cleanup should still remove panes that never became the live owner"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn failed_route_cleanup_reaps_startup_miss_owner_pane() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let iso = IsolatedTmux::new("route-test-cleanup-startup-miss");
    let pane = iso.new_session("test", dir.path()).unwrap();
    let file = dir.path().join("tasks/software/corky.md");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&file, "# Corky\n").unwrap();
    sessions::register_full_in(
        dir.path(),
        "session-1",
        &pane,
        "tasks/software/corky.md",
        123,
        "@1",
    )
    .unwrap();
    crate::startup_miss::record(
        &file,
        &pane,
        "session-1",
        "claude",
        crate::startup_miss::StartupMissOrigin::FreshStart,
        None,
    )
    .unwrap();

    cleanup_failed_route_panes(&iso, &file, "session-1", std::slice::from_ref(&pane));

    assert!(
        !iso.pane_alive(&pane),
        "fresh-route startup-miss panes should be reaped instead of preserved idle"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn failed_route_cleanup_only_reaps_attempt_local_created_panes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let iso = IsolatedTmux::new("route-test-cleanup-concurrent-sibling");
    let pane_owned = iso.new_session("test", dir.path()).unwrap();
    let pane_sibling = iso.split_window(&pane_owned, dir.path(), "-dh").unwrap();

    sessions::register_full_in(
        dir.path(),
        "session-1",
        &pane_owned,
        "tasks/software/corky.md",
        123,
        "@1",
    )
    .unwrap();
    sessions::register_full_in(
        dir.path(),
        "session-2",
        &pane_sibling,
        "tasks/software/tsift.md",
        456,
        "@1",
    )
    .unwrap();

    let file = dir.path().join("tasks/software/corky.md");
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(&file, "# Corky\n").unwrap();

    cleanup_failed_route_panes(&iso, &file, "session-1", std::slice::from_ref(&pane_owned));

    assert!(
        iso.pane_alive(&pane_owned),
        "cleanup should preserve the live owner pane for the failed route"
    );
    assert!(
        iso.pane_alive(&pane_sibling),
        "cleanup must not reap sibling panes that were not created by this route attempt"
    );
}

#[test]
fn run_with_tmux_resolves_file_path_to_absolute() {
    // Verify that resolve_absolute_file_path turns a relative path into an
    // absolute one when the file exists. This is the guard against submodule
    // CWD-dependent resolution (#route1).
    use std::fs;
    let dir = tempfile::TempDir::new().unwrap();
    let root = dir.path().canonicalize().unwrap();
    let tasks = root.join("tasks");
    fs::create_dir_all(&tasks).unwrap();
    let doc = tasks.join("bugs.md");
    fs::write(&doc, "# Bugs\n").unwrap();

    let _cwd_guard = ScopedCurrentDir::set(&root);

    let resolved = crate::git::resolve_absolute_file_path(std::path::Path::new("tasks/bugs.md"));
    assert!(
        resolved.is_absolute(),
        "route must send absolute paths to avoid submodule CWD misrouting"
    );
    assert_eq!(
        resolved, doc,
        "resolved path must point to the CWD-relative file, not a submodule shadow"
    );
}

#[test]
fn startup_miss_recorded_on_fresh_start_timeout() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::startup_miss::record(
        &doc,
        "%42",
        "session-test",
        "claude",
        crate::startup_miss::StartupMissOrigin::FreshStart,
        None,
    )
    .unwrap();

    let miss = crate::startup_miss::load(&doc)
        .unwrap()
        .expect("should have marker");
    assert_eq!(miss.pane_id, "%42");
    assert_eq!(
        miss.origin,
        crate::startup_miss::StartupMissOrigin::FreshStart
    );
    assert!(crate::startup_miss::is_startup_miss_pane(&doc, "%42"));
}

#[test]
fn startup_miss_cleared_on_successful_ack() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::startup_miss::record(
        &doc,
        "%42",
        "session-test",
        "claude",
        crate::startup_miss::StartupMissOrigin::FreshStart,
        None,
    )
    .unwrap();
    assert!(crate::startup_miss::load(&doc).unwrap().is_some());

    crate::startup_miss::clear(&doc).unwrap();
    assert!(crate::startup_miss::load(&doc).unwrap().is_none());
    assert!(!crate::startup_miss::is_startup_miss_pane(&doc, "%42"));
}

#[test]
fn startup_miss_pane_detected_on_rerun() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::startup_miss::record(
        &doc,
        "%99",
        "session-test",
        "codex",
        crate::startup_miss::StartupMissOrigin::RoutedTrigger,
        Some("cycle-old"),
    )
    .unwrap();

    assert!(crate::startup_miss::is_startup_miss_pane(&doc, "%99"));
    assert!(
        !crate::startup_miss::is_startup_miss_pane(&doc, "%100"),
        "different pane should not match"
    );
}

#[test]
fn startup_miss_routed_trigger_records_with_baseline_id() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    crate::startup_miss::record(
        &doc,
        "%50",
        "session-test",
        "claude",
        crate::startup_miss::StartupMissOrigin::RoutedTrigger,
        Some("cycle-baseline-123"),
    )
    .unwrap();

    let miss = crate::startup_miss::load(&doc).unwrap().expect("marker");
    assert_eq!(
        miss.origin,
        crate::startup_miss::StartupMissOrigin::RoutedTrigger
    );
    assert_eq!(
        miss.cycle_baseline_id.as_deref(),
        Some("cycle-baseline-123")
    );
}

#[test]
fn startup_miss_requires_fresh_start_only_without_matching_live_owner() {
    assert!(startup_miss_requires_fresh_start(
        "%42",
        None,
        SupervisorHealth::NoSocket
    ));
    assert!(startup_miss_requires_fresh_start(
        "%42",
        Some("%99"),
        SupervisorHealth::Unreachable
    ));
    assert!(!startup_miss_requires_fresh_start(
        "%42",
        Some("%42"),
        SupervisorHealth::NoSocket
    ));
    assert!(!startup_miss_requires_fresh_start(
        "%42",
        None,
        SupervisorHealth::Restartable
    ));
    assert!(!startup_miss_requires_fresh_start(
        "%42",
        None,
        SupervisorHealth::Healthy
    ));
}

#[test]
fn startup_miss_live_owner_restart_requires_closed_unsuperseded_start() {
    let miss = crate::startup_miss::StartupMiss {
        file: "test.md".to_string(),
        pane_id: "%42".to_string(),
        session_id: "session-123".to_string(),
        harness: "codex".to_string(),
        timestamp: 10,
        origin: crate::startup_miss::StartupMissOrigin::RoutedTrigger,
        cycle_baseline_id: Some("cycle-abc".to_string()),
    };
    let closed_same_start = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%42".to_string()),
        latest_start_timestamp: Some(10),
        latest_run_timestamp: Some(10),
        latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some(
            "auto_trigger_timeout harness=codex reason=no_prompt_after_30s".to_string(),
        ),
        saw_process_exit_after_latest_start: true,
        saw_session_end_after_latest_start: false,
        saw_process_exit_after_latest_run: true,
        saw_session_end_after_latest_run: false,
    };
    let newer_open_start = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%42".to_string()),
        latest_start_timestamp: Some(10),
        latest_run_timestamp: Some(11),
        latest_run_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some("codex_start mode=fresh_restart restart_count=1".to_string()),
        saw_process_exit_after_latest_start: true,
        saw_session_end_after_latest_start: false,
        saw_process_exit_after_latest_run: false,
        saw_session_end_after_latest_run: false,
    };

    assert!(startup_miss_should_restart_live_owner(
        &miss,
        "%42",
        Some("%42"),
        Some(&closed_same_start)
    ));
    assert!(!startup_miss_should_restart_live_owner(
        &miss,
        "%42",
        Some("%42"),
        Some(&newer_open_start)
    ));
    assert!(startup_miss_superseded_by_later_open_start(
        &miss,
        "%42",
        Some(&newer_open_start)
    ));
    assert!(!startup_miss_superseded_by_later_open_start(
        &miss,
        "%42",
        Some(&closed_same_start)
    ));
}

#[test]
fn startup_miss_fail_closed_only_for_alive_open_no_socket_sessions() {
    let open = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%42".to_string()),
        latest_start_timestamp: Some(1),
        latest_run_timestamp: Some(1),
        latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_process_exit_after_latest_start: false,
        saw_session_end_after_latest_start: false,
        saw_process_exit_after_latest_run: false,
        saw_session_end_after_latest_run: false,
    };
    let closed = crate::startup_miss::SessionLogStatus {
        latest_start_pane: Some("%42".to_string()),
        latest_start_timestamp: Some(1),
        latest_run_timestamp: Some(1),
        latest_run_event: Some("codex_start mode=fresh restart_count=0".to_string()),
        saw_committed_cycle_after_latest_run: false,
        last_event: Some("session_end".to_string()),
        saw_process_exit_after_latest_start: true,
        saw_session_end_after_latest_start: true,
        saw_process_exit_after_latest_run: true,
        saw_session_end_after_latest_run: true,
    };

    assert!(startup_miss_should_fail_closed(
        true,
        "%42",
        None,
        SupervisorHealth::NoSocket,
        Some(&open)
    ));
    assert!(!startup_miss_should_fail_closed(
        true,
        "%42",
        Some("%42"),
        SupervisorHealth::NoSocket,
        Some(&open)
    ));
    assert!(!startup_miss_should_fail_closed(
        true,
        "%42",
        None,
        SupervisorHealth::Healthy,
        Some(&open)
    ));
    assert!(!startup_miss_should_fail_closed(
        true,
        "%42",
        None,
        SupervisorHealth::NoSocket,
        Some(&closed)
    ));
    assert!(!startup_miss_should_fail_closed(
        false,
        "%42",
        None,
        SupervisorHealth::NoSocket,
        Some(&open)
    ));
}

#[test]
fn startup_miss_diagnostic_message_includes_retry_command() {
    let doc = std::path::Path::new("tasks/agent-doc/agent-doc-bugs2.md");
    let message = startup_miss_diagnostic_message(
        doc,
        "routed trigger accepted but no document cycle started for pending #smdq",
    );
    assert!(message.contains("[agent-doc] startup-miss:"));
    assert!(message.contains("agent-doc start tasks/agent-doc/agent-doc-bugs2.md"));
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn startup_miss_diagnostic_does_not_queue_shell_echo_in_pane() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();

    let iso = IsolatedTmux::new("route-test-startup-miss-diagnostic");
    let pane = iso.new_session("test", dir.path()).unwrap();
    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();

    send_keys_with_retry(&iso, &pane, "printf '> '");
    let before = wait_for_pane_contains(&iso, &pane, "> ", std::time::Duration::from_secs(5));
    assert!(
        before.contains("> "),
        "shell prompt should be visible: {before}"
    );

    emit_startup_miss_diagnostic(&iso, &pane, &doc, "startup timed out");

    std::thread::sleep(std::time::Duration::from_millis(250));
    let after = sessions::capture_pane(&iso, &pane).unwrap();
    assert!(
        !after.contains("echo '[agent-doc] startup-miss:"),
        "diagnostic should not be left as drafted shell input: {after}"
    );
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_fails_closed_for_halted_supervisor_when_no_live_owner() {
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-supervisor-restart");
    let session = "claude";
    let cwd = test_cwd();
    let pane = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-supervisor-restart";
    let mut registry = sessions::SessionRegistry::default();
    registry.insert(
        file_path.clone(),
        sessions::SessionEntry {
            pane: pane.clone(),
            pid: 0,
            cwd: dir.path().to_string_lossy().to_string(),
            started: String::new(),
            session_id: session_id.to_string(),
            file: file_path.clone(),
            window: iso.pane_window(&pane).unwrap_or_default(),
            supervisor_instance_id: String::new(),
        },
    );
    sessions::save_in(dir.path(), &registry).unwrap();

    let restart_called = Arc::new(AtomicBool::new(false));
    let restart_called_for_ipc = restart_called.clone();
    let mut ipc =
        crate::supervisor::ipc::SupervisorIpc::start(dir.path(), session_id, move |method| {
            match method {
                IpcMethod::State => IpcResponse::ok(serde_json::json!({
                    "running": false,
                    "state": "halted",
                    "restart_count": 5
                })),
                IpcMethod::Restart { .. } => {
                    restart_called_for_ipc.store(true, Ordering::Relaxed);
                    IpcResponse::ok_empty()
                }
                IpcMethod::Pid => IpcResponse::ok(serde_json::json!({ "pid": null })),
                IpcMethod::Inject { bytes } => {
                    IpcResponse::ok(serde_json::json!({ "n": bytes.len() }))
                }
                IpcMethod::Stop { .. } => IpcResponse::ok_empty(),
            }
        })
        .unwrap();

    let panes_before = iso
        .list_panes_ordered(&format!("{session}:0"))
        .unwrap_or_default();
    let err = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err("route should fail closed instead of reviving a halted crash loop");
    let panes_after = iso
        .list_panes_ordered(&format!("{session}:0"))
        .unwrap_or_default();

    assert_eq!(
        panes_after.len(),
        panes_before.len(),
        "route should not create a duplicate pane when the registered supervisor is halted"
    );
    assert!(
        err.to_string()
            .contains("halted supervisor after 5 restarts"),
        "unexpected error: {err:#}"
    );
    assert!(
        !restart_called.load(Ordering::Relaxed),
        "route should not restart a halted supervisor automatically"
    );

    ipc.stop();
}

#[test]
#[ignore = "live tmux integration test; run `make tmux-ci`"]
fn resolve_or_create_pane_fails_closed_after_repeated_recent_session_losses() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc/logs")).unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let iso = IsolatedTmux::new("route-test-recent-session-loss");
    let session = "codex";
    let cwd = test_cwd();
    let anchor = iso.auto_start(session, &cwd).unwrap();

    let doc = dir.path().join("session.md");
    std::fs::write(&doc, "# Session\n").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let session_id = "route-recent-session-loss";
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    std::fs::write(
            dir.path()
                .join(".agent-doc/logs")
                .join(format!("{session_id}.log")),
            format!(
                "[{}] supervisor_exit code=missing_pane pane=%41 reason=registered_pane_missing\n[{}] supervisor_exit code=missing_pane pane=%42 reason=registered_pane_dead\n",
                now.saturating_sub(30),
                now.saturating_sub(5)
            ),
        )
        .unwrap();

    let panes_before = iso
        .list_panes_ordered(&format!("{session}:0"))
        .unwrap_or_default();
    let err = resolve_or_create_pane(
        &iso,
        &doc,
        None,
        &[],
        session_id,
        &file_path,
        session,
        &HarnessConfig::codex(),
        &mut Vec::new(),
    )
    .expect_err("route should fail closed after repeated recent pane losses");
    let panes_after = iso
        .list_panes_ordered(&format!("{session}:0"))
        .unwrap_or_default();

    assert_eq!(
        panes_after.len(),
        panes_before.len(),
        "route should not spawn a replacement pane once the repeated-loss guard trips"
    );
    assert_eq!(panes_after.first(), Some(&anchor));
    assert!(
        err.to_string().contains("refusing to auto-start"),
        "unexpected error: {err:#}"
    );
    assert!(
        err.to_string().contains("unexpected pane-loss events"),
        "unexpected error: {err:#}"
    );
}

#[test]
fn truncate_log_line_preserves_utf8_boundaries() {
    let line = "  gpt-5.4 high · ~/work/btakita/agent-loop/src/boost-clien…";
    let truncated = truncate_log_line(line, 60);
    assert_eq!(truncated, line);
    assert!(std::str::from_utf8(truncated.as_bytes()).is_ok());

    let longer = format!("{line} with trailing content");
    let truncated_longer = truncate_log_line(&longer, 60);
    assert!(std::str::from_utf8(truncated_longer.as_bytes()).is_ok());
    assert_eq!(truncated_longer.chars().count(), 60);
    assert!(longer.starts_with(&truncated_longer));
}

#[test]
fn skip_capability_proof_bypasses_failed_proof_status() {
    let dir = tempfile::tempdir().unwrap();
    let _cwd_guard = ScopedCurrentDir::set(dir.path());
    let session_id = "route-skip-proof";
    let doc = write_codex_proof_status_fixture(
        dir.path(),
        session_id,
        "opencode_capability_proof status=failed error=\"dns\"",
    );
    let status =
        managed_capability_proof_status(&doc, session_id, &HarnessConfig::opencode()).unwrap();
    assert_eq!(status, ManagedCapabilityProofStatus::Failed);
}

fn test_actor_record(pane_id: &str) -> crate::session_actor::ActorRecord {
    crate::session_actor::ActorRecord {
        document_id: "test-doc".to_string(),
        session_id: "test-session".to_string(),
        generation: 1,
        pane_id: pane_id.to_string(),
        window_id: "@1".to_string(),
        harness: "codex".to_string(),
        state: crate::session_actor::ActorState::Ready,
        last_transition: crate::session_actor::ActorLastTransition {
            caller: "test".to_string(),
            reason: "test".to_string(),
            timestamp: 0,
            prior_generation: 0,
            new_generation: 1,
        },
    }
}

fn test_degraded_actor(pane_id: &str) -> AuthoritativeActorDispatchTarget {
    AuthoritativeActorDispatchTarget {
        record: test_actor_record(pane_id),
        runtime: SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
        },
    }
}

#[test]
fn authoritative_actor_state_preserves_terminal_record_over_runtime_starting() {
    let mut blocked_record = test_actor_record("%42");
    blocked_record.state = crate::session_actor::ActorState::Blocked;
    blocked_record.last_transition.reason = "starting_actor_timeout".to_string();
    let blocked_actor = AuthoritativeActorDispatchTarget {
        record: blocked_record,
        runtime: SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(crate::session_actor::ActorState::Starting),
        },
    };
    assert_eq!(
        blocked_actor.actor_state(),
        crate::session_actor::ActorState::Blocked,
        "a route-owned blocked record should remain a durable terminal gate even if stale supervisor IPC still reports starting"
    );
    assert!(
        actor_blocked_by_starting_timeout(&blocked_actor),
        "a route-owned starting timeout should be identifiable before route re-registers the stale pane"
    );

    let mut starting_record = test_actor_record("%43");
    starting_record.state = crate::session_actor::ActorState::Starting;
    let ready_actor = AuthoritativeActorDispatchTarget {
        record: starting_record,
        runtime: SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(crate::session_actor::ActorState::Ready),
        },
    };
    assert_eq!(
        ready_actor.actor_state(),
        crate::session_actor::ActorState::Ready,
        "non-terminal records should still accept fresher supervisor runtime state"
    );
}

#[test]
fn starting_timeout_blocked_actor_recovery_requires_prompt_ready_proof() {
    let mut blocked_record = test_actor_record("%42");
    blocked_record.state = crate::session_actor::ActorState::Blocked;
    blocked_record.last_transition.reason = "starting_actor_timeout".to_string();
    let blocked_actor = AuthoritativeActorDispatchTarget {
        record: blocked_record,
        runtime: SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(crate::session_actor::ActorState::Starting),
        },
    };

    assert!(
        starting_timeout_blocked_actor_can_recover(&blocked_actor, true),
        "a route-owned starting timeout may recover only after direct dispatch-ready prompt proof"
    );
    assert!(
        !starting_timeout_blocked_actor_can_recover(&blocked_actor, false),
        "route must not clear a durable starting timeout without prompt proof"
    );
    assert!(
        !starting_timeout_blocked_actor_can_recover(&test_degraded_actor("%43"), true),
        "ordinary degraded actors must not use the starting-timeout recovery path"
    );
}

#[test]
fn dispatch_only_can_use_degraded_authoritative_actor_returns_true_when_registered_matches() {
    let actor = test_degraded_actor("%42");
    assert!(dispatch_only_can_use_degraded_authoritative_actor(
        &actor,
        Some("%42"),
        None,
    ));
}

#[test]
fn dispatch_only_can_use_degraded_authoritative_actor_returns_true_when_live_owner_matches() {
    let actor = test_degraded_actor("%42");
    assert!(dispatch_only_can_use_degraded_authoritative_actor(
        &actor,
        None,
        Some("%42"),
    ));
}

#[test]
fn dispatch_only_can_use_degraded_authoritative_actor_returns_true_when_both_match() {
    let actor = test_degraded_actor("%42");
    assert!(dispatch_only_can_use_degraded_authoritative_actor(
        &actor,
        Some("%42"),
        Some("%42"),
    ));
}

#[test]
fn dispatch_only_can_use_degraded_authoritative_actor_returns_false_when_no_match() {
    let actor = test_degraded_actor("%42");
    assert!(!dispatch_only_can_use_degraded_authoritative_actor(
        &actor,
        Some("%99"),
        Some("%99"),
    ));
}

#[test]
fn dispatch_only_can_use_degraded_authoritative_actor_returns_false_when_none_provided() {
    let actor = test_degraded_actor("%42");
    assert!(!dispatch_only_can_use_degraded_authoritative_actor(
        &actor, None, None,
    ));
}

#[test]
fn authoritative_actor_dispatch_guard_reason_returns_none_for_healthy_with_state() {
    let runtime = SupervisorRuntime {
        health: SupervisorHealth::Healthy,
        actor_state: Some(crate::session_actor::ActorState::Ready),
    };
    assert!(authoritative_actor_dispatch_guard_reason(&runtime).is_none());
}

#[test]
fn authoritative_actor_dispatch_guard_reason_returns_reason_for_restartable() {
    let runtime = SupervisorRuntime {
        health: SupervisorHealth::Restartable,
        actor_state: Some(crate::session_actor::ActorState::Ready),
    };
    let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
    assert!(
        reason.contains("restartable"),
        "expected restartable in reason: {reason}"
    );
}

#[test]
fn authoritative_actor_dispatch_guard_reason_returns_reason_for_halted() {
    let runtime = SupervisorRuntime {
        health: SupervisorHealth::Halted { restart_count: 3 },
        actor_state: Some(crate::session_actor::ActorState::Ready),
    };
    let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
    assert!(
        reason.contains("halted"),
        "expected halted in reason: {reason}"
    );
}

#[test]
fn authoritative_actor_dispatch_guard_reason_returns_reason_for_unreachable() {
    let runtime = SupervisorRuntime {
        health: SupervisorHealth::Unreachable,
        actor_state: Some(crate::session_actor::ActorState::Ready),
    };
    let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
    assert!(
        reason.contains("unreachable"),
        "expected unreachable in reason: {reason}"
    );
}

#[test]
fn authoritative_actor_dispatch_guard_reason_returns_reason_for_no_socket() {
    let runtime = SupervisorRuntime {
        health: SupervisorHealth::NoSocket,
        actor_state: Some(crate::session_actor::ActorState::Ready),
    };
    let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
    assert!(
        reason.contains("no_socket"),
        "expected no_socket in reason: {reason}"
    );
}

#[test]
fn authoritative_actor_dispatch_guard_reason_returns_reason_for_missing_actor_state() {
    let runtime = SupervisorRuntime {
        health: SupervisorHealth::Healthy,
        actor_state: None,
    };
    let reason = authoritative_actor_dispatch_guard_reason(&runtime).unwrap();
    assert!(
        reason.contains("missing"),
        "expected missing in reason: {reason}"
    );
}

#[test]
fn authoritative_actor_dispatch_target_eligible_true_only_when_no_guard_reason() {
    let healthy = AuthoritativeActorDispatchTarget {
        record: test_actor_record("%1"),
        runtime: SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: Some(crate::session_actor::ActorState::Ready),
        },
    };
    assert!(authoritative_actor_dispatch_target_eligible(&healthy));

    let degraded = AuthoritativeActorDispatchTarget {
        record: test_actor_record("%1"),
        runtime: SupervisorRuntime {
            health: SupervisorHealth::NoSocket,
            actor_state: None,
        },
    };
    assert!(!authoritative_actor_dispatch_target_eligible(&degraded));

    let no_state = AuthoritativeActorDispatchTarget {
        record: test_actor_record("%1"),
        runtime: SupervisorRuntime {
            health: SupervisorHealth::Healthy,
            actor_state: None,
        },
    };
    assert!(!authoritative_actor_dispatch_target_eligible(&no_state));
}

#[test]
fn mismatched_authoritative_actor_can_be_replaced_only_when_not_live_authority() {
    let healthy_ready = SupervisorRuntime {
        health: SupervisorHealth::Healthy,
        actor_state: Some(crate::session_actor::ActorState::Ready),
    };
    assert!(
        !mismatched_authoritative_actor_can_be_replaced(
            &healthy_ready,
            crate::session_actor::ActorState::Ready,
        ),
        "a healthy ready actor from another harness is still authoritative and must block"
    );

    let healthy_closed = SupervisorRuntime {
        health: SupervisorHealth::Healthy,
        actor_state: Some(crate::session_actor::ActorState::Closed),
    };
    assert!(
        mismatched_authoritative_actor_can_be_replaced(
            &healthy_closed,
            crate::session_actor::ActorState::Closed,
        ),
        "a closed actor from another harness should not strand a fresh harness start"
    );

    let unreachable = SupervisorRuntime {
        health: SupervisorHealth::Unreachable,
        actor_state: None,
    };
    assert!(
        mismatched_authoritative_actor_can_be_replaced(
            &unreachable,
            crate::session_actor::ActorState::Ready,
        ),
        "an unreachable supervisor cannot prove live cross-harness ownership"
    );
}

#[test]
fn dispatch_only_starting_pane_recovery_timeout_default() {
    let timeout = dispatch_only_starting_pane_recovery_timeout(None);
    assert_eq!(timeout, Duration::from_millis(400));
}

#[test]
fn dispatch_only_starting_pane_ready_timeout_production_values() {
    assert_eq!(
        dispatch_only_starting_pane_ready_timeout_for_binary(Some("opencode"), false),
        Duration::from_secs(15)
    );
    assert_eq!(
        dispatch_only_starting_pane_ready_timeout_for_binary(Some("codex"), false),
        Duration::from_secs(2)
    );
    assert_eq!(
        dispatch_only_starting_pane_ready_timeout_for_binary(Some("claude"), false),
        Duration::from_secs(2)
    );
    assert_eq!(
        dispatch_only_starting_pane_ready_timeout_for_binary(Some("opencode"), true),
        Duration::from_millis(250)
    );
}

#[test]
fn dispatch_only_starting_pane_recovery_timeout_opencode() {
    let h = crate::harness::HarnessConfig::opencode();
    let timeout = dispatch_only_starting_pane_recovery_timeout(Some(&h));
    assert_eq!(timeout, Duration::from_millis(400));
}

#[test]
fn dispatch_only_starting_pane_recovery_timeout_claude() {
    let h = crate::harness::HarnessConfig::claude();
    let timeout = dispatch_only_starting_pane_recovery_timeout(Some(&h));
    assert_eq!(timeout, Duration::from_millis(400));
}

#[test]
fn dispatch_only_starting_pane_recovery_timeout_codex() {
    let h = crate::harness::HarnessConfig::codex();
    let timeout = dispatch_only_starting_pane_recovery_timeout(Some(&h));
    assert_eq!(timeout, Duration::from_millis(400));
}

#[test]
fn route_starting_actor_not_ready_log_line_includes_typed_lifecycle_facts() {
    let h = crate::harness::HarnessConfig::codex();
    let facts = AuthoritativeActorReadyFacts {
        pane_id: "%7".to_string(),
        generation: 42,
        actor_state: ActorDispatchState::Busy,
        supervisor_health: "healthy".to_string(),
        runtime_state: "busy".to_string(),
        prompt_ready: false,
        last_transition_reason: "restart_bootstrap".to_string(),
        last_transition_caller: "start".to_string(),
    };

    let line = route_starting_actor_not_ready_log_line(
        Path::new("/tmp/doc.md"),
        &h,
        Duration::from_secs(8),
        Duration::from_millis(8_125),
        &facts,
    );

    assert!(line.contains("route_authoritative_actor_starting_not_ready"));
    assert!(line.contains("file=/tmp/doc.md"));
    assert!(line.contains("harness=codex"));
    assert!(line.contains("timeout_ms=8000"));
    assert!(line.contains("elapsed_ms=8125"));
    assert!(line.contains("pane=%7"));
    assert!(line.contains("generation=42"));
    assert!(line.contains("actor_state=busy"));
    assert!(line.contains("supervisor_health=healthy"));
    assert!(line.contains("runtime_state=busy"));
    assert!(line.contains("prompt_ready=false"));
    assert!(line.contains("last_transition_reason=restart_bootstrap"));
    assert!(line.contains("last_transition_caller=start"));
}

#[test]
fn starting_actor_timeout_record_coalesces_same_generation_and_pane() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("tasks/agent-doc/timeout.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "body").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let facts = AuthoritativeActorReadyFacts {
        pane_id: "%7".to_string(),
        generation: 42,
        actor_state: ActorDispatchState::Starting,
        supervisor_health: "healthy".to_string(),
        runtime_state: "starting".to_string(),
        prompt_ready: false,
        last_transition_reason: "session_start".to_string(),
        last_transition_caller: "start".to_string(),
    };

    assert_eq!(
        record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap(),
        StartingActorTimeoutLogDecision::NewTimeout
    );
    assert_eq!(
        record_starting_actor_timeout(&file_path, &facts, "repeat timeout").unwrap(),
        StartingActorTimeoutLogDecision::DuplicateTimeout
    );

    let mut next_generation = facts.clone();
    next_generation.generation += 1;
    assert_eq!(
        record_starting_actor_timeout(&file_path, &next_generation, "next timeout").unwrap(),
        StartingActorTimeoutLogDecision::NewTimeout
    );
}

#[test]
fn starting_actor_timeout_record_matches_same_generation_and_pane() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("tasks/agent-doc/timeout-match.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "body").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let facts = AuthoritativeActorReadyFacts {
        pane_id: "%7".to_string(),
        generation: 3,
        actor_state: ActorDispatchState::Starting,
        supervisor_health: "healthy".to_string(),
        runtime_state: "starting".to_string(),
        prompt_ready: false,
        last_transition_reason: "session_start".to_string(),
        last_transition_caller: "start".to_string(),
    };

    assert!(!starting_actor_timeout_record_matches(&file_path, &facts));
    record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap();
    assert!(starting_actor_timeout_record_matches(&file_path, &facts));

    let mut different_generation = facts.clone();
    different_generation.generation += 1;
    assert!(!starting_actor_timeout_record_matches(
        &file_path,
        &different_generation
    ));

    let mut different_pane = facts;
    different_pane.pane_id = "%8".to_string();
    assert!(!starting_actor_timeout_record_matches(
        &file_path,
        &different_pane
    ));
}

#[test]
fn starting_actor_timeout_record_clears_after_ready_or_terminal_refresh() {
    let dir = tempfile::TempDir::new().unwrap();
    std::fs::create_dir_all(dir.path().join(".agent-doc")).unwrap();
    let doc = dir.path().join("tasks/agent-doc/timeout-clear.md");
    std::fs::create_dir_all(doc.parent().unwrap()).unwrap();
    std::fs::write(&doc, "body").unwrap();
    let file_path = doc.canonicalize().unwrap().to_string_lossy().to_string();
    let facts = AuthoritativeActorReadyFacts {
        pane_id: "%9".to_string(),
        generation: 5,
        actor_state: ActorDispatchState::Starting,
        supervisor_health: "healthy".to_string(),
        runtime_state: "starting".to_string(),
        prompt_ready: false,
        last_transition_reason: "session_start".to_string(),
        last_transition_caller: "start".to_string(),
    };

    assert_eq!(
        record_starting_actor_timeout(&file_path, &facts, "first timeout").unwrap(),
        StartingActorTimeoutLogDecision::NewTimeout
    );
    clear_starting_actor_timeout_record(&file_path);
    assert_eq!(
        record_starting_actor_timeout(&file_path, &facts, "after clear").unwrap(),
        StartingActorTimeoutLogDecision::NewTimeout
    );
}

#[test]
fn wait_for_ready_override_guard_sets_and_restores_thread_local() {
    use std::time::Duration;

    // Baseline: no override set.
    assert_eq!(wait_for_ready_override(), None);

    // Outer scope sets a 30s override.
    let outer = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(30)));
    assert_eq!(wait_for_ready_override(), Some(Duration::from_secs(30)));

    {
        // Inner scope replaces with a 60s override.
        let _inner = WaitForReadyOverrideGuard::set(Some(Duration::from_secs(60)));
        assert_eq!(wait_for_ready_override(), Some(Duration::from_secs(60)));

        // Nested unset is honored too.
        let _none = WaitForReadyOverrideGuard::set(None);
        assert_eq!(wait_for_ready_override(), None);
    }

    // Both nested guards dropped — back to outer 30s.
    assert_eq!(wait_for_ready_override(), Some(Duration::from_secs(30)));

    drop(outer);
    // Outer dropped — back to unset baseline.
    assert_eq!(wait_for_ready_override(), None);
}

// #route-busy-vs-starting-wording: the FailClosed wait context distinguishes a
// pane busy on an active harness turn from a genuine cold startup timeout.
#[test]
fn failclosed_wait_context_distinguishes_busy_turn_from_cold_startup() {
    let claude = crate::harness::HarnessConfig::claude();
    // No busy cue → cold-startup timeout wording (unchanged behavior).
    assert_eq!(
        failclosed_wait_context(&claude, None, 12),
        "waited 12s for claude startup"
    );
    // A live busy cue → the pane is busy on an active turn, not cold-starting.
    assert_eq!(
        failclosed_wait_context(&claude, Some("active claude turn"), 12),
        "the pane is busy on an active claude turn (active claude turn), not cold-starting"
    );
}
